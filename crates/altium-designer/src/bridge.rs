//! File-based request/response protocol with the DelphiScript bridge inside Altium.
//!
//! Layout on the Altium side (`C:\altium-cli-extended`), reached through a [`Host`]:
//!
//! ```text
//! bridge\<hash>\AltiumCliBridge.PrjScr  script project (installed from assets/)
//! bridge\<hash>\Bridge.pas              dispatcher
//! bridge\<hash>\VERSION                 protocol version
//! jobs\current.txt                id of the job the next RunScript call executes
//! jobs\<id>\request.ini           op=..., key=value lines (CRLF, single-line values)
//! jobs\<id>\log.txt               progress lines the script appends
//! jobs\<id>\response.json         {"ok":bool, "data":{...}, "error":"..."}
//! jobs\<id>\done                  written last: the response is complete
//! ```
//!
//! A request is sent by `X2.EXE -RScriptingSystem:RunScript(ProjectName=...|ProcName=...)`.
//! A running Altium receives the command; otherwise this starts Altium first. RunScript cannot
//! pass arguments, hence `current.txt`. Only one request runs at a time (a lock on this
//! machine).
//!
//! `<hash>` is a hash of the bridge sources. An updated bridge lands in a new folder, so
//! Altium never sees a script change under a project it already has open (that raises a
//! reload prompt and stalls every request).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use indexmap::IndexMap;
use serde::Serialize;
use serde_json::{Value, json};

use altium_core::{Error, ErrorCode, Result};

use crate::host::{Host, ROOT, fnv, host_key};

pub const PROTOCOL: &str = "2";
const BRIDGE_PAS: &str = include_str!("../assets/Bridge.pas");
const BRIDGE_PRJ: &str = include_str!("../assets/AltiumCliBridge.PrjScr");

#[derive(Debug, Clone, Serialize)]
pub struct Request {
    pub op: String,
    pub params: IndexMap<String, String>,
    /// Dialogs the operation is expected to raise and how to answer them (see
    /// [`Request::answer`]); pressed from the host while the script waits.
    #[serde(skip)]
    pub answers: Vec<(String, Vec<String>)>,
}

impl Request {
    pub fn new(op: &str) -> Request {
        Request { op: op.to_owned(), params: IndexMap::new(), answers: vec![] }
    }
    pub fn param(mut self, k: &str, v: impl Into<String>) -> Request {
        self.params.insert(k.to_owned(), v.into());
        self
    }
    /// When a modal dialog whose title contains `title` appears, press the first of `buttons`
    /// it has. For Altium processes that only exist as dialogs (DRC, ECO).
    pub fn answer(mut self, title: &str, buttons: &[&str]) -> Request {
        self.answers.push((title.to_owned(), buttons.iter().map(|b| (*b).to_owned()).collect()));
        self
    }

