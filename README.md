<p align="center"><img src="https://raw.githubusercontent.com/Hybirdss/altium-cli-extended/main/docs/assets/banner.png" alt="altium-cli-extended" width="820"></p>

<div align="center">

# altium-cli-extended (`ace`)

**Native Rust parser, byte-safe editor, and Altium Designer automation bridge for AI agents and CI.**

<a href="https://github.com/Hybirdss/altium-cli-extended/actions/workflows/ci.yml"><img src="https://img.shields.io/github/actions/workflow/status/Hybirdss/altium-cli-extended/ci.yml?branch=main&label=CI" alt="CI"></a>
<a href="https://github.com/Hybirdss/altium-cli-extended/releases"><img src="https://img.shields.io/github/v/release/Hybirdss/altium-cli-extended?label=release" alt="latest release"></a>
<a href="https://github.com/Hybirdss/altium-cli-extended/blob/main/LICENSE"><img src="https://img.shields.io/badge/license-GPL--3.0--or--later-blue.svg" alt="GPL-3.0-or-later license"></a>

**English** | [한국어](https://github.com/Hybirdss/altium-cli-extended/blob/main/README.ko.md) | [简体中文](https://github.com/Hybirdss/altium-cli-extended/blob/main/README.zh-CN.md) | [繁體中文](https://github.com/Hybirdss/altium-cli-extended/blob/main/README.zh-TW.md) | [日本語](https://github.com/Hybirdss/altium-cli-extended/blob/main/README.ja.md) | [Español](https://github.com/Hybirdss/altium-cli-extended/blob/main/README.es.md) | [हिन्दी](https://github.com/Hybirdss/altium-cli-extended/blob/main/README.hi.md)

[Backends](#native-and-designer-backends) · [Install](#install) · [Quickstart](#quickstart) · [Agent Skill](#agent-skill) · [Workflows](#workflows) · [Commands](#command-groups) · [JSON Contract](#json-contract) · [Development](#development)

</div>

> **Working with KiCad 10?** See the companion tool **[kicad-cli-extended (`kce`)](https://github.com/Hybirdss/kicad-cli-extended)**. Both projects use GPL-3.0-or-later.

Altium Designer ships no official CLI — it is a Windows GUI application (`X2.EXE`). **`ace`** (Altium CLI Extended) fills that gap with a **dual-backend architecture**:

1. **Native Rust engine (`backend: native`):** Reads, inspects, diffs, checks, and makes byte-safe edits to `.PrjPcb`, `.SchDoc`, `.PcbDoc`, `.SchLib`, `.PcbLib`, and `.OutJob` files directly on **Windows, macOS, and Linux** — with **no Altium Designer installation required**.
2. **Designer bridge (`backend: designer`):** Drives your own licensed Altium Designer (on local Windows, remote Windows over SSH, or Wine on Linux) whenever a job needs Altium's engine (project compile, ERC, DRC, Gerber/ODB++/PDF/STEP Output Jobs, processes, or DelphiScript).

Every command outputs a single JSON envelope, every failure returns a stable error code and recovery hint, and `ace commands` / `ace describe` expose the entire command surface so AI agents and CI pipelines never have to guess.

---

## Native and designer backends

| Aspect | Native (`backend: native`) | Designer (`backend: designer`) |
|---|---|---|
| **Requires** | Nothing (standalone binary) | A user-licensed Altium Designer (`X2.EXE`) |
| **Runs on** | Windows, macOS, Linux | Local Windows, Remote Windows over SSH, or Wine on Linux |
| **Capabilities** | Inspect projects/sheets/boards/libraries, BOM, netlist, design checks, revision diffs, SVG previews, byte-safe parameter/rule/variant/footprint edits | Compile projects, ERC, DRC, execute `.OutJob` releases, run Altium processes & DelphiScript |

### Designer host modes

* **Local Windows (default):** Uses Altium Designer installed on the same machine within a logged-in desktop session (`conhost --headless` on Windows 10 1809+). Run `ace designer setup` to inspect prerequisites and `ace designer setup --apply runner` as the desktop user (Limited privilege; no elevation required) to register the per-user scheduled task. If Altium is installed in a non-default path, set `ACE_X2` to `X2.EXE`.
* **Remote Windows over SSH:** Run `ace` on macOS, Linux, or another PC and drive Altium on a Windows machine or VM over SSH with automatic project sync. `ace` enforces `StrictHostKeyChecking=yes` against `~/.ssh/known_hosts_altium` — verify the server fingerprint out-of-band and pin the host key before running setup.
* **Wine on Linux (optional):** Drives Altium Designer under Wine.

> **Byte-safe native writes & Designer note:**
> * Native writes (`effect: write`) support `--dry-run`, preserve the first `<file>.bak` (unless `--no-backup` is passed), and re-parse the modified bytes before replacing the file. Binary `.SchDoc`/`.PcbDoc`/`.PcbLib` edits keep untouched records and streams intact. Project and OutJob INI writes preserve entry order, line endings, and BOM, while normalizing section spacing and omitting comments (keep `.bak` if your INI files contain hand-written comments).
> * Known Designer limitation: Designer jobs leave `.PrjPcb` and bridge script projects (`AltiumCliBridge.PrjScr`, `Job.PrjScr`) open in the Projects panel, and a board after DRC appears modified. Close them manually from the Projects panel when needed and select **"Don't Save"** for files you did not edit.

---

## Install

### Prebuilt binaries

Download the archive for your platform from [GitHub Releases](https://github.com/Hybirdss/altium-cli-extended/releases):

| OS | Archive | Binary |
|---|---|---|
| **Windows x64** | `altium-cli-extended-x86_64-pc-windows-msvc.zip` | `ace.exe` |
| **macOS Apple Silicon** | `altium-cli-extended-aarch64-apple-darwin.tar.gz` | `ace` |
| **macOS Intel** | `altium-cli-extended-x86_64-apple-darwin.tar.gz` | `ace` |
| **Linux x86_64** | `altium-cli-extended-x86_64-unknown-linux-gnu.tar.gz` | `ace` |
| **Linux arm64** | `altium-cli-extended-aarch64-unknown-linux-gnu.tar.gz` | `ace` |

Each release includes a `<archive>.sha256` checksum (`shasum -a 256 -c` on macOS/Linux, `Get-FileHash` on Windows) alongside the binary, `README.md`, `LICENSE`, `NOTICE`, and `skills/`.

```powershell
# Windows (PowerShell)
Expand-Archive altium-cli-extended-x86_64-pc-windows-msvc.zip -DestinationPath $env:LOCALAPPDATA\altium-cli-extended
$userPath = [Environment]::GetEnvironmentVariable("Path", "User")
[Environment]::SetEnvironmentVariable("Path", "$userPath;$env:LOCALAPPDATA\altium-cli-extended", "User")
$env:Path += ";$env:LOCALAPPDATA\altium-cli-extended"
ace.exe doctor --quick
```

```bash
# macOS / Linux
mkdir -p altium-cli-extended && tar xzf altium-cli-extended-*.tar.gz -C altium-cli-extended
sudo mv altium-cli-extended/ace /usr/local/bin/
ace doctor --quick
```

*macOS Gatekeeper note:* Prebuilt macOS binaries are unsigned. After verifying the SHA-256 checksum, if Gatekeeper blocks execution, run `xattr -d com.apple.quarantine /usr/local/bin/ace`.

### Build from source (Rust 1.89+)

```bash
cargo install --locked --git https://github.com/Hybirdss/altium-cli-extended altium-cli-extended
```

Run `ace doctor --quick` to check native readiness without contacting a Designer host, or `ace doctor` for the full Designer host probe.

---

## Quickstart

```bash
ace inspect Board.PrjPcb                          # summarize any project, sheet, board, or library
ace bom Board.PrjPcb --out bom.csv                # grouped BOM; add --variant <name> for a variant
ace netlist Board.PrjPcb --where name=GND         # inspect a net and its connected pins
ace check Board.PrjPcb                            # check annotation, footprints, schematic vs PCB
ace diff Board.PrjPcb --rev HEAD~1                # compare against a git revision
ace pcb rules Board.PcbDoc --kind Clearance --units mil
ace sch set-param Power.SchDoc R5 MPN RC0603FR-0710KL --dry-run
ace pcblib list Footprints.PcbLib --where mount=smd --fields name,pads
ace pcblib rename Footprints.PcbLib SOIC-8 SOIC-8N --dry-run
```

* **Filtering & projection:** Shape any list output with `--fields`, `--where` (`key=value`, `key!=value`, `key~regex`, `key^=substring`), and `--limit`.
* **Units & encoding:** Lengths default to `mm` unless `--units mil|raw` or `ACE_UNITS` is set. Legacy string encoding defaults can be set with `--codepage` or `ACE_CODEPAGE`.

---

## Agent skill

The repository ships a generated AI agent skill in [`skills/altium-cli-extended/`](skills/altium-cli-extended/): a concise [`SKILL.md`](skills/altium-cli-extended/SKILL.md) (core rules + 12 trigger-driven recipes) and the complete [`COMMANDS.md`](skills/altium-cli-extended/COMMANDS.md) reference.

```bash
ace skill install                          # ~/.agents/skills plus detected agents (Claude, Codex, Gemini)
ace skill install --target claude          # or codex, gemini, agents, all
ace skill install --dir ./.claude/skills   # project-local install
ace skill status                           # check installed copies and currency
ace skill uninstall --target all
```

Files are copied directly so installation behaves identically across Windows, macOS, and Linux (`--target` and `--dir` are mutually exclusive).

---

## Workflows

Start from your goal. In the command chains below, `$P` is the project (`.PrjPcb`), `$B` is the board (`.PcbDoc`), `$S` is a schematic sheet (`.SchDoc`), and `$R` is a report directory outside the source tree. Always use **absolute paths** (a relative `Board.PrjPcb` cannot resolve parent-relative sheets like `..\Utils\*.SchDoc`).

| Goal | Entry Command | Section |
|---|---|---|
| Audit a contractor handover without Altium | `ace project find <dir>` | [Review a contractor's handover](#review-a-contractors-handover) |
| Inspect schematic, PCB, or library changes between revisions | `ace diff A.PrjPcb B.PrjPcb --summary` | [Compare revisions](#compare-revisions) |
| Fail a CI build or block a release on design regressions | `ace check P.PrjPcb --fail-on error` | [Gate a design in CI](#gate-a-design-in-ci) |
| Edit component values, variants, or PCB rules headlessly | `ace sch set-param ... --dry-run` | [Apply an ECO without opening Altium](#apply-an-eco-without-opening-altium) |
| Rename or copy a footprint across libraries safely | `ace pcblib rename ... --dry-run` | [Rename a footprint without stale references](#rename-a-footprint-without-stale-references) |
| Locate pins, nets, stackup, and footprints | `ace sch locate P.PrjPcb U1` | [Find parts, nets, and library footprints](#find-parts-nets-and-library-footprints) |
| Run full compile, ERC, DRC, and `.OutJob` exports | `ace designer ping --timeout 60` | [Prepare a release with Designer](#prepare-a-release-with-designer) |
| Roll back an edit or recover a stuck Altium session | `ace restore F --from first --dry-run` | [Recover an edit or unstick Designer](#recover-an-edit-or-unstick-designer) |
| Migrate an Altium board into KiCad | `ace check P.PrjPcb --fail-on error` | [Altium to KiCad](#altium-to-kicad) |

### Review a contractor's handover

Discover the project first, set `ACE_READONLY=1` so write and Designer-control commands are blocked before any file access, and write reports outside the source directory:

```bash
ace project find /absolute/handover --depth 6
P=/absolute/handover/Board.PrjPcb
B=/absolute/handover/Board.PcbDoc
R=/absolute/review
mkdir -p "$R"
export ACE_READONLY=1
ace inspect "$P"
ace project documents "$P" --fields rel,kind,exists
ace sch hierarchy "$P"
ace pcb info "$B"
ace bom "$P" --group-by designator --out "$R/parts.csv"
ace bom "$P" --source pcb --limit 1
ace netlist "$P" --out "$R/source.net"
ace check "$P" --severity warning --require MPN
ace pcb svg "$B" --out "$R/board.svg"
```

* Run `ace check --list` to see what each finding `code` means and how to fix it; report findings by `code`, not only by `passed`.
* Check `missing_documents`, `incomplete`, and declared vs. found sheet counts before trusting a BOM, and reconcile schematic vs. PCB part counts.
* If `meta.suggest.codepage` or a warning appears, set `ACE_CODEPAGE` and re-run exports. Native checks do not replace Designer compile, ERC, DRC, or fab output verification.

### Compare revisions

```bash
ace diff /absolute/RevA/Board.PrjPcb /absolute/RevB/Board.PrjPcb --summary
ace diff /absolute/RevA/Board.PrjPcb /absolute/RevB/Board.PrjPcb --by part --ignore-field '^param:ComponentLink'
ace diff /absolute/RevA/Board.PrjPcb /absolute/RevB/Board.PrjPcb --where change=part_added --fields ref
ace diff /absolute/RevA/Board.PcbDoc /absolute/RevB/Board.PcbDoc --geometry
ace diff "$P" --rev HEAD~1 --source pcb
```

* Start with `--summary`, then filter with `--where` and `--fields`.
* Components match by Unique ID before designator, so re-annotation appears as `part_renamed` while preserving concurrent parameter diffs.
* `identical: true` on schematics does not cover the board — always run the `.PcbDoc` diff (or `--source pcb`) as well.

### Gate a design in CI

```bash
ace check "$P" --list
ace check "$P" --pcb "$B" --require MPN,Manufacturer --fail-on error > "$R/check.json"
ace diff "$P" --rev HEAD~1 --exit-code
ace check "$P" --pcb "$B" --require MPN,Manufacturer --baseline "$R/check.json" --fail-on error
```

* `check --fail-on error` and `diff --exit-code` keep normal JSON on `stdout` and exit `9` when the gate triggers. Always archive the JSON envelope even on exit `9`.
* Passing `--baseline` counts only *new* findings against a prior unshaped check envelope run with the same options and board.
* Exit codes `10`, `11`, and `12` indicate that the Altium Designer host is missing, failed, or busy — never interpret them as design pass/fail results.

### Apply an ECO without opening Altium

```bash
ace check "$P" --pcb "$B" > "$R/before.json"
unset ACE_READONLY
ace sch locate "$P" R20,BF1
ace sch set-param /absolute/handover/Power.SchDoc R20 Value 22K --dry-run
ace sch set-param /absolute/handover/Power.SchDoc R20 Value 22K
ace project add-variant "$P" NoBuzzer --dry-run
ace project add-variant "$P" NoBuzzer
ace project set-variation "$P" NoBuzzer BF1
ace bom "$P" --variant NoBuzzer --group-by designator --limit 0
ace pcb set-rule "$B" Clearance gap 0.2mm --dry-run
ace pcb set-rule "$B" Clearance gap 0.2mm
ace check "$P" --pcb "$B" --baseline "$R/before.json" --fail-on error
ace diff "$P" --rev HEAD --by part
```

* Inspect `changes[].before` in every `--dry-run`: `=Value` is an indirection link (edit its `Value` parameter, or pass `--literal` to overwrite the link itself).
* Rule lengths require an explicit unit (`mm` or `mil`).
* Schematic edits leave the PCB with its previous comment/footprint until you run Update PCB in Altium, so `comment-mismatch` is expected. Keep a separate project copy for multi-file work, as `ace` does not provide multi-file project transactions.

### Rename a footprint without stale references

```bash
ace sch find "$S" SOIC-8
ace pcb components "$B" --where footprint=SOIC-8 --fields designator,footprint
ace pcblib copy /absolute/Target.PcbLib /absolute/Source.PcbLib SOIC-8 --name SOIC-8N --dry-run
ace pcblib rename /absolute/Target.PcbLib SOIC-8 SOIC-8N --dry-run
ace sch set-footprint "$S" U3 SOIC-8N --dry-run
ace diff /absolute/before/Target.PcbLib /absolute/Target.PcbLib
```

* Audit all references to a footprint before renaming it. Note that `pcblib copy` takes the **destination library first**.
* Renaming a footprint in a `.PcbLib` updates only that library; schematic sheets, `.SchLib` defaults, and `.PcbDoc` boards keep the old footprint name until updated (`sch set-footprint` updates one designator per call; updating placed PCB footprints requires Altium Designer).

### Find parts, nets, and library footprints

```bash
ace sch locate "$P" R20,BF1
ace netlist "$P" --pins --where designator=R20
ace sch pins /absolute/handover/Power.SchDoc --component R20 --project "$P" --unconnected --fields pin,name,net
ace pcblib find 0603 /absolute/libraries --pads
ace sch component "$S" R20 --project "$P"
ace pcb lengths "$B" --fields net,routed_length,vias --limit 20
ace pcb info "$B"
ace pcb stackup "$B"
ace pcb rules "$B" --where kind^=Clearance
```

* Use `pin` for pin numbers; unconnected pins return `net: null` and `connected: false`.
* Check `skipped` and `incomplete` in library searches before treating zero matches as absence (`.IntLib` extraction requires Altium Designer).
* `--kind Clearance` matches that exact rule kind; `--where kind^=Clearance` matches rule kinds starting with `Clearance`.

### Prepare a release with Designer

```bash
ace doctor --quick
ace designer status
ace designer ping --timeout 60
ace designer compile "$P" --timeout 300
ace designer erc "$P" --timeout 300
ace designer drc "$B" --timeout 300
ace designer pcb info "$B"
ace outjob outputs /absolute/Release.OutJob
ace outjob mediums /absolute/Release.OutJob
ace designer outjob "$P" /absolute/Release.OutJob
```

* Gate the release on zero compile errors, zero ERC errors, zero DRC violations, and zero unrouted nets before running `designer outjob`.
* Export into a clean staging directory and verify every expected output file's kind, size, and SHA-256 hash before publishing.
* In PowerShell, pass paths with spaces via an argument array and check `$LASTEXITCODE` explicitly:

```powershell
$AceArgs = @('--json', '--read-only')
$Project = 'C:\Designs\My Board\Board.PrjPcb'
& ace.exe @AceArgs check $Project --fail-on error
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
```

### Recover an edit or unstick Designer

```bash
ace restore "$S" --from first --dry-run
ace restore "$S" --from first
ace designer status
ace designer dialogs
ace designer screenshot --out /tmp/designer.png
ace designer jobs --last 3
```

* Native writes preserve the oldest backup at `<file>.bak` (`--from first`). Restoring saves the current file to `<file>.pre-restore` before replacing it (and refuses if `.pre-restore` already exists).
* If Altium Designer hangs on a modal dialog, inspect `ace designer dialogs` and `ace designer screenshot` before running `ace designer reset`, and treat any timed-out job as unverified.

### Altium to KiCad

Freeze a copy of the source and record a recursive file/hash manifest outside both projects. Import into an explicit target path: `--project` alone does not select the import output.

```bash
kce project new /tmp/target --name target
kce --json --project /tmp/target pcb import /absolute/source/Board.PcbDoc --format altium --output /tmp/target/target.kicad_pcb --report-format json --report-file /tmp/import.json
kce --json --project /tmp/target pcb info
```

Read back the exact target board: require the reported path to equal `/tmp/target/target.kicad_pcb`, inspect the imported footprints and compare its hash with the empty board. Recompute the source manifest including file additions and deletions; require it to be identical. Stop if the board is still empty. A successful board import is not a
verified migration: symbol/library conversion and independent electrical equivalence still need separate evidence. Keep the source oracle outside the KiCad project so restore cannot remove it. Discover supported KiCad steps with `kce --help` before continuing.

---

## Command groups

| Group | Purpose |
|---|---|
| `project` | Project discovery, documents, parameters, variants (and variant edits), settings |
| `sch` | Schematic sheets: components, pins, nets, labels, ports, hierarchy, parameter/comment/footprint/designator edits |
| `schlib` | Schematic symbol libraries: symbols, pins, parameter and description edits |
| `pcb` | PCB boards: layers, stackup, components, nets, rules, classes, primitives, holes, routed lengths, SVG; rule/class/parameter edits |
| `pcblib` | Footprint libraries: footprints, pads, 3D models, SVG; byte-safe `rename`, `set-param`, `remove`, `copy` |
| `outjob` | Output Job files: outputs, mediums, enable/disable |
| `bom`, `netlist`, `check`, `diff` | Cross-document BOM generation, netlist extraction, design rule checks, and revision/git diffs |
| `designer` | Live Altium Designer bridge: setup, status, dialogs, screenshot, compile, ERC, DRC, Output Jobs, processes, scripts |
| `raw` | Low-level OLE/CFB stream and record inspection on any compound file |
| `commands`, `describe`, `skill`, `errors`, `doctor`, `inspect`, `restore` | Self-discovery, JSON schema/shape inspection, agent skill management, diagnostics, and backup recovery |

---

## JSON contract

When piped or invoked with `--json`, `stdout` emits exactly one JSON envelope:

```json
{"ok":true,"schema":"altium-cli-extended/v1","command":"pcb nets","data":{"count":2,"items":[...]},
 "warnings":[],"meta":{"units":"mm","backend":"native","effect":"read","dry_run":false,...}}
{"ok":false,"schema":"altium-cli-extended/v1","command":"pcb net","error":{"code":"NOT_FOUND","category":"input",
 "message":"no net \"CLKK\"","hint":"see `ace pcb nets` for the net names"}}
```

* **List envelopes:** Always return `data.count` and `data.items`.
* **Diagnostics & warnings:** `warnings` reports partial decoding, approximations, and codepage suggestions (`warnings` and `meta` appear on `ok: true` envelopes; `ok: false` envelopes carry `error` with optional `hint` and `details`).
* **Exit codes:**
  * `0`: Command succeeded (`ok: true`).
  * `9`: Explicit check or diff gate triggered (`check --fail-on`, `diff --exit-code`) — `ok` remains `true` on `stdout` so CI can archive the report.
  * `10`, `11`, `12`: Designer host unavailable, failed, or busy.
  * Run `ace errors` for the complete table of error codes, categories, and exit codes.
* **Self-description:** Run `ace commands` or `ace describe <command>` (e.g. `ace describe pcb nets`) for arguments, return shape, and working examples.

---

## Development

```bash
git clone https://github.com/Hybirdss/altium-cli-extended
cd altium-cli-extended
scripts/fetch-fixtures.sh        # sample projects into .fixtures/ (Windows: scripts/fetch-fixtures.ps1)
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
python3 scripts/check-examples.py --ace target/debug/ace
ACE_UPDATE_SKILL=1 cargo test -p altium-cli-extended --test skill   # regenerate skills/ after a command change
```

See [ARCHITECTURE.md](ARCHITECTURE.md) for crate layering and binary format conventions.

---

## License

GPL-3.0-or-later, see [LICENSE](LICENSE). [NOTICE](NOTICE) credits the sources against which the Altium record layouts were written: KiCad's Altium importers (GPL-3.0-or-later) and `python-altium`'s format notes.

**Trademark & License Disclaimer:** `altium-cli-extended` (`ace`) is an independent open-source project and is not affiliated with, endorsed by, or sponsored by Altium LLC or Renesas Electronics Corporation. "Altium" and "Altium Designer" are registered trademarks of Altium LLC. Native commands (`backend: native`) operate directly on user design files using an independent parser; `designer` commands (`backend: designer`) require a separate, validly licensed installation of Altium Designer provided by the user.
