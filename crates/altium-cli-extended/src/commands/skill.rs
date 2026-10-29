//! The agent skill: `SKILL.md` (when and how to use the tool) and `COMMANDS.md` (the full
//! reference), both generated from the live command table, plus install/status/uninstall
//! into the skill directories of common coding agents. Installation copies files, so it works
//! the same on Windows, macOS and Linux.

use std::path::{Path, PathBuf};

use clap::{Subcommand, ValueEnum};
use serde_json::{Value, json};

use altium_core::error::{Error, ErrorCode, Result};

use crate::commands::meta::{describe_json, error_meaning, find_cmd, global_flags, leaf_paths};
use crate::ctx::Ctx;
use crate::output::{Out, list_with};

/// Name of the skill directory.
pub const SKILL_NAME: &str = "altium-cli-extended";
/// Files that make up the skill.
pub const FILES: &[&str] = &["SKILL.md", "COMMANDS.md"];

#[derive(Subcommand, Debug)]
pub enum SkillCmd {
    /// Read SKILL.md (or COMMANDS.md with --file commands); --raw/--text prints Markdown.
    Show {
        #[arg(long, value_enum, default_value = "skill")]
        file: SkillFile,
    },
    /// Copy SKILL.md and COMMANDS.md into agent skill directories.
    Install {
        /// Agents to install for (repeatable). Default: `agents` plus every agent whose home
        /// directory (~/.claude, ~/.codex, ~/.gemini) exists.
        #[arg(long = "target", value_enum)]
        targets: Vec<Target>,
        /// Install into DIR/altium-cli-extended instead (a project's `.claude/skills`, say).
        #[arg(long, conflicts_with = "targets")]
        dir: Option<PathBuf>,
    },
    /// Where the skill is installed and whether each copy matches this binary.
    Status {
        /// Also check DIR/altium-cli-extended.
        #[arg(long)]
        dir: Option<PathBuf>,
    },
    /// Remove installed copies (only the files this tool writes).
    Uninstall {
        #[arg(long = "target", value_enum)]
        targets: Vec<Target>,
        #[arg(long, conflicts_with = "targets")]
        dir: Option<PathBuf>,
    },
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// ~/.agents/skills (shared location read by several agents)
    Agents,
    /// ~/.claude/skills (Claude Code)
    Claude,
    /// ~/.codex/skills (Codex CLI)
    Codex,
    /// ~/.gemini/skills (Gemini CLI)
    Gemini,
    /// Every target above.
    All,
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillFile {
    /// SKILL.md: when to use the tool, rules, workflows.
    Skill,
    /// COMMANDS.md: every command with arguments, return shape and example.
    Commands,
}

const ALL: &[Target] = &[Target::Agents, Target::Claude, Target::Codex, Target::Gemini];

impl Target {
    fn name(self) -> &'static str {
        match self {
            Target::Agents => "agents",
            Target::Claude => "claude",
            Target::Codex => "codex",
            Target::Gemini => "gemini",
            Target::All => "all",
        }
    }
    /// Agent home directory under the user's home (`.claude`...).
    fn home_dir(self) -> &'static str {
        match self {
            Target::Agents | Target::All => ".agents",
            Target::Claude => ".claude",
            Target::Codex => ".codex",
            Target::Gemini => ".gemini",
        }
    }
}

/// The user's home directory: `HOME` (`USERPROFILE` first on Windows, where shells such as
/// Git Bash set `HOME` to a POSIX path), else HOMEDRIVE+HOMEPATH.
pub fn home() -> Result<PathBuf> {
    let order = if cfg!(windows) { ["USERPROFILE", "HOME"] } else { ["HOME", "USERPROFILE"] };
    for k in order {
        if let Some(v) = std::env::var_os(k).filter(|v| !v.is_empty()) {
            return Ok(PathBuf::from(v));
        }
    }
    if let (Some(d), Some(p)) = (std::env::var_os("HOMEDRIVE"), std::env::var_os("HOMEPATH")) {
        let mut s = d;
        s.push(p);
        return Ok(PathBuf::from(s));
    }
    Err(Error::new(ErrorCode::Io, "cannot find the home directory (HOME / USERPROFILE unset)").hint("pass --dir <skills directory>"))
}

fn expand(targets: &[Target]) -> Vec<Target> {
    let mut v: Vec<Target> = vec![];
    for t in targets {
        let add: &[Target] = if *t == Target::All { ALL } else { std::slice::from_ref(t) };
        for a in add {
            if !v.contains(a) {
                v.push(*a);
            }
        }
    }
    v
}