    /// Set a parameter only when there is a value.
    pub fn opt(self, k: &str, v: Option<impl Into<String>>) -> Request {
        match v {
            Some(v) => self.param(k, v),
            None => self,
        }
    }
    /// A list as `<k>_count` plus `<k>1..N` (values must be single lines).
    pub fn list(mut self, k: &str, items: &[String]) -> Request {
        self.params.insert(format!("{k}_count"), items.len().to_string());
        for (i, v) in items.iter().enumerate() {
            self.params.insert(format!("{k}{}", i + 1), v.clone());
        }
        self
    }
    /// `request.ini` bytes: UTF-8 with a BOM (so Altium's TStringList decodes non-ASCII paths),
    /// CRLF line ends, one `key=value` per line.
    pub fn to_ini(&self) -> Result<Vec<u8>> {
        let mut s = format!("\u{feff}op={}\r\nprotocol={PROTOCOL}\r\n", self.op);
        for (k, v) in &self.params {
            if v.contains(['\r', '\n']) || k.contains(['=', '\r', '\n']) || k.is_empty() {
                return Err(Error::usage(format!("parameter {k:?} must be a single line")));
            }
            s.push_str(&format!("{k}={v}\r\n"));
        }
        Ok(s.into_bytes())
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Response {
    pub job: String,
    pub data: Value,
    pub log: Vec<String>,
    pub elapsed_ms: u64,
}

fn crlf(s: &str) -> String {
    s.replace("\r\n", "\n").replace('\n', "\r\n")
}

/// Decode a response file: UTF-8 (optionally with BOM) or UTF-16LE with BOM (what Delphi
/// writes for `TEncoding.Unicode`).
pub fn decode_text(raw: &[u8]) -> String {
    if raw.starts_with(&[0xff, 0xfe]) {
        let u: Vec<u16> = raw[2..].as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
        return String::from_utf16_lossy(&u);
    }
    let raw = raw.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(raw);
    String::from_utf8_lossy(raw).into_owned()
}

/// Parse `response.json` into data or a DESIGNER_FAILED error.
pub fn parse_response(raw: &[u8], job: &str, log: &[String]) -> Result<Value> {
    let text = decode_text(raw);
    let v: Value = serde_json::from_str(text.trim()).map_err(|e| {
        Error::new(ErrorCode::DesignerFailed, format!("bridge wrote invalid JSON: {e}"))
            .details(json!({"response": text.chars().take(2000).collect::<String>(), "job": job}))
    })?;
    if v.get("ok").and_then(Value::as_bool) != Some(true) {
        let msg = v.get("error").and_then(Value::as_str).unwrap_or("unknown bridge error").to_owned();
        let mut e = Error::new(ErrorCode::DesignerFailed, msg).details(json!({"job": job, "log": log, "data": v.get("data")}));
        if let Some(h) = v.get("hint").and_then(Value::as_str) {
            e = e.hint(h.to_owned());
        }
        return Err(e);
    }
    Ok(v.get("data").cloned().unwrap_or(Value::Null))
}

pub struct Bridge<'a> {
    pub host: &'a dyn Host,
}

impl<'a> Bridge<'a> {
    pub fn new(host: &'a dyn Host) -> Self {
        Bridge { host }
    }

    pub fn jobs_dir() -> String {
        format!(r"{ROOT}\jobs")
    }

    fn bridges_root() -> String {
        format!(r"{ROOT}\bridge")
    }

    pub fn hash() -> String {
        fnv(&format!("{BRIDGE_PAS}\0{BRIDGE_PRJ}"))[..12].to_owned()
    }

    pub fn bridge_dir() -> String {
        format!(r"{}\{}", Self::bridges_root(), Self::hash())
    }

    pub fn project_path() -> String {
        format!(r"{}\AltiumCliBridge.PrjScr", Self::bridge_dir())
    }

    /// The bridge compiled into this binary is installed.
    pub fn installed(&self) -> bool {
        self.host.exists(&format!(r"{}\Bridge.pas", Self::bridge_dir())).unwrap_or(false)
    }

    /// Only an older bridge is installed.
    pub fn outdated(&self) -> bool {
        !self.installed() && self.host.list(&Self::bridges_root()).is_ok_and(|l| !l.is_empty())
    }

    fn marker(&self) -> PathBuf {
        crate::config::state_dir().join(format!("bridge-{}-{}", host_key(self.host), Self::hash()))
    }

    /// Install the bridge script project (a no-op rewrite when it is already there).
    pub fn install(&self) -> Result<Vec<String>> {
        let d = Self::bridge_dir();
        let files = vec![
            (format!(r"{d}\Bridge.pas"), crlf(BRIDGE_PAS).into_bytes()),
            (format!(r"{d}\AltiumCliBridge.PrjScr"), crlf(BRIDGE_PRJ).into_bytes()),
            (format!(r"{d}\VERSION"), PROTOCOL.as_bytes().to_vec()),
        ];
        self.host.mkdirs(&Self::jobs_dir())?;
        self.host.write_many(&files)?;
        let m = self.marker();
        if let Some(p) = m.parent() {
            let _ = std::fs::create_dir_all(p);
        }
        let _ = std::fs::write(m, b"");
        Ok(files.into_iter().map(|(p, _)| p).collect())
    }

