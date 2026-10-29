# altium-cli-extended architecture

`ace` (the binary of the `altium-cli-extended` crate) is a command line an AI agent can drive without guessing: every command is
discoverable (`ace commands`), every result is one JSON envelope, every failure has a stable
code and a hint, and nothing in a design file is out of reach.

## Layers

```
altium-cli-extended   binary `ace` — clap commands, envelope, output shaping, manifest/skill
  ├─ altium-analysis   BOM, netlist, checks, diffs over model::Design
  ├─ altium-designer   real Altium Designer on a host (local Windows, Windows over SSH, Wine): discovery, sessions, bridge
  └─ altium-doc        typed documents: project, outjob, sch, schlib, pcb, pcblib, model
       └─ altium-format     containers: CFB, record framing, |K=V| props, INI, safe writes
            └─ altium-core       Error/ErrorCode/Result, Coord/Point/Unit
```

The six crates live in `crates/`; only `altium-cli-extended` is published, the others are internal
(`publish = false`). Dependencies point down only. A lower crate never knows about a higher one; in particular
nothing below `altium-cli-extended` uses clap, prints, reads environment flags or builds envelopes.

| crate | owns | must not |
|---|---|---|
| core | error codes + exit codes, internal length unit | depend on anything Altium-specific |
| format | byte-level containers, codepage decoding, atomic file replace | know what a record *means* |
| doc | meaning of every record/section; typed models; edits that return new bytes | touch the CLI, decide output shape |
| analysis | cross-document reasoning over `model::Design` | parse files itself |
| designer | everything about running Altium (hosts: local Windows, SSH, Wine; paths, bridge protocol, DelphiScript) | parse design files (use doc) |
| cli | argument parsing, envelope, `--fields/--where/--limit`, manifest, SKILL.md | contain format or domain logic |

## Conventions

### Errors
All layers return `altium_core::Result<T>`. Use the most specific `ErrorCode`; attach a `hint`
that tells the caller the next step (a command to run, the valid names). Partial decoding is a
*warning* (`warnings: Vec<String>` on the document), never silent and never an error unless
nothing usable was decoded.

### Units
Every length is `Coord` (Altium internal units, 1e-4 mil, `i64`); points are `Point`. Serialising
a `Coord` emits the unit chosen by `--units` (presentation setting held in `altium_core::units`).
Angles are degrees `f64`. Never convert to mm inside doc/analysis code.

### Document modules (`altium-doc`)
- `pub fn load(path) -> Result<Doc>` (or `load_with(path, &LoadOptions)` when parsing everything
  is expensive) returning a typed struct deriving `Serialize`. The serialised form *is* the JSON
  contract, so field names are snake_case, stable, and documented with `///`.
- Typed fields for everything understood; `raw: Option<Props>` on objects, filled only when the
  caller asks (`LoadOptions::keep_raw`), so `--raw` can show what is not modelled.
- Unknowns are omitted (`Option`/`skip_serializing_if`), never defaulted to fake values.
- Each object carries a stable reference usable in a follow-up command (designator, net name,
  record index, layer name, sheet file).
- Edits are methods on the typed document that keep every untouched record/byte as-is, plus
  `to_bytes()`. The CLI writes through `Ctx::write_file` (dry run, `.bak`, atomic rename) and
  re-parses the result to verify.
- `model.rs` is the document-independent view (`Part`, `Net`, `Node`, `Design`); sch and pcb each
  provide a `design(...)` conversion so analysis treats both alike.

### Commands (`altium-cli-extended/src/commands/<group>.rs`)
- One module per group: a clap `Subcommand` enum, `pub fn run(cmd, &Ctx) -> Result<Out>`, and
  `pub const META: &[CmdMeta]` with exactly one row per leaf command (backend, effect, return
  shape, example, notes). `manifest_complete` fails the build's tests on any mismatch.
- Handlers are thin: resolve inputs, call doc/analysis/designer, wrap with `Out`.
- Lists return `{"count", "items", ...}` via `output::list` so the global shaping flags work.
- Write commands report `{changed, before, after, backup}`-style data and honour `--dry-run`.

### PcbLib edits (`altium-doc/src/pcblib/edit.rs`)
`ace pcblib rename|set-param|remove|copy` are byte-safe: a footprint lives in its own compound-file
storage (`<storage>/Data`, `Parameters`, `Header`, `WideStrings`, ...) and the footprint list is the
tail of `Library/Data`. An edit rewrites only the streams it must touch (a `Parameters` field keeps
every other byte via `altium_format::rawprops`; a rename gives `Data` a new length and name in front
of unchanged primitives and moves the storage, named after the footprint with `/` as `_`, cut at 31
characters; `copy` carries the footprint's streams and any 3D model the target lacks). Each function
(`set_param`, `rename`, `remove`, `copy_from`) returns an `Edit` (new bytes plus a `Change` list), and
`verify` re-parses it and checks that every stream outside the edit is byte-identical before the CLI
writes through `Ctx::write_file`. Geometry (pads, tracks, 3D bodies) is not edited natively.

### Environment and configuration
Environment variables are all `ACE_*`: `ACE_UNITS` and `ACE_CODEPAGE` (defaults of `--units` and
`--codepage`, read by clap in `main.rs`), `ACE_X2` (path of `X2.EXE`, `altium-designer`),
`ACE_FIXTURES` (sample projects for tests) and `ACE_UPDATE_SKILL` (regenerate `skills/` in the skill
test). Designer configuration is `designer.json` in the `altium-cli-extended` config directory
(`$XDG_CONFIG_HOME` or `~/.config`, `%APPDATA%` on Windows); per-user state (locks, sync manifests)
lives in `~/.local/state/altium-cli-extended` or `%LOCALAPPDATA%\altium-cli-extended`, and the bridge
in `C:\altium-cli-extended` on the Windows host.

### Backends
`backend: native` commands run anywhere, instantly. `backend: designer` commands drive a real
Altium Designer through `altium-designer` and are the authority for anything Altium computes
(compile, ERC, DRC, fabrication outputs, live edits). Native code may approximate (e.g. schematic
connectivity) but must say so in notes/warnings and point at the designer command. The designer
host is Altium Designer on the local Windows machine (default), on a Windows machine reached over
SSH, or under Wine on Linux. Native writes only change what can be rewritten byte-safely (text
property fields and records, whole PcbLib footprints); geometry and binary records are designer
territory.

### Agent skill
`skills/altium-cli-extended/SKILL.md` (short guide) and `COMMANDS.md` (full reference) are
generated from the command table by `commands/skill.rs` (the fixed text of `SKILL.md` is the `SKILL_BODY`
constant there; the command groups and `COMMANDS.md` come from the `META` rows); `ace skill install` copies
them into agent skill directories. A test fails when the committed copies are stale
(`ACE_UPDATE_SKILL=1 cargo test -p altium-cli-extended --test skill` regenerates them).

## Tests
- Unit tests next to the code.
- Fixture tests: real projects in `.fixtures/samples` (gitignored; `scripts/fetch-fixtures.sh`
  downloads them). Tests skip when the directory is absent.
- CLI tests (`crates/altium-cli-extended/tests`) run the built binary on copies of the samples and check
  envelopes, exit codes and that writes read back.