/// `(target name, skill directory)` for the requested targets or `--dir`.
fn destinations(targets: &[Target], dir: Option<&Path>, default_detect: bool) -> Result<Vec<(String, PathBuf)>> {
    if let Some(d) = dir {
        return Ok(vec![("dir".into(), d.join(SKILL_NAME))]);
    }
    let h = home()?;
    let chosen = if targets.is_empty() {
        if default_detect {
            ALL.iter().copied().filter(|t| *t == Target::Agents || h.join(t.home_dir()).is_dir()).collect()
        } else {
            ALL.to_vec()
        }
    } else {
        expand(targets)
    };
    Ok(chosen.into_iter().map(|t| (t.name().to_owned(), h.join(t.home_dir()).join("skills").join(SKILL_NAME))).collect())
}

/// Every skill file with its generated content.
pub fn files(root: &clap::Command) -> Vec<(&'static str, String)> {
    vec![("SKILL.md", skill_md(root)), ("COMMANDS.md", commands_md(root))]
}

pub fn run_print(file: SkillFile, root: &clap::Command) -> Result<Out> {
    let (name, md) = match file {
        SkillFile::Skill => ("SKILL.md", skill_md(root)),
        SkillFile::Commands => ("COMMANDS.md", commands_md(root)),
    };
    Out::new(json!({"name": SKILL_NAME, "file": name, "markdown": md, "files": FILES}))
}

pub fn run(c: SkillCmd, root: &clap::Command, ctx: &Ctx) -> Result<Out> {
    let generated = files(root);
    match c {
        SkillCmd::Show { file } => run_print(file, root),
        SkillCmd::Install { targets, dir } => {
            let mut items = vec![];
            for (target, d) in destinations(&targets, dir.as_deref(), true)? {
                // A symlink left by older versions is replaced by a real directory.
                let was_link = d.symlink_metadata().is_ok_and(|m| m.file_type().is_symlink());
                let mut written = vec![];
                let mut unchanged = vec![];
                for (name, content) in &generated {
                    let p = d.join(name);
                    if !was_link && std::fs::read(&p).ok().as_deref() == Some(content.as_bytes()) {
                        unchanged.push(*name);
                        continue;
                    }
                    written.push(*name);
                }
                if !ctx.dry_run && !written.is_empty() {
                    if was_link {
                        remove_link(&d)?;
                    }
                    std::fs::create_dir_all(&d)?;
                    for (name, content) in &generated {
                        if written.contains(name) {
                            std::fs::write(d.join(name), content)?;
                        }
                    }
                }
                items.push(json!({"target": target, "path": d, "written": written, "unchanged": unchanged, "replaced_symlink": was_link}));
            }
            Out::new(list_with(&items, json!({"skill": SKILL_NAME, "version": env!("CARGO_PKG_VERSION")}))?)
        }
        SkillCmd::Status { dir } => {
            let mut dests = destinations(&[], None, false)?;
            if let Some(d) = dir {
                dests.push(("dir".into(), d.join(SKILL_NAME)));
            }
            let items: Vec<Value> = dests
                .into_iter()
                .map(|(target, d)| {
                    let present: Vec<&str> = FILES.iter().copied().filter(|f| d.join(f).is_file()).collect();
                    let current = generated.iter().all(|(n, c)| std::fs::read(d.join(n)).ok().as_deref() == Some(c.as_bytes()));
                    json!({
                        "target": target,
                        "path": d,
                        "installed": !present.is_empty(),
                        "files": present,
                        "up_to_date": current,
                        "symlink": d.symlink_metadata().is_ok_and(|m| m.file_type().is_symlink()),
                    })
                })
                .collect();
            let stale = items.iter().any(|i| i["installed"] == true && i["up_to_date"] == false);
            let mut out = Out::new(list_with(&items, json!({"skill": SKILL_NAME, "version": env!("CARGO_PKG_VERSION")}))?)?;
            if stale {
                out = out.warn("an installed copy differs from this binary's skill: run `ace skill install` to refresh it");
            }
            Ok(out)
        }
        SkillCmd::Uninstall { targets, dir } => {
            let mut items = vec![];
            for (target, d) in destinations(&targets, dir.as_deref(), false)? {
                let is_link = d.symlink_metadata().is_ok_and(|m| m.file_type().is_symlink());
                let mut removed = vec![];
                if is_link {
                    if !ctx.dry_run {
                        remove_link(&d)?;
                    }
                    removed.push("(symlink)".to_owned());
                } else {
                    for f in FILES {
                        let p = d.join(f);
                        if p.is_file() {
                            if !ctx.dry_run {
                                std::fs::remove_file(&p)?;
                            }
                            removed.push(f.to_string());
                        }
                    }
                    // Remove the directory only when nothing else is left in it.
                    if !ctx.dry_run && d.is_dir() && std::fs::read_dir(&d).is_ok_and(|mut r| r.next().is_none()) {
                        std::fs::remove_dir(&d)?;
                    }
                }
                if !removed.is_empty() {
                    items.push(json!({"target": target, "path": d, "removed": removed}));
                }
            }
            Out::new(list_with(&items, json!({"skill": SKILL_NAME}))?)
        }
    }
}