    /// Never rewrites an installed bridge: Altium raises a reload prompt for a script that
    /// changed under an open script project, which stalls every later request.
    fn ensure_installed(&self) -> Result<()> {
        if self.marker().exists() {
            return Ok(());
        }
        if self.installed() {
            let _ = std::fs::write(self.marker(), b"");
            return Ok(());
        }
        self.install().map(|_| ())
    }

    fn new_job(&self) -> (String, String) {
        let id = format!("{}-{}", chrono::Local::now().format("%Y%m%d-%H%M%S-%3f"), std::process::id() % 100000);
        let dir = format!(r"{}\{id}", Self::jobs_dir());
        (id, dir)
    }

    pub fn run_script_arg(project_win: &str, proc_name: &str) -> String {
        format!("-RScriptingSystem:RunScript(ProjectName={project_win}|ProcName={proc_name})")
    }

    /// Send one request and wait for its response.
    pub fn call(&self, req: &Request, timeout: Duration) -> Result<Response> {
        let _lock = JobLock::acquire()?;
        self.ensure_installed()?;
        let (id, job) = self.new_job();
        let started = Instant::now();
        let prior = self.host.write_and_launch(
            &[
                (format!(r"{job}\request.ini"), req.to_ini()?),
                (format!(r"{}\current.txt", Self::jobs_dir()), format!("{id}\r\n").into_bytes()),
            ],
            &[Self::run_script_arg(&Self::project_path(), "Bridge.pas>Run")],
        )?;
        let mut r = self.wait_done(&id, &job, started, timeout, prior.map(|_| timeout.min(Duration::from_secs(150))), &req.answers);
        if let Err(e) = &r
            && e.code == ErrorCode::Timeout
            && prior.is_none()
            && self.host.altium_pid().ok().flatten().is_some()
        {
            // First start: Altium loaded but dropped the -R command it was started with (it
            // was busy signing in / loading extensions). Ask the running instance once more.
            let again = Instant::now();
            self.host.launch_x2(&[Self::run_script_arg(&Self::project_path(), "Bridge.pas>Run")])?;
            r = self.wait_done(&id, &job, again, Duration::from_secs(150), Some(Duration::from_secs(90)), &req.answers);
        }
        r
    }

