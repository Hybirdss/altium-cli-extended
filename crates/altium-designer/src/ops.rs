//! Typed operations on a running Altium Designer — the API the CLI uses. Hides which host
//! runs Altium, start-up, project staging (copy to a VM / path mapping for Wine) and the
//! bridge protocol.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{Value, json};

use altium_core::{Error, ErrorCode, Result};
use altium_doc::project::Project;
use altium_format::DocKind;

use crate::bridge::{Bridge, Request, Response};
use crate::config::Config;
use crate::host::{Host, Staged};
use crate::install::discover;
use crate::local::LocalHost;
use crate::ssh::{SshConfig, SshHost};
use crate::wine::WineHost;

/// Result of [`Designer::call_with`].
pub struct Called {
    pub resp: Response,
    pub staged: Option<Staged>,
    /// Local files Altium created or changed (brought back from an SSH host).
    pub pulled: Vec<PathBuf>,
}

/// Replace every `{"$c": N}` (Altium internal units from the bridge) with the length in the
/// output unit (`--units`).
pub fn convert_coords(v: &mut Value) {
    match v {
        Value::Object(o) => {
            if o.len() == 1
                && let Some(n) = o.get("$c").and_then(Value::as_i64)
            {
                *v = json!(altium_core::Coord(n).out());
                return;
            }
            for x in o.values_mut() {
                convert_coords(x);
            }
        }
        Value::Array(a) => a.iter_mut().for_each(convert_coords),
        _ => {}
    }
}

/// Rewrite Altium-side paths under the staged root (e.g. `C:\altium-cli-extended\work\X-1234\a.SchDoc`)
/// as the local paths they mirror, so results point at the caller's files.
pub fn localize_paths(v: &mut Value, st: &Staged) {
    if !st.copied {
        return;
    }
    let root = st.win_root.to_ascii_lowercase();
    match v {
        Value::String(s) => {
            let low = s.to_ascii_lowercase();
            if low == root || low.starts_with(&format!("{root}\\")) {
                let rest = s[root.len()..].trim_start_matches('\\').replace('\\', "/");
                *s = if rest.is_empty() { st.local_root.display().to_string() } else { st.local_root.join(rest).display().to_string() };
            }
        }
        Value::Object(o) => o.values_mut().for_each(|x| localize_paths(x, st)),
        Value::Array(a) => a.iter_mut().for_each(|x| localize_paths(x, st)),
        _ => {}
    }
}

pub struct Designer {
    pub cfg: Config,
    pub host: Box<dyn Host>,
    write_opts: altium_format::fsx::WriteOpts,
}

#[derive(Debug, Clone, Serialize)]
pub struct Status {
    pub host: &'static str,
    pub running: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    pub altium_dir: String,
    pub bridge_installed: bool,
    pub bridge_outdated: bool,
    pub details: Value,
}

/// Which host runs Altium: the `host` setting, else `local` on Windows, `ssh` when an SSH host
/// is configured, `wine` otherwise.
pub fn host_choice(cfg: &Config) -> &'static str {
    match cfg.host.as_deref() {
        Some("local") => "local",
        Some("ssh") => "ssh",
        Some("wine") | Some("bottles") => "wine",
        _ if cfg!(windows) => "local",
        _ if cfg.ssh_host.is_some() => "ssh",
        _ => "wine",
    }
}

/// The project a document belongs to: a `.PrjPcb` in its folder (or up to two levels above)
/// that lists it.
pub fn owning_project(doc: &Path) -> Option<PathBuf> {
    let abs = crate::paths::absolute(doc);
    let mut dir = abs.parent()?.to_path_buf();
    for _ in 0..3 {
        let mut found: Vec<PathBuf> = std::fs::read_dir(&dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| DocKind::of(p) == DocKind::Project)
            .collect();
        found.sort();
        for prj in found {
            if let Ok(p) = Project::load(&prj)
                && p.documents.iter().any(|d| crate::paths::absolute(&d.path) == abs)
            {
                return Some(prj);
            }
        }
        if !dir.pop() {
            break;
        }
    }
    None
}

/// What staging makes visible to Altium for one file.
#[derive(Debug, Clone, PartialEq)]
pub struct StageSet {
    /// The folder Altium sees the files under.
    pub root: PathBuf,
    /// Absolute paths under `root`; only these are sent to a remote host.
    pub files: Vec<PathBuf>,
}