fn remove_link(p: &Path) -> Result<()> {
    // Windows directory symlinks/junctions are removed with remove_dir, files with remove_file.
    std::fs::remove_file(p).or_else(|_| std::fs::remove_dir(p)).map_err(Error::from)
}

// ------------------------------------------------------------------------------ content

const DESCRIPTION: &str = "Read, query, check and edit Altium Designer projects (.PrjPcb, .SchDoc, .SchLib, .PcbDoc, \
.PcbLib, .OutJob) with the `ace` CLI, natively on Windows, macOS and Linux, and drive a real Altium Designer \
(local Windows, a Windows machine over SSH, or Wine) for compile, ERC, DRC and fabrication outputs. Use for any task \
on Altium files, including when the user has no Altium, such as \"a contractor sent me an Altium project\", \"what changed between \
rev A and rev B\", \"is this releasable\", \"make an ECO\" or \"change R20\", \"do not populate\", \"rename a footprint\", \
\"which footprints have 0603\", \"what is on this net or pin\", \"why is the build red\", \"Altium hangs\", \"migrate to KiCad\". \
Covers handover review, release CI, BOMs and variants, netlists and connectivity, schematic vs PCB consistency, \
design rules, parameters, footprints and symbols, revision diffs, Gerber/NC drill/ODB++/PDF/STEP outputs.";

/// One line about a command group.
fn group_about(root: &clap::Command, g: &str) -> String {
    find_cmd(root, g).and_then(|c| c.get_about()).map(|a| a.to_string()).unwrap_or_default()
}

pub fn skill_md(root: &clap::Command) -> String {
    let mut s = String::new();
    s.push_str(&format!("---\nname: {SKILL_NAME}\ndescription: {DESCRIPTION}\n---\n\n"));
    s.push_str(SKILL_BODY);
    // Generated overview of command groups.
    s.push_str("\n## Command groups\n\n| group | commands | what |\n|---|---|---|\n");
    let leaves = leaf_paths(root);
    let mut groups: Vec<String> = vec![];
    for p in &leaves {
        let g = p.split_whitespace().next().unwrap_or("").to_owned();
        if !groups.contains(&g) {
            groups.push(g);
        }
    }
    let meta_cmds: Vec<&str> = vec!["commands", "describe", "skill", "errors", "doctor", "inspect"];
    for g in groups.iter().filter(|g| !meta_cmds.contains(&g.as_str())) {
        let n = leaves.iter().filter(|p| p == &g || p.starts_with(&format!("{g} "))).count();
        s.push_str(&format!("| `{g}` | {n} | {} |\n", group_about(root, g).replace('|', "/")));
    }
    s.push_str(&format!(
        "| meta | {} | `commands`, `describe`, `skill`, `errors`, `doctor`, `inspect`: discovery and setup |\n",
        meta_cmds.iter().map(|m| leaves.iter().filter(|p| p == m || p.starts_with(&format!("{m} "))).count()).sum::<usize>()
    ));
    s.push_str("\nEvery command with arguments, return shape and an example: [COMMANDS.md](COMMANDS.md) \
(or `ace commands`, `ace describe <command>`).\n");
    s
}

const SKILL_BODY: &str = r#"# altium-cli-extended

Use `ace` for contractor handover, board review, revision comparison, ECO edits,
Windows or SSH release checks, library audits, and Altium to KiCad migration intake.
Native commands run locally without Altium. Designer commands need a licensed Altium host.