    /// Run a user DelphiScript: copied into a per-job script project with a wrapper that calls
    /// `proc_name` and then signals completion. The script can call `AltiumCliJobDir` (defined
    /// by the wrapper) and write `AltiumCliJobDir + 'user.json'`; that JSON becomes the
    /// response data.
    pub fn run_user_script(&self, script: &Path, proc_name: &str, timeout: Duration) -> Result<Response> {
        let src = std::fs::read(script)?;
        if proc_name.is_empty() || !proc_name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return Err(Error::usage(format!("invalid procedure name {proc_name:?}")));
        }
        let _lock = JobLock::acquire()?;
        let (id, job) = self.new_job();
        let wrap = WRAP_PAS.replace("{{JOB}}", &format!(r"{job}\")).replace("{{PROC}}", proc_name);
        let started = Instant::now();
        let prior = self.host.write_and_launch(
            &[
                (format!(r"{job}\User.pas"), src),
                (format!(r"{job}\Wrap.pas"), crlf(&wrap).into_bytes()),
                (format!(r"{job}\Job.PrjScr"), crlf(USER_PRJ).into_bytes()),
            ],
            &[Self::run_script_arg(&format!(r"{job}\Job.PrjScr"), "Wrap.pas>Run")],
        )?;
        let mut resp = self.wait_done(&id, &job, started, timeout, prior.map(|_| timeout.min(Duration::from_secs(150))), &[])?;
        if let Some(b) = self.host.read(&format!(r"{job}\user.json"))? {
            let t = decode_text(&b);
            resp.data = serde_json::from_str(t.trim()).unwrap_or(Value::String(t));
        }
        Ok(resp)
    }

    /// Best effort: save what the Altium machine shows right now, for an error report.
    pub fn snapshot(&self) -> Option<String> {
        let png = self.host.screenshot().ok()?;
        let dir = crate::config::state_dir();
        std::fs::create_dir_all(&dir).ok()?;
        let p = dir.join(format!("designer-{}.png", chrono::Local::now().format("%Y%m%d-%H%M%S")));
        std::fs::write(&p, png).ok()?;
        Some(p.display().to_string())
    }

    /// The modal dialog Altium is showing, as an error to return (with a screenshot).
    fn blocked_error(&self, job: &str, log: &[String]) -> Option<Error> {
        let rep = crate::dialogs::list(self.host).ok()?;
        if !rep.blocked {
            return None;
        }
        Some(
            Error::new(ErrorCode::DesignerFailed, format!("Altium is waiting on a dialog: {}", rep.summary()))
                .hint("read error.details.dialogs (and the screenshot); answer it with `ace designer dialogs --press <button>` or `ace designer reset`, then retry")
                .details(json!({"job": job, "log": log, "dialogs": rep.modal(), "screenshot": self.snapshot()})),
        )
    }

    /// Polls for `done`. Fails early when Altium is stuck on a modal dialog: checked whenever
    /// the job log has not moved for a while (`grace` covers a script that never started in an
    /// Altium that was already running).
    fn wait_done(&self, id: &str, job: &str, started: Instant, timeout: Duration, grace: Option<Duration>, answers: &[(String, Vec<String>)]) -> Result<Response> {
        let poll = if self.host.kind() == "ssh" { Duration::from_millis(500) } else { Duration::from_millis(200) };
        let files = [format!(r"{job}\done"), format!(r"{job}\response.json"), format!(r"{job}\log.txt")];
        let mut log_seen = false;
        // The job's own files on the host: proof the script ran even if a read came back empty.
        let mut job_files_seen = false;
        let (mut last_len, mut last_change, mut last_check) = (0usize, Instant::now(), Instant::now());
        let mut can_check = self.host.kind() != "wine";
        let (quiet_for, every) = if answers.is_empty() { (Duration::from_secs(20), Duration::from_secs(25)) } else { (Duration::from_secs(2), Duration::from_secs(4)) };
        let mut answered: Vec<String> = vec![];
        loop {
            let got = self.host.read_many(&files)?;
            let log: Vec<String> = got[2].as_deref().map(|b| decode_text(b).lines().map(str::to_owned).collect()).unwrap_or_default();
            log_seen |= got[2].is_some();
            if got[0].is_some() {
                let raw = got[1].clone().ok_or_else(|| Error::new(ErrorCode::DesignerFailed, "bridge finished without a response"))?;
                let mut data = parse_response(&raw, job, &log)?;
                if !answered.is_empty()
                    && let Value::Object(o) = &mut data
                {
                    o.insert("_answered".into(), json!(answered));
                }
                return Ok(Response { job: id.to_owned(), data, log, elapsed_ms: started.elapsed().as_millis() as u64 });
            }
            if log.len() != last_len {
                last_len = log.len();
                last_change = Instant::now();
            }
            let quiet = last_change.elapsed();
            let stalled = if log_seen { quiet > quiet_for } else { grace.is_some_and(|g| started.elapsed() > g.min(Duration::from_secs(30))) };
            if can_check && stalled && last_check.elapsed() > every {
                last_check = Instant::now();
                match crate::dialogs::list(self.host) {
                    Ok(rep) if rep.blocked => {
                        let modal = rep.modal();
                        let expected = answers.iter().find(|(t, _)| modal.iter().any(|w| w.title.contains(t.as_str()) && !w.launcher));
                        if let Some((title, buttons)) = expected {
                            let b: Vec<&str> = buttons.iter().map(String::as_str).collect();
                            if let Ok(r) = crate::dialogs::close(self.host, &b)
                                && let Some(p) = r.pressed
                            {
                                answered.push(format!("{title}: {p}"));
                                last_change = Instant::now();
                                last_check = Instant::now() - every;
                                continue;
                            }
                        }
                        if let Some(e) = self.blocked_error(job, &log) {
                            return Err(e);
                        }
                    }
                    Ok(_) => {}
                    Err(_) => can_check = false,
                }
            }
            if !log_seen && grace.is_some_and(|g| started.elapsed() > g) {
                // A busy host can answer one poll with nothing while the job itself ran fine: ask
                // the host directly before blaming Altium for not starting the script.
                if !job_files_seen {
                    let probe = |path: &str| self.host.exists(path).map_err(|mut e| {
                        e.message = format!("cannot poll bridge job {id} at {path}: {}", e.message);
                        let hint = "check host connectivity and permissions on the bridge job directory; the job may still be running, so inspect `ace designer status` and `ace designer screenshot` before retrying";
                        e.hint = Some(match e.hint { Some(prior) => format!("{prior}; {hint}"), None => hint.to_owned() });
                        e
                    });
                    job_files_seen = probe(&files[2])? || probe(&files[0])?;
                }
                if !job_files_seen {
                    return Err(Error::new(ErrorCode::Timeout, "the bridge script did not start in the running Altium")
                        .hint("Altium may be busy with another script, unlicensed, or showing a dialog; see error.details.screenshot, run `ace designer reset`, then retry")
                        .details(json!({"job": job, "screenshot": self.snapshot()})));
                }
            }
            if started.elapsed() > timeout {
                return Err(Error::new(ErrorCode::Timeout, format!("Altium did not answer within {}s", timeout.as_secs()))
                    .hint(if log.is_empty() {
                        "the script never started: Altium may still be loading, unlicensed, or showing a dialog; see error.details.screenshot, run `ace designer reset`, retry, or raise --timeout"
                    } else {
                        "the script started but did not finish: Altium is still working or waiting on something; see error.details.screenshot and error.details.log, or raise --timeout"
                    })
                    .details(json!({"job": job, "log": log, "screenshot": self.snapshot()})));
            }
            std::thread::sleep(poll);
        }
    }
}

/// One designer request at a time per machine: an exclusive OS lock on a file in the state
/// directory (released by the OS when the process ends, so a crash never leaves it stale).
struct JobLock {
    _file: std::fs::File,
}

impl JobLock {
    fn acquire() -> Result<JobLock> {
        let dir = crate::config::state_dir();
        Self::acquire_in(&dir)
    }