/// How many folders above the project folder a referenced document may sit (`..\Utils\`).
const MAX_CLIMB: usize = 2;

/// For a project, the project file and every document it references (`..\Utils\` sheets
/// included) under the deepest folder holding them all; for a document of a project, that
/// project's set (so Altium sees one copy of the project); otherwise the file in its folder.
pub fn stage_set(file: &Path) -> Result<StageSet> {
    let abs = crate::paths::resolve(file);
    let parent = abs.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("/"));
    if DocKind::of(&abs) != DocKind::Project {
        if let Some(prj) = owning_project(&abs) {
            let set = stage_set(&prj)?;
            if abs.starts_with(&set.root) {
                return Ok(set);
            }
        }
        return Ok(StageSet { root: parent, files: vec![abs] });
    }
    let Ok(p) = Project::load(&abs) else { return Ok(StageSet { root: parent, files: vec![abs] }) };
    let docs: Vec<PathBuf> = p.documents.iter().filter(|d| d.exists).map(|d| crate::paths::resolve(&d.path)).collect();
    let root = project_root(&parent, &docs, &crate::config::home())?;
    Ok(StageSet { root, files: std::iter::once(abs).chain(docs).collect() })
}

/// The deepest folder holding `project_dir` and every document. Refused when that is more than
/// [`MAX_CLIMB`] folders up, or when climbing reaches `home` or above it: staging would then
/// cover unrelated files.
fn project_root(project_dir: &Path, docs: &[PathBuf], home: &Path) -> Result<PathBuf> {
    let mut root = project_dir.to_path_buf();
    let mut climbed = 0;
    for d in docs {
        while !d.starts_with(&root) && root.pop() {
            climbed += 1;
        }
    }
    if climbed > MAX_CLIMB || (climbed > 0 && home.starts_with(&root)) {
        let far = docs.iter().find(|d| !d.starts_with(project_dir)).unwrap_or(&root);
        return Err(Error::new(ErrorCode::Unsafe, format!("the project references {}, too far outside its folder {} to stage", far.display(), project_dir.display()))
            .hint(format!("keep project documents at most {MAX_CLIMB} folders above the project folder and below your home folder; move shared sheets next to the project and update their paths")));
    }
    Ok(root)
}

fn clock_file(root: &Path) -> PathBuf {
    let key: u64 = root.to_string_lossy().bytes().fold(0xcbf29ce484222325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3));
    crate::config::state_dir().join(format!("sync/{key:016x}.clock"))
}

fn writes_design(req: &Request) -> bool {
    match req.op.as_str() {
        "annotate" | "pcb_edit" | "pcb_rule" | "sch_param" | "sch_place" | "sch_label" | "sch_wire" | "save" => true,
        "pcb_polygons" => req.params.get("action").is_some_and(|action| !action.is_empty() && action != "list"),
        "pcb_classes" => req.params.get("name").is_some_and(|name| !name.is_empty()),
        _ => false,
    }
}

impl Designer {
    /// Select and connect to the configured host, or fail with DESIGNER_UNAVAILABLE.
    pub fn open() -> Result<Designer> {
        let cfg = Config::load()?;
        let host: Box<dyn Host> = match host_choice(&cfg) {
            "local" => Box::new(LocalHost::new(&cfg)?),
            "ssh" => {
                let sc = SshConfig::from(&cfg).ok_or_else(|| {
                    Error::new(ErrorCode::DesignerUnavailable, "host=ssh but no ssh_host is configured")
                        .hint("`ace designer config --set ssh_host=<address> --set ssh_user=<user>`")
                })?;
                Box::new(SshHost::connect(sc)?)
            }
            _ => {
                let d = discover(&cfg);
                match d.installation {
                    Some(inst) => Box::new(WineHost::new(inst)),
                    None => {
                        let failed = d.checks.iter().find(|c| !c.ok);
                        let mut e = Error::new(
                            ErrorCode::DesignerUnavailable,
                            failed.map(|c| format!("{}: {}", c.name, c.detail)).unwrap_or_else(|| "Altium Designer not found".into()),
                        )
                        .hint("Altium Designer runs on Windows: use this CLI on that machine, or `ace designer config --set host=ssh --set ssh_host=...`");
                        if let Some(h) = failed.and_then(|c| c.hint.clone()) {
                            e = e.hint(h);
                        }
                        return Err(e.details(json!({"checks": d.checks})));
                    }
                }
            }
        };
        Ok(Designer { cfg, host, write_opts: Default::default() })
    }