## Start and interpret results

```bash
ace doctor --quick
ace commands --brief
ace describe check
ace skill show --raw
```

- Resolve the project and board to absolute paths. A directory with several projects returns
  `AMBIGUOUS` and `error.details.candidates`. Use `project find` to choose a project.
- For review, set `ACE_READONLY=1` or use `--read-only`. It refuses write and control commands
  before file or host access. A read-only Designer dry-run is a local preview, not a host test.
  Keep reports outside the source tree.
- Parse stdout as one JSON envelope with `schema: altium-cli-extended/v1`, `command`, and `ok`.
  Success has `data`, `warnings`, and `meta`. Failure has `error.code`, `error.category`,
  `message`, and optional `hint` and `details`. Warnings and meta appear only on success.
  Help and `skill --raw`/`skill --text` print text.
- Branch on `error.category`, never on the number. ace and kce use different numeric exits.
  `ok:true` means execution succeeded. For design approval, read `passed`, `identical`,
  `incomplete`, and `gate`. `check --fail-on error|warning` and `diff --exit-code` preserve
  the report and exit 9 when the gate matches. Default report-only commands exit 0.
- Read warnings before using counts. Check `documents_declared`, `documents_found`, and
  `missing_documents`. Missing design sheets disable sync checks and produce `incomplete-project`.
- If a codepage warning suggests `gbk` or `windows-1251`, set `ACE_CODEPAGE` for every later
  call. Use `--codepage gbk` for a GBK suggestion. `meta.suggest.codepage` carries the suggestion. Unmarked text may still need a known
  codepage from the author. Automatic detection is not supported.
- Use `--where name=GND`, `--where name^=clk`, `--where designator~^R`, `--fields`, and
  `--limit` instead of piping to `head`. `matched` counts filtered rows before the limit.
  `--limit 0` returns the counts and summary without rows. Dotted fields work on lists and
  single objects. Quote unannotated designators such as `'U?'`.
- A `NOT_FOUND` error is an answer, not a crash: its `hint` lists valid names. A misspelled command
  or flag also gets a hint; follow it before guessing.
- Native edits keep the first `<file>.bak` unless `--no-backup` is set. Preview with `--dry-run`,
  inspect `changes`, then apply. Close the document in Altium first. Binary edits preserve
  untouched records and streams. Project and OutJob INI writes keep entry order, line endings,
  and the BOM, but normalize spacing and omit comments and unrecognized lines.
  Keep a separate project copy for a multi-file ECO. There is no project checkpoint or transaction.
- A comment such as `=Value` is a link. Set its Value parameter. Replacing the link needs
  `--literal`; dry runs mark `special_string`. Give rule lengths units, for example `0.2mm`.
- Native connectivity is an approximation. Altium compile, ERC, DRC, and fabrication output
  verification require Designer. Never claim those checks passed from native results.
- Give every Designer call `--timeout`. A timeout, or a result that never arrived, is
  "not verified", never "clean". Exits 10, 11 and 12 are infrastructure, not design findings.

## Choose a recipe from the user's goal

Each recipe is Trigger (what the user says), Chain (commands to run without being asked),
Verify (what to read before answering). Replace the quoted path variables with absolute paths.
`$P` is the project, `$B` its board, `$S` a sheet from `sch locate`, and `$R` an existing
report directory outside the source. Pick by intent, then run the whole chain.

### Recipe 1 - Review a contractor's project without Altium

Trigger: "a contractor sent me an Altium project", "I have no Altium", "what is in this .PrjPcb",
"BOM from Altium without Altium", "check the handover", "review this board".

```bash
export ACE_READONLY=1
ace project find /absolute/handover --depth 6
ace inspect "$P"
ace project documents "$P" --fields rel,kind,exists
ace sch hierarchy "$P"
ace pcb info "$B"
ace bom "$P" --group-by comment,footprint,mpn --columns designators,quantity,comment,footprint,manufacturer,mpn --out "$R/bom.csv"
ace bom "$P" --source pcb --limit 1
ace netlist "$P" --out "$R/source.net"
ace check "$P" --severity warning --require MPN
ace pcb svg "$B" --out "$R/board.svg"
```

Verify: `documents_found` equals `documents_declared` and `missing_documents` is empty, else the
path was relative or sheets are missing: stop. Reconcile `total_parts` of the schematic and
`--source pcb` BOMs. Report findings by `code` (`ace check --list` explains each), not by `passed`.
Adopt a codepage suggestion before exporting. Finish with "not verified: compile, ERC, DRC,
fabrication outputs; nets are ace's native approximation".