    fn acquire_in(dir: &Path) -> Result<JobLock> {
        std::fs::create_dir_all(dir)?;
        let path = dir.join("designer.lock");
        let file = std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(&path)?;
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            match file.try_lock() {
                Ok(()) => return Ok(JobLock { _file: file }),
                Err(std::fs::TryLockError::WouldBlock) => {
                    if Instant::now() > deadline {
                        return Err(Error::new(ErrorCode::Timeout, "another altium-cli-extended process is talking to Altium")
                            .hint("designer commands run one at a time; wait and retry"));
                    }
                    std::thread::sleep(Duration::from_millis(200));
                }
                Err(std::fs::TryLockError::Error(e)) => return Err(e.into()),
            }
        }
    }
}

const WRAP_PAS: &str = "{ altium-cli-extended wrapper: runs the user procedure, then signals completion. }
Function AltiumCliJobDir : String;
Begin
  Result := '{{JOB}}';
End;

Procedure Signal(Ok : Boolean);
Var F : TStringList;
Begin
  F := TStringList.Create;
  If Ok Then F.Text := '{\"ok\":true,\"data\":null}'
  Else F.Text := '{\"ok\":false,\"error\":\"user script raised an error\"}';
  F.SaveToFile('{{JOB}}response.json');
  F.Clear;
  F.SaveToFile('{{JOB}}done');
  F.Free;
End;

Procedure Run;
Var L : TStringList;
Begin
  L := TStringList.Create;
  L.Add('started');
  L.SaveToFile('{{JOB}}log.txt');
  L.Free;
  Try
    {{PROC}};
    Signal(True);
  Except
    Signal(False);
  End;
End;
";

const USER_PRJ: &str = "[Design]
Version=1.0

[Document1]
DocumentPath=User.pas

[Document2]
DocumentPath=Wrap.pas
";

#[cfg(test)]
mod tests {
    use super::*;