    pub fn with_write_options(mut self, opts: altium_format::fsx::WriteOpts) -> Self {
        self.write_opts = opts;
        self
    }

    fn backup_remote(&self, path: &str) -> Result<()> {
        if self.write_opts.dry_run || self.write_opts.no_backup { return Ok(()) }
        let backup = format!("{path}.bak");
        if !self.host.exists(&backup)? {
            let data = self.host.read(path)?.ok_or_else(|| Error::not_found(format!("cannot back up {path}")).hint("check that the design exists on the Altium host"))?;
            self.host.write(&backup, &data)?;
        }
        Ok(())
    }

    pub fn bridge(&self) -> Bridge<'_> {
        Bridge::new(self.host.as_ref())
    }

    pub fn status(&self) -> Result<Status> {
        let pid = self.host.altium_pid()?;
        let b = self.bridge();
        Ok(Status {
            host: self.host.kind(),
            running: pid.is_some(),
            pid,
            altium_dir: self.host.altium_dir().to_owned(),
            bridge_installed: b.installed(),
            bridge_outdated: b.outdated(),
            details: self.host.describe(),
        })
    }

    fn timeout(&self, explicit: Option<u64>) -> Result<Duration> {
        let s = match explicit {
            Some(s) => s,
            None if self.host.altium_pid()?.is_some() => self.cfg.request_timeout(),
            None => self.cfg.start_timeout() + self.cfg.request_timeout(),
        };
        Ok(Duration::from_secs(s))
    }

    pub fn call(&self, req: Request, timeout: Option<u64>) -> Result<Response> {
        let t = self.timeout(timeout)?;
        self.bridge().call(&req, t)
    }

    /// Make `file` (and its project) available to Altium; remembers the Altium-side clock so
    /// [`Designer::pull`] knows what changed afterwards.
    /// The clock is only taken the first time: it advances when changes are pulled, so edits
    /// Altium made are never skipped by a later staging.
    pub fn stage(&self, file: &Path) -> Result<Staged> {
        self.stage_files(&stage_set(file)?, file)
    }

    fn stage_files(&self, set: &StageSet, file: &Path) -> Result<Staged> {
        let cf = clock_file(&crate::paths::absolute(&set.root));
        if !cf.exists() {
            let clock = self.host.clock()?;
            if let Some(d) = cf.parent() {
                std::fs::create_dir_all(d)?;
            }
            std::fs::write(&cf, clock)?;
        }
        self.host.stage_in(set, file)
    }

    /// Bring back what Altium wrote under the staged root since the last pull.
    fn pull_staged(&self, st: &Staged) -> Result<Vec<PathBuf>> {
        let cf = clock_file(&st.local_root);
        let since = std::fs::read_to_string(&cf).unwrap_or_default();
        let (files, now) = self.host.stage_out(st, since.trim(), self.write_opts)?;
        if !now.is_empty() && !self.write_opts.dry_run {
            std::fs::write(cf, now)?;
        }
        Ok(files)
    }

    /// Copy back what Altium changed in `file`'s project since it was last staged.
    pub fn pull(&self, file: &Path) -> Result<Vec<PathBuf>> {
        let root = stage_set(file)?.root;
        std::fs::read_to_string(clock_file(&crate::paths::absolute(&root))).map_err(|_| {
            Error::new(ErrorCode::NotFound, "this project was never sent to Altium").hint("run a designer command on it (or `ace designer push`) first")
        })?;
        let st = self.host.staged_location(&root, file)?;
        self.pull_staged(&st)
    }

    /// Launch Altium with no command; optionally wait until the bridge answers.
    pub fn start(&self, wait: bool, timeout: Option<u64>) -> Result<Value> {
        if !wait {
            if self.host.altium_pid()?.is_some() {
                return Ok(json!({"started": false, "already_running": true}));
            }
            self.host.launch_x2(&[])?;
            return Ok(json!({"started": true}));
        }
        let t0 = Instant::now();
        let r = self.call(Request::new("ping"), timeout)?;
        Ok(json!({"ready": true, "waited_ms": t0.elapsed().as_millis() as u64, "ping": r.data}))
    }

    /// Stop Altium. Without `force`, documents are saved first.
    pub fn stop(&self, force: bool) -> Result<Value> {
        if self.host.altium_pid()?.is_none() {
            return Ok(json!({"stopped": false, "was_running": false}));
        }
        let saved = if force { Value::Null } else { self.save_all()?.data };
        let n = self.host.kill_altium()?;
        let t0 = Instant::now();
        while self.host.altium_pid()?.is_some() && t0.elapsed() < Duration::from_secs(20) {
            std::thread::sleep(Duration::from_millis(500));
        }
        Ok(json!({"stopped": self.host.altium_pid()?.is_none(), "signalled": n, "saved": saved}))
    }

    /// Send `req` with design files made visible to Altium first. `files` are (parameter,
    /// local path) pairs; all must live under the first file's staging root. Returns the
    /// response (coordinates converted to the output unit), the staging, and, when `pull` is
    /// set, the local files Altium changed (copied back from an SSH host).
    pub fn call_with(&self, mut req: Request, files: &[(&str, &Path)], pull: bool, timeout: Option<u64>) -> Result<Called> {
        let Some((_, first)) = files.first() else {
            let mut r = self.call(req, timeout)?;
            convert_coords(&mut r.data);
            return Ok(Called { resp: r, staged: None, pulled: vec![] });
        };
        let mut set = stage_set(first)?;
        let root = set.root.clone();
        set.files.extend(files[1..].iter().map(|(_, f)| crate::paths::resolve(f)).filter(|f| f.starts_with(&root)));
        let st = self.stage_files(&set, first)?;
        for (k, f) in files {
            let abs = crate::paths::absolute(f);
            let rel = abs.strip_prefix(&st.local_root).map_err(|_| {
                Error::usage(format!("{} is not inside {}", abs.display(), root.display())).hint("keep every file of the operation in the project folder")
            })?;
            let win = if rel.as_os_str().is_empty() {
                st.win_root.clone()
            } else {
                format!(r"{}\{}", st.win_root, rel.to_string_lossy().replace('/', "\\"))
            };
            req = req.param(k, win);
        }
        if writes_design(&req) && req.params.get("dry_run").is_none_or(|v| v != "1") {
            for (key, file) in files {
                if let Some(path) = req.params.get(*key) { self.backup_remote(path)?; }
                if DocKind::of(file) == DocKind::Project {
                    let project = Project::load(file)?;
                    for doc in project.documents.iter().filter(|d| d.exists) {
                        let absolute = crate::paths::absolute(&doc.path);
                        let rel = absolute.strip_prefix(&st.local_root).map_err(|_| Error::usage("project document is outside staging root"))?;
                        self.backup_remote(&format!(r"{}\{}", st.win_root, rel.to_string_lossy().replace('/', "\\")))?;
                    }
                }
            }
        }
        let mut resp = self.call(req, timeout)?;
        convert_coords(&mut resp.data);
        localize_paths(&mut resp.data, &st);
        let pulled = if pull {
            self.pull_staged(&st)?
        } else {
            vec![]
        };
        Ok(Called { resp, staged: Some(st), pulled })
    }

    pub fn ping(&self, timeout: Option<u64>) -> Result<Response> {
        self.call(Request::new("ping"), timeout)
    }

    pub fn documents(&self) -> Result<Response> {
        self.call(Request::new("documents"), None)
    }

    pub fn open_doc(&self, path: &Path) -> Result<Called> {
        let kind = if DocKind::of(path) == DocKind::Project { "Project" } else { "Document" };
        self.call_with(Request::new("open").param("kind", kind), &[("path", path)], false, None)
    }

    /// Compile; violations carry severity, descriptor, detail and the related objects.
    pub fn compile(&self, project: &Path, timeout: Option<u64>) -> Result<Called> {
        self.call_with(Request::new("compile"), &[("project", project)], false, timeout)
    }

    /// The compiled netlist as Altium computes it (components with parameters, nets with pins).
    pub fn netlist(&self, project: &Path, timeout: Option<u64>) -> Result<Called> {
        self.call_with(Request::new("netlist"), &[("project", project)], false, timeout)
    }

    pub fn variants(&self, project: &Path) -> Result<Called> {
        self.call_with(Request::new("variants"), &[("project", project)], false, None)
    }

    /// Annotate; changed documents are saved by Altium and copied back.
    pub fn annotate(&self, project: &Path, mode: &str, timeout: Option<u64>) -> Result<Called> {
        let r = self.call_with(Request::new("annotate").param("mode", mode), &[("project", project)], false, timeout)?;
        self.save_and_pull(project, r)
    }

    /// Save every document Altium changed, then bring the files back.
    pub fn save_and_pull(&self, file: &Path, mut called: Called) -> Result<Called> {
        called.pulled = self.pull(file)?;
        Ok(called)
    }

    /// Run the design rule check. `process`/`params` override how DRC is started.
    pub fn drc(&self, pcb: &Path, process: Option<&str>, params: Option<&str>, timeout: Option<u64>) -> Result<Called> {
        // PCB:DesignRuleCheck is the Tools > Design Rule Check dialog: the host presses its
        // "Run Design Rule Check" button, then the script collects the violations.
        let r = Request::new("drc")
            .param("process", process.unwrap_or("PCB:DesignRuleCheck"))
            .opt("params", params)
            .answer("Design Rule Check", &["Run Design Rule Check...", "Run Design Rule Check"]);
        self.call_with(r, &[("pcb", pcb)], false, timeout)
    }

    /// Run an OutJob medium; returns the response and the generated files (copied back from an
    /// SSH host, or found by modification time locally).
    pub fn outjob(&self, project: &Path, outjob: &Path, medium: Option<&str>, action: &str, timeout: Option<u64>) -> Result<Called> {
        let root = stage_set(project)?.root;
        if !crate::paths::absolute(outjob).starts_with(&root) {
            return Err(Error::usage("the OutJob must be inside the project folder").hint("copy it next to the .PrjPcb"));
        }
        let r = Request::new("outjob").param("action", action).opt("medium", medium);
        self.call_with(r, &[("project", project), ("outjob", outjob)], true, timeout)
    }

    /// Generate standard outputs without an OutJob of the user's: a temporary OutJob is written
    /// on Altium's side, added to the project for the run, and removed again. Generated files
    /// land in `<project>/Project Outputs/altium-cli-extended/<stamp>/` and are copied back.
    pub fn export(&self, project: &Path, outputs: Vec<altium_doc::outjob::NewOutput>, variant: Option<&str>, timeout: Option<u64>) -> Result<(Called, PathBuf)> {
        use altium_doc::outjob::NewJob;
        let st = self.stage(project)?;
        let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
        let rel_out = format!(r"Project Outputs\altium-cli-extended\{stamp}");
        let job = NewJob {
            outputs,
            out_dir: format!(r"{}\{rel_out}", st.win_root),
            pdf_name: format!("{}.pdf", project.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "outputs".into())),
            variant: variant.unwrap_or("").to_owned(),
        };
        let oj = format!(r"{}\altium-cli-extended-export-{stamp}.OutJob", st.win_root);
        self.host.write(&oj, job.to_ini_text().as_bytes())?;
        let mut req = Request::new("outjob").param("project", &st.win_file).param("outjob", &oj).param("temporary", "1");
        let mediums: Vec<String> = job.mediums().into_iter().map(str::to_owned).collect();
        let actions: Vec<String> = job.mediums().iter().map(|m| if *m == altium_doc::outjob::PDF_MEDIUM { "PublishToPDF".to_owned() } else { "Run".to_owned() }).collect();
        req = req.list("m", &mediums).list("a", &actions);
        let resp = self.call(req, timeout);
        let _ = self.host.remove(&oj);
        let mut resp = resp?;
        convert_coords(&mut resp.data);
        localize_paths(&mut resp.data, &st);
        let pulled = self.pull_staged(&st)?;
        let out_dir = st.local_root.join(rel_out.replace('\\', "/"));
        let pulled = pulled.into_iter().filter(|p| p.starts_with(&out_dir)).collect();
        Ok((Called { resp, staged: Some(st), pulled }, out_dir))
    }

    /// A PCB operation (`op` = pcb_*); writes save in Altium and the board is copied back.
    pub fn pcb_op(&self, pcb: &Path, req: Request, write: bool, timeout: Option<u64>) -> Result<Called> {
        self.call_with(req, &[("pcb", pcb)], write, timeout)
    }

    /// A schematic operation (`op` = sch_*); writes save in Altium and the sheet is copied back.
    pub fn sch_op(&self, sch: &Path, req: Request, write: bool, timeout: Option<u64>) -> Result<Called> {
        self.call_with(req, &[("sch", sch)], write, timeout)
    }

    pub fn process(&self, name: &str, params: &[(String, String)], timeout: Option<u64>) -> Result<Response> {
        if !name.contains(':') {
            return Err(Error::usage(format!("process {name:?} must look like Server:Process")).hint("e.g. PCB:Zoom, WorkspaceManager:Compile, Sch:Annotate"));
        }
        let joined = params
            .iter()
            .map(|(k, v)| {
                if k.contains(['|', '=']) || v.contains('|') {
                    Err(Error::usage(format!("process parameter {k}={v} may not contain '|'")))
                } else {
                    Ok(format!("{k}={v}"))
                }
            })
            .collect::<Result<Vec<_>>>()?
            .join("|");
        self.call(Request::new("process").param("name", name).param("params", joined), timeout)
    }

    pub fn save_all(&self) -> Result<Response> {
        let docs = self.documents()?;
        if self.write_opts.dry_run { return Ok(docs) }
        if let Some(projects) = docs.data.get("projects").and_then(Value::as_array) {
            for project in projects {
                if let Some(path) = project.get("project").and_then(Value::as_str) && DocKind::of(Path::new(path)) == DocKind::Project {
                    self.backup_remote(path)?;
                }
                if let Some(documents) = project.get("documents").and_then(Value::as_array) {
                    for doc in documents {
                        if let Some(path) = doc.get("path").and_then(Value::as_str) && self.host.exists(path)? { self.backup_remote(path)?; }
                    }
                }
            }
        }
        self.call(Request::new("save_all"), None)
    }

    pub fn close_all(&self) -> Result<Response> {
        self.call(Request::new("close_all"), None)
    }

    /// Dialogs Altium shows; with `press`, answer the modal one with the first matching button.
    pub fn dialogs(&self, press: &[String]) -> Result<crate::dialogs::Report> {
        if press.is_empty() {
            crate::dialogs::list(self.host.as_ref())
        } else {
            let p: Vec<&str> = press.iter().map(String::as_str).collect();
            crate::dialogs::close(self.host.as_ref(), &p)
        }
    }

    /// Get a stuck Altium usable again without restarting it: dismiss modal dialogs (the
    /// safest button first: Cancel, Close, No, OK) and stop a script halted in the debugger.
    pub fn reset(&self) -> Result<Value> {
        if self.host.altium_pid()?.is_none() {
            return Ok(json!({"running": false}));
        }
        let mut pressed = vec![];
        for _ in 0..6 {
            let rep = crate::dialogs::close(self.host.as_ref(), &["Cancel", "Close", "No", "OK", "*"])?;
            match &rep.pressed {
                Some(p) => pressed.push(json!({"dialog": rep.modal().first().map(|w| w.title.clone()), "pressed": p})),
                None => break,
            }
            std::thread::sleep(Duration::from_millis(800));
        }
        // Launcher boxes may still be popping up for queued requests; one more sweep.
        std::thread::sleep(Duration::from_secs(1));
        // Halts a script stopped in the debugger after a runtime error (Run » Stop).
        self.host.launch_x2(&["-REditScript:Stop".to_owned()])?;
        std::thread::sleep(Duration::from_secs(3));
        let after = crate::dialogs::list(self.host.as_ref())?;
        Ok(json!({"pressed": pressed, "script_stopped": true, "blocked": after.blocked, "dialogs": after.modal()}))
    }

    pub fn screenshot(&self, out: &Path) -> Result<PathBuf> {
        let png = self.host.screenshot()?;
        std::fs::write(out, png)?;
        Ok(crate::paths::absolute(out))
    }

    pub fn script(&self, file: &Path, proc_name: &str, timeout: Option<u64>) -> Result<Response> {
        let t = self.timeout(timeout)?;
        self.bridge().run_user_script(file, proc_name, t)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn project_root_stays_near_the_project() {
        let home = Path::new("/home/u");
        let board = Path::new("/home/u/designs/board");
        let docs = |d: &[&str]| d.iter().map(PathBuf::from).collect::<Vec<_>>();
        assert_eq!(project_root(board, &docs(&["/home/u/designs/board/A.SchDoc"]), home).unwrap(), board);
        assert_eq!(project_root(board, &docs(&["/home/u/designs/Utils/B.SchDoc"]), home).unwrap(), Path::new("/home/u/designs"));
        assert_eq!(project_root(home, &docs(&["/home/u/A.SchDoc"]), home).unwrap(), home);
        assert_eq!(project_root(Path::new("/srv/p/a/board"), &docs(&["/srv/p/shared/Logo.SchDoc"]), home).unwrap(), Path::new("/srv/p"));
        for (dir, doc) in [
            ("/home/u/designs/board", "/home/u/shared/Logo.SchDoc"),
            ("/home/u/board", "/home/u/.ssh/id_ed25519"),
            ("/srv/p/a/b/board", "/srv/p/shared/Logo.SchDoc"),
            ("/srv/p/board", "/etc/passwd"),
        ] {
            let e = project_root(Path::new(dir), &docs(&[doc]), home).unwrap_err();
            assert_eq!(e.code, ErrorCode::Unsafe, "{doc}");
            assert!(e.message.contains(doc), "{}", e.message);
        }
    }

    #[test]
    fn stage_set_lists_only_the_project_and_its_documents() {
        let tmp = std::env::temp_dir().join(format!("ace-stage-set-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("a/b/board")).unwrap();
        let tmp = crate::paths::resolve(&tmp);
        let board = tmp.join("a/b/board");
        std::fs::create_dir_all(tmp.join("a/b/Utils")).unwrap();
        std::fs::create_dir_all(tmp.join("shared")).unwrap();
        for f in ["board/A.SchDoc", "board/secret.txt", "Utils/B.SchDoc", "notes.txt"] {
            std::fs::write(tmp.join("a/b").join(f), b"x").unwrap();
        }
        std::fs::write(tmp.join("shared/Logo.SchDoc"), b"x").unwrap();
        let prj = board.join("Board.PrjPcb");
        std::fs::write(&prj, "[Design]\r\nVersion=1.0\r\n\r\n[Document1]\r\nDocumentPath=A.SchDoc\r\n\r\n[Document2]\r\nDocumentPath=..\\Utils\\B.SchDoc\r\n").unwrap();
        let set = stage_set(&prj).unwrap();
        assert_eq!(set.root, tmp.join("a/b"));
        assert_eq!(set.files, vec![prj.clone(), board.join("A.SchDoc"), tmp.join("a/b/Utils/B.SchDoc")]);
        assert_eq!(stage_set(&board.join("A.SchDoc")).unwrap(), set);
        assert_eq!(stage_set(&board.join("secret.txt")).unwrap(), StageSet { root: board.clone(), files: vec![board.join("secret.txt")] });

        let far = tmp.join("a/b/far/Far.PrjPcb");
        std::fs::create_dir_all(far.parent().unwrap()).unwrap();
        std::fs::write(&far, "[Document1]\r\nDocumentPath=..\\..\\..\\shared\\Logo.SchDoc\r\n").unwrap();
        assert_eq!(stage_set(&far).unwrap_err().code, ErrorCode::Unsafe);
        std::fs::remove_dir_all(tmp).unwrap();
    }

    #[test]
    fn backups_only_cover_design_writes() {
        for op in ["ping", "compile", "pcb_components", "pcb_select", "pcb_classes", "pcb_polygons", "sch_components", "outjob"] {
            assert!(!writes_design(&Request::new(op)), "{op}");
        }
        assert!(!writes_design(&Request::new("pcb_polygons").param("action", "list")));
        assert!(writes_design(&Request::new("pcb_polygons").param("action", "repour_all")));
        assert!(writes_design(&Request::new("pcb_classes").param("name", "signals")));
        for op in ["annotate", "pcb_edit", "pcb_rule", "sch_param", "sch_place", "sch_label", "sch_wire", "save"] {
            assert!(writes_design(&Request::new(op)), "{op}");
        }
    }
}