### Recipe 2 - Compare revisions and find an annotation change

Trigger: "what changed between Rev A and Rev B", "compare these two versions", "changelog against
the last tag", "did they fix my comments", "review a change".

```bash
ace project documents "$PA" --fields rel,kind,exists
ace diff /absolute/RevA/Board.PrjPcb /absolute/RevB/Board.PrjPcb --summary
ace diff /absolute/RevA/Board.PrjPcb /absolute/RevB/Board.PrjPcb --by part --ignore-field '^param:ComponentLink'
ace diff /absolute/RevA/Board.PrjPcb /absolute/RevB/Board.PrjPcb --where change=part_added --fields ref
ace diff /absolute/RevA/Board.PcbDoc /absolute/RevB/Board.PcbDoc --geometry
ace diff "$P" --rev HEAD~1 --source pcb
ace check "$PB" --severity warning
```

The default match uses unique IDs before designators. `part_renamed` preserves simultaneous
parameter changes. Project diffs include variants and settings; compare the PCB separately.
`--rev` uses the nearest enclosing git repository and needs committed dependencies.

Verify: both sides have the same sheet set (read `coverage`). `identical:true` on schematics says
nothing about the board, so run the PCB diff too. Summarize from `summary` and `by_field`, then
open only the rows that matter with `--where`. A revision can fix one finding and add another,
so run `check` on the new side.

### Recipe 3 - Gate a design in CI

Trigger: "is this releasable", "pre-release check", "preflight", "fail the build",
"is the schematic in sync with the PCB", "annotation", "missing footprints", "block a bad release".

```bash
ace check "$P" --list
ace check "$P" --pcb "$B" --require MPN,Manufacturer --fail-on error
ace diff "$P" --rev HEAD~1 --exit-code
```

Archive the envelope even on exit 9. To gate only new findings, save a complete unshaped
`check` envelope, then pass `--baseline` with the same checks and PCB selection.

Verify: `schematic` lists every sheet and `pcb` is the intended board. On a multi-board project
gate on a baseline instead of demanding zero. Triage by code: `unannotated` -> `ace sch find "$S" '?'`;
`missing-in-pcb` -> `ace pcb component "$B" REF` and expect `NOT_FOUND`; `footprint-mismatch` ->
compare `ace sch component "$S" REF --project "$P"` with `ace pcb component "$B" REF`;
`net-mismatch`, `nets-merged-in-pcb`, `pin-unrouted-in-pcb` -> `ace pcb net "$B" NET` and
`ace netlist "$P" --pins --where designator=REF`.

### Recipe 4 - Run release checks with Altium

Trigger: "release board", "fab package", "gerbers for", "send to fab", "tag a release",
"run the OutJob", "gate on DRC/ERC", "headless Altium", "Windows build machine", "Linux CI over SSH".

```bash
ace designer status
ace designer ping --timeout 60
ace outjob outputs /absolute/Release.OutJob --fields name,output_type,enabled_for
ace outjob mediums /absolute/Release.OutJob --fields name,kind
ace check "$P" --pcb "$B" --require MPN,Manufacturer --fail-on error
ace designer compile "$P" --timeout 300
ace designer erc "$P" --timeout 300
ace designer drc "$B" --timeout 300
ace designer pcb info "$B"
ace designer outjob "$P" /absolute/Release.OutJob --medium "Release folder" --timeout 1200
ace bom "$P" --group-by designator --limit 0
ace diff "$P" --rev HEAD~1 --summary
```

Run the native gate first; it is instant and uses no licence seat. Never run write-effect
Designer commands (`annotate`, `process`, `script`) in a release job.

Verify: `ok:true` and exit 0 do not mean the design passed. Gate on `errors` of compile,
any ERC item with `level` 2 or higher, `count` of DRC, and `unrouted_connections` of `pcb info`.
Require every expected output to exist with nonzero size, in a fresh staging directory, and save
every envelope. Exits 10, 11 and 12 are infrastructure: retry or alert, do not call the design bad.
There is no native `release` transaction or output manifest. For SSH, read `designer config`
and `designer setup` before changing configuration.

### Recipe 5 - Apply an ECO and compare with its baseline