    /// Line of the first `{` inside a `{ }` comment. Pascal brace comments do not nest, so the
    /// first `}` ends the comment and the rest of it is parsed as code.
    fn nested_brace_comment(src: &str) -> Option<usize> {
        let b = src.as_bytes();
        let (mut line, mut i) = (1, 0);
        let (mut in_str, mut in_brace, mut in_paren, mut in_line) = (false, false, false, false);
        while i < b.len() {
            let c = b[i];
            if c == b'\n' {
                line += 1;
                in_line = false;
                in_str = false;
            } else if in_line {
            } else if in_brace {
                match c {
                    b'{' => return Some(line),
                    b'}' => in_brace = false,
                    _ => {}
                }
            } else if in_paren {
                if c == b'*' && b.get(i + 1) == Some(&b')') {
                    in_paren = false;
                    i += 1;
                }
            } else if in_str {
                in_str = c != b'\'';
            } else {
                match c {
                    b'\'' => in_str = true,
                    b'{' => in_brace = true,
                    b'(' if b.get(i + 1) == Some(&b'*') => {
                        in_paren = true;
                        i += 1;
                    }
                    b'/' if b.get(i + 1) == Some(&b'/') => in_line = true,
                    _ => {}
                }
            }
            i += 1;
        }
        None
    }