Trigger: "ECO", "engineering change", "change R20 to 22K", "change a value and MPN",
"add a variant", "do not populate", "adjust the clearance rule", "without opening Altium".

```bash
unset ACE_READONLY
ace sch locate "$P" R20,BF1
ace check "$P" --pcb "$B" > "$R/before.json"
ace sch set-param "$S" R20 Value 22K --dry-run
ace sch set-param "$S" R20 Value 22K
ace sch set-param "$S" R20 MPN RC0603FR-0722KL
ace check "$P" --pcb "$B" --baseline "$R/before.json" --fail-on error
ace diff "$P" --rev HEAD --by part
```

Use the returned sheet for each part. For a requested DNP variant or clearance ECO, continue:

```bash
ace project add-variant "$P" NoBuzzer --dry-run
ace project add-variant "$P" NoBuzzer
ace project set-variation "$P" NoBuzzer BF1
ace bom "$P" --variant NoBuzzer --group-by designator --limit 0
ace pcb set-rule "$B" Clearance gap 0.2mm --dry-run
ace pcb set-rule "$B" Clearance gap 0.2mm
ace project variant "$P" NoBuzzer
```

Verify: read `changes[].before` in each dry run; if it starts with `=`, stop and edit the
parameter it points at. Every write envelope has `verified`. The BOM reports `dnp` and a
smaller `total_parts`. Review new and resolved findings against `before.json`, the variant,
and the rule diff. The PCB-side comment and footprint stay old until Update PCB in Altium:
report `comment-mismatch` instead of "fixing" it. Re-run Designer checks when available.
Roll back one file with Recipe 12; commit the ECO as one revision for the rest.

### Recipe 6 - Change a footprint library without leaving stale references

Trigger: "rename the footprint", "replace the footprint", "copy a footprint from the library",
"swap the 0603 for 0402", "replace the package".

```bash
ace sch find "$S" SOIC-8
ace pcb components "$B" --where footprint=SOIC-8 --fields designator,footprint
ace pcblib list /absolute/Target.PcbLib --fields name,pads,mount
ace pcblib copy /absolute/Target.PcbLib /absolute/Source.PcbLib SOIC-8 --name SOIC-8N --dry-run
ace pcblib rename /absolute/Target.PcbLib SOIC-8 SOIC-8N --dry-run
ace sch set-footprint "$S" U3 SOIC-8N --dry-run
ace diff /absolute/before/Target.PcbLib /absolute/Target.PcbLib
```

`pcblib copy` takes the destination first. Discover references before applying the preview.

Verify: never rename a footprint before listing its users. A library rename does not update
sheets, SchLib defaults, or the PCB; `pcb set-component ... footprint` is refused with a designer
hint. List those as open items before claiming the rename is complete. `sch set-footprint` takes
one designator per call. Library diff excludes unknown streams and model payload bytes.

### Recipe 7 - Explain a red build

Trigger: "the build went red", "why is CI red", "what is wrong", "which parts or nets",
"explain this DRC", "did the tool crash", "is the board bad".

Read the saved envelopes first; they need no Altium call. Exit 9 means an explicit gate matched.
An `environment` or `timeout` error describes infrastructure, not design violations.

```bash
jq '.data.items | group_by(.rule) | map({rule: .[0].rule, n: length})' reports/drc.json
ace pcb net "$B" GND
ace pcb rule "$B" "Clearance"
ace sch component "$S" U5 --project "$P"
ace diff "$P" --rev HEAD~1 --summary
ace errors
ace designer jobs --last 3
```

Verify: name the rule, layer, the two objects with nets and the limit ("Clearance on TopLayer
between U5-3 and C41-1; rule requires 0.15 mm"). Separate design failures (check, compile, ERC,
DRC) from infrastructure failures (10, 11, 12). Propose fixes; do not apply them in a release job.

### Recipe 8 - Answer a part, pin, or net question

Trigger: "what footprint is U1", "what is on pin 9", "what connects to net X", "what is on GND",
"how long is net X", "which pins of U3 are unconnected", "where is R31".

```bash
ace sch locate "$P" U1
ace pcb components "$B" --where designator=U1 --fields designator,footprint,comment,side,x,y,rotation
ace sch component "$S" U1 --project "$P"
ace sch pins "$S" --component U1 --project "$P" --unconnected --fields pin,name,net
ace netlist "$P" --where name=GND --fields name,node_count,nodes
ace netlist "$P" --pins --where designator=R31
ace pcb net "$B" GND
ace pcb pads "$B" --component R31 --fields number,net
ace pcb lengths "$B" --fields net,routed_length,vias --limit 20
```

Use the actual returned designator. `sch pins` uses `pin` for the pin number; its old
`designator` alias is deprecated. `net:null` and `connected:false` mean no native net in that scope.

Verify: quote the command and the exact value with its unit (`--units mil` if the user thinks
in mil). `NOT_FOUND` is a finding: read `error.hint`, which lists valid designators. `ok:true`
with `count:0` for a `--where` filter means the part does not exist; re-check with
`ace sch locate`. Names may differ legally between sheet and board; `check` reports
`net-mismatch` when they matter.

### Recipe 9 - Search and audit libraries

Trigger: "which footprints have 0603", "find a 0603 footprint", "list pads of", "what is in this
library", "3D models of", "library audit".

```bash
ace pcblib find 0603 /absolute/libraries --pads
ace pcblib list /absolute/Footprints.PcbLib --where name~0603 --fields name,pads,mount
ace pcblib pads /absolute/Footprints.PcbLib R0603
ace pcblib models /absolute/Footprints.PcbLib --extract /absolute/models
ace schlib list /absolute/Symbols.SchLib --fields name,footprint
ace schlib show /absolute/Symbols.SchLib LM317
```

Verify: state which files were searched. Inspect `skipped` and `incomplete` before treating zero
as absence. IntLib containers need extraction in Altium; `raw streams` can inspect their storage.
`pad_numbers` is a legacy count, and `pad_number_values` lists the numbers. A model reference
without extracted bytes is an unresolved dependency.

### Recipe 10 - Start an Altium to KiCad migration

Trigger: "migrate to KiCad", "convert PrjPcb/PcbDoc/SchLib/PcbLib", "replace Altium",
"preserve libraries", "verify converted connectivity", "use ace with kce".
Freeze a source copy and record a recursive file/hash manifest outside both projects.

```bash
ace check "$P" --fail-on error
ace bom "$P" --group-by designator --include-dnp --include-excluded --out "$R/source-bom.csv"
ace sch project-nets "$P"
ace schlib show /absolute/Symbols.SchLib LM317
ace pcblib models /absolute/Footprints.PcbLib --extract "$R/models"
kce project new /tmp/target --name target
kce --json --project /tmp/target pcb import /absolute/source/Board.PcbDoc --format altium --output /tmp/target/target.kicad_pcb --report-format json --report-file /tmp/import.json
kce --json --project /tmp/target pcb info
```

Verify: the source gate fails on annotation errors, missing documents, or unexplained
schematic-versus-PCB differences even when `check` exits 0. Read back the exact `/tmp/target/target.kicad_pcb`: inspect footprints and require its hash
to differ from the empty board. Recompute the source manifest including file additions and deletions.
Require source equality. A successful board import is not a
verified migration. Library conversion and independent cross-format equivalence are not provided
by ace. Report "migration blocked" with the missing proof; never compare net counts. Discover kce's installed commands
before continuing. kce exit codes are categories, not ace's numbers. Keep the source oracle outside any project that a restore could replace.

### Recipe 11 - Summarize board size, stack, and rules

Trigger: "how big is the board", "board summary", "clearance", "design rules", "layer stack",
"stackup".

```bash
ace pcb info "$B"
ace pcb stackup "$B"
ace pcb layers "$B"
ace pcb rules "$B" --where kind^=Clearance
ace pcb rules "$B" --kind Clearance --units mil
ace pcb classes "$B"
ace pcb outline "$B"
ace pcb svg "$B" --out "$R/board.svg"
```

Verify: the size is `data.board.width` by `height` in mm unless `--units mil`. `stackup` reports
the physical stack and thicknesses; `layers` reports layer definitions. `--kind` matches an
exact kind, while `--where kind^=Clearance` finds every kind containing Clearance (several rule
kinds, not one). A rule with `values:{}` is enabled but undecoded: say so and read the warning.

### Recipe 12 - Recover an edit or investigate a stuck Designer

Trigger: "undo that edit", "restore the file", "Altium hangs", "compile never returns",
"DESIGNER_UNAVAILABLE", "Designer is stuck", "timeout", "a dialog is open", "set up Altium over SSH".

```bash
ace restore "$S" --from first --dry-run
ace restore "$S" --from first
ace designer status
ace designer dialogs
ace designer screenshot --out /tmp/designer.png
ace designer jobs --last 3
```