    #[test]
    fn pascal_comments_do_not_nest() {
        assert_eq!(nested_brace_comment("{ a {\"ok\":false} b }"), Some(1));
        assert_eq!(nested_brace_comment("S := '{\"ok\":true}'; { fine }"), None);
        let wrap = WRAP_PAS.replace("{{JOB}}", r"C:\j\").replace("{{PROC}}", "Main");
        for (name, src) in [("Bridge.pas", BRIDGE_PAS), ("Wrap.pas", wrap.as_str())] {
            assert_eq!(nested_brace_comment(src), None, "{name}: a brace inside a brace comment ends it early");
        }
    }

    struct PollHost {
        probes: std::cell::RefCell<std::collections::VecDeque<Result<bool>>>,
    }

    impl Host for PollHost {
        fn kind(&self) -> &'static str { "mock" }
        fn describe(&self) -> Value { json!({}) }
        fn altium_dir(&self) -> &str { "" }
        fn read(&self, _: &str) -> Result<Option<Vec<u8>>> { Ok(None) }
        fn exists(&self, _: &str) -> Result<bool> { self.probes.borrow_mut().pop_front().expect("unexpected probe") }
        fn write(&self, _: &str, _: &[u8]) -> Result<()> { unreachable!() }
        fn mkdirs(&self, _: &str) -> Result<()> { unreachable!() }
        fn remove(&self, _: &str) -> Result<()> { unreachable!() }
        fn list(&self, _: &str) -> Result<Vec<String>> { unreachable!() }
        fn launch_x2(&self, _: &[String]) -> Result<Option<u32>> { unreachable!() }
        fn altium_pid(&self) -> Result<Option<u32>> { unreachable!() }
        fn kill_altium(&self) -> Result<usize> { unreachable!() }
        fn clock(&self) -> Result<String> { unreachable!() }
        fn stage_in(&self, _: &Path, _: &Path) -> Result<crate::host::Staged> { unreachable!() }
        fn stage_out(&self, _: &crate::host::Staged, _: &str, _: altium_format::fsx::WriteOpts) -> Result<(Vec<PathBuf>, String)> { unreachable!() }
    }

    #[test]
    fn poll_probe_errors_surface_with_recovery_hint() {
        for prefix in [vec![], vec![Ok(false)]] {
            let mut probes = prefix;
            probes.push(Err(Error::new(ErrorCode::Io, "permission denied").details(json!({"source": "mock"}))));
            let host = PollHost { probes: std::cell::RefCell::new(probes.into()) };
            let e = Bridge::new(&host).wait_done("id", "job", Instant::now() - Duration::from_millis(1), Duration::from_secs(60), Some(Duration::ZERO), &[]).unwrap_err();
            assert_eq!(e.code, ErrorCode::Io);
            assert!(e.message.contains("permission denied"));
            assert!(e.hint.as_deref().is_some_and(|h| h.contains("permissions") && h.contains("job may still be running")), "{e}");
            assert_eq!(e.details.as_deref().unwrap()["source"], "mock");
            assert!(host.probes.borrow().is_empty());
        }
    }

    #[test]
    fn absent_job_files_still_report_not_started() {
        let host = PollHost { probes: std::cell::RefCell::new([Ok(false), Ok(false)].into()) };
        let e = Bridge::new(&host).wait_done("id", "job", Instant::now() - Duration::from_millis(1), Duration::from_secs(60), Some(Duration::ZERO), &[]).unwrap_err();
        assert_eq!(e.code, ErrorCode::Timeout);
        assert!(e.message.contains("did not start"));
    }

    #[test]
    fn request_ini() {
        let r = Request::new("pcb_move").param("pcb", r"C:\w\Ä.PcbDoc").list("d", &["U1".into(), "R2".into()]);
        let b = r.to_ini().unwrap();
        assert!(b.starts_with(&[0xef, 0xbb, 0xbf]));
        let t = String::from_utf8(b[3..].to_vec()).unwrap();
        assert_eq!(t, "op=pcb_move\r\nprotocol=2\r\npcb=C:\\w\\Ä.PcbDoc\r\nd_count=2\r\nd1=U1\r\nd2=R2\r\n");
        assert!(Request::new("x").param("a", "b\nc").to_ini().is_err());
        assert!(Request::new("x").param("a=b", "c").to_ini().is_err());
    }

    #[test]
    fn responses() {
        assert_eq!(parse_response(b"\xef\xbb\xbf{\"ok\":true,\"data\":{\"a\":\"\\u00c4\"}}\r\n", "j", &[]).unwrap(), json!({"a": "Ä"}));
        let utf16: Vec<u8> = [0xff, 0xfe].into_iter().chain("{\"ok\":true,\"data\":1}".encode_utf16().flat_map(|u| u.to_le_bytes())).collect();
        assert_eq!(parse_response(&utf16, "j", &[]).unwrap(), json!(1));
        let e = parse_response(b"{\"ok\":false,\"error\":\"boom\",\"hint\":\"do x\",\"data\":{}}", "j", &[]).unwrap_err();
        assert_eq!(e.code, ErrorCode::DesignerFailed);
        assert_eq!(e.message, "boom");
        assert!(parse_response(b"{not json", "j", &[]).is_err());
    }

    #[test]
    fn job_lock_is_exclusive() {
        let dir = std::env::temp_dir().join(format!("ace-job-lock-test-{}", std::process::id()));
        let a = JobLock::acquire_in(&dir).unwrap();
        let f = std::fs::OpenOptions::new().write(true).open(dir.join("designer.lock")).unwrap();
        assert!(matches!(f.try_lock(), Err(std::fs::TryLockError::WouldBlock)));
        drop(a);
        assert!(f.try_lock().is_ok());
        drop(f);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn bridge_dir_tracks_sources() {
        let d = Bridge::bridge_dir();
        assert!(d.starts_with(r"C:\altium-cli-extended\bridge\") && d.len() == r"C:\altium-cli-extended\bridge\".len() + 12, "{d}");
    }
}