Restore validates the preserved first backup and reports `verified.reparsed` and `read_back`.
`first` means the oldest preserved `.bak`, not the last edit. Review both sizes and modification
times in the dry run. Restore saves the current bytes to `<file>.pre-restore` even with `--no-backup`;
archive that copy before another restore. It restores one file, not a whole ECO.

Verify: inspect dialogs and screenshots before using `designer reset` or retrying a timed-out job,
and retry a read operation once at most. Exit 10 is final until `designer setup` or
`designer config` changes: steps with `who:"user"` (licence sign-in, auto-logon) need the human.
Trust `designer ping`, not `doctor`, to confirm a licensed, responsive Altium. A timeout does not
prove that a remote edit was rolled back; do not retry mutations or exports blindly.
"#;

pub fn commands_md(root: &clap::Command) -> String {
    let mut s = String::new();
    s.push_str("# ace command reference\n\nGenerated from the command table of `ace ");
    s.push_str(env!("CARGO_PKG_VERSION"));
    s.push_str("` (`ace skill --file commands`). `backend/effect` after each name: native commands need nothing \
installed; designer commands drive Altium Designer; native `write` commands support `--dry-run` and keep the first `.bak` unless `--no-backup` is set.\n\n");
    s.push_str("## Global flags\n\n");
    for a in global_flags(root) {
        let name = a["long"].as_str().unwrap_or("");
        let mut line = format!("- `{name}` — {}", a["help"].as_str().unwrap_or(""));
        if let Some(d) = a["default"].as_str() {
            line.push_str(&format!(" (default `{d}`)"));
        }
        s.push_str(&line);
        s.push('\n');
    }
    s.push_str("\nList commands return `data.count` and `data.items`; `--fields/--where/--limit` act on `items` \
and add `matched`/`truncated`.\n");
    let mut group = String::new();
    for p in leaf_paths(root) {
        let g = p.split_whitespace().next().unwrap_or("").to_owned();
        if g != group {
            let about = find_cmd(root, &g).and_then(|c| c.get_about()).map(|a| a.to_string()).unwrap_or_default();
            s.push_str(&format!("\n## {g}\n\n{about}\n\n"));
            group = g;
        }
        let Some(d) = describe_json(root, &p, true) else { continue };
        s.push_str(&format!(
            "### `ace {p}`\n_{}/{}_ — {}\n\n",
            d["backend"].as_str().unwrap_or("?"),
            d["effect"].as_str().unwrap_or("?"),
            d["about"].as_str().unwrap_or("")
        ));
        let args = d["args"].as_array().cloned().unwrap_or_default();
        if !args.is_empty() {
            for a in args {
                let name = match a["kind"].as_str() {
                    Some("positional") => format!("<{}>", a["name"].as_str().unwrap_or("")),
                    _ => a["long"].as_str().unwrap_or("").to_owned(),
                };
                let mut extra = vec![];
                if a["required"] == true {
                    extra.push("required".to_owned());
                }
                if a["multiple"] == true {
                    extra.push("repeatable".to_owned());
                }
                if let Some(c) = a["choices"].as_array() {
                    extra.push(format!("one of {}", c.iter().filter_map(Value::as_str).collect::<Vec<_>>().join("|")));
                }
                if let Some(df) = a["default"].as_str() {
                    extra.push(format!("default `{df}`"));
                }
                let extra = if extra.is_empty() { String::new() } else { format!(" ({})", extra.join(", ")) };
                s.push_str(&format!("- `{name}`{extra} — {}\n", a["help"].as_str().unwrap_or("").replace('\n', " ")));
            }
            s.push('\n');
        }
        s.push_str(&format!("Example: `{}`\n\n", d["example"].as_str().unwrap_or("")));
        s.push_str(&format!("Returns: `{}`\n\n", d["returns"].as_str().unwrap_or("")));
        if let Some(n) = d["notes"].as_str().filter(|n| !n.is_empty()) {
            s.push_str(&format!("Notes: {n}\n\n"));
        }
    }
    s.push_str("## Errors\n\n| code | category | exit | meaning |\n|---|---|---|---|\n");
    for c in ErrorCode::all() {
        s.push_str(&format!("| `{}` | {} | {} | {} |\n", c.name(), c.category(), c.exit_code(), error_meaning(*c)));
    }
    s
}
