# ace command reference

Generated from the command table of `ace 0.1.0` (`ace skill --file commands`). `backend/effect` after each name: native commands need nothing installed; designer commands drive Altium Designer; native `write` commands support `--dry-run` and keep the first `.bak` unless `--no-backup` is set.

## Global flags

- `--json` — Force JSON output (default when stdout is not a terminal)
- `--text` — Force human-readable output (tables)
- `--pretty` — Pretty-print JSON
- `--units` — Unit for every length/coordinate in the output (default `mm`)
- `--codepage` — ANSI code page of the design files (e.g. windows-1251, euc-kr, gbk, shift_jis) (default `windows-1252`)
- `--fields` — Keep only these fields of each list item (comma-separated, dotted paths allowed)
- `--where` — Filter list items: key=value, key!=value, key~regex, key^=substring. Repeatable (AND)
- `--limit` — Keep at most N list items
- `--raw` — Include raw Altium properties; with skill/skill show, print Markdown directly
- `--read-only` — Refuse design writes and control operations before any file or host access
- `--dry-run` — Write commands: report the change without writing anything
- `--no-backup` — Write commands: do not keep a <file>.bak copy

List commands return `data.count` and `data.items`; `--fields/--where/--limit` act on `items` and add `matched`/`truncated`.

## commands

List every command with backend, effect, return shape, arguments and an example

### `ace commands`
_meta/read_ — List every command with backend, effect, return shape, arguments and an example

- `<group>` — Only commands under this group (e.g. `pcb`, `designer`)
- `--brief` — Only path and up to 48 characters of whole words, with ... when shortened

Example: `ace commands pcb --brief`

Returns: `{count, items:[{path, about, backend, effect, returns, example, notes, args:[...]}]}`

Notes: Call this first. backend=native needs nothing installed; backend=designer needs Altium Designer on this Windows machine, a Windows machine over SSH, or Wine (see `ace doctor`).


## describe

Show one command in full: arguments, return shape, example, notes

### `ace describe`
_meta/read_ — Show one command in full: arguments, return shape, example, notes

- `<path>` (required, repeatable) — Command path, e.g. `pcb nets` or `designer drc`

Example: `ace describe designer drc`

Returns: `{path, about, backend, effect, returns, example, notes, args, flags, global_flags, supports_dry_run}`


## skill

Print the agent skill (SKILL.md, or COMMANDS.md with --file commands), generated from the live command table; `skill install|status|uninstall` manage installed copies

### `ace skill`
_meta/read_ — Print the agent skill (SKILL.md, or COMMANDS.md with --file commands), generated from the live command table; `skill install|status|uninstall` manage installed copies

- `--file` (one of skill|commands, default `skill`) — Which file to print

Example: `ace skill --file commands`

Returns: `{name, file, markdown, files}`

Notes: SKILL.md is the short guide (when to use, rules, workflows); COMMANDS.md is the full reference. Both are generated from this binary.

### `ace skill show`
_meta/read_ — Read SKILL.md (or COMMANDS.md with --file commands); --raw/--text prints Markdown

- `--file` (one of skill|commands, default `skill`) — Input Altium document path; quote spaces

Example: `ace skill show --raw`

Returns: `{name, file, markdown, files}`

Notes: --raw or --text prints Markdown directly; default remains a JSON envelope.

### `ace skill install`
_meta/control_ — Copy SKILL.md and COMMANDS.md into agent skill directories

- `--target` (repeatable, one of agents|claude|codex|gemini|all) — Agents to install for (repeatable). Default: `agents` plus every agent whose home directory (~/.claude, ~/.codex, ~/.gemini) exists
- `--dir` — Install into DIR/altium-cli-extended instead (a project's `.claude/skills`, say)

Example: `ace skill install --target claude --target codex`

Returns: `{skill, version, count, items:[{target, path, written, unchanged, replaced_symlink}]}`

Notes: Copies SKILL.md + COMMANDS.md to <home>/.agents|.claude|.codex|.gemini/skills/altium-cli-extended (default: .agents plus every agent home that exists), or DIR/altium-cli-extended with --dir. Works the same on Windows, macOS and Linux; honours --dry-run.

### `ace skill status`
_meta/read_ — Where the skill is installed and whether each copy matches this binary

- `--dir` — Also check DIR/altium-cli-extended

Example: `ace skill status`

Returns: `{skill, version, count, items:[{target, path, installed, files, up_to_date, symlink}]}`

Notes: up_to_date=false: the copy was written by another version; rerun `skill install`.

### `ace skill uninstall`
_meta/control_ — Remove installed copies (only the files this tool writes)

- `--target` (repeatable, one of agents|claude|codex|gemini|all) — targets for this operation; see the example and notes
- `--dir` — Directory path

Example: `ace skill uninstall --target all`

Returns: `{skill, count, items:[{target, path, removed}]}`

Notes: Removes only SKILL.md/COMMANDS.md (and the directory when it is then empty).


## restore

Restore a design file from its preserved first .bak, after parsing the backup

### `ace restore`
_native/write_ — Restore a design file from its preserved first .bak, after parsing the backup

- `<file>` (required) — Current design file; its original backup is <file>.bak
- `--from` (one of first, default `first`) — Oldest preserved backup, not the last edit

Example: `ace restore Board.SchDoc --from first --dry-run`

Returns: `{file, from, backup, pre_restore, current:{bytes,modified_unix_ms}, restore_source:{bytes,modified_unix_ms}, changed, verified:{reparsed,read_back}, bytes}`

Notes: first means the oldest preserved .bak, not the last edit. Re-parses it before replacing one design file. Saves current bytes to <file>.pre-restore even with --no-backup; an existing recovery copy blocks replacement. --dry-run reports both sizes and modification times without writing. The .bak is kept; recovery covers one file.


## errors

List error codes and exit codes

### `ace errors`
_meta/read_ — List error codes and exit codes

Example: `ace errors`

Returns: `{count, items:[{code, exit_code, category, meaning}]}`


## doctor

Check what works on this machine: native parsing, Altium Designer host (local Windows, SSH or Wine), bridge

### `ace doctor`
_meta/read_ — Check what works on this machine: native parsing, Altium Designer host (local Windows, SSH or Wine), bridge

- `--quick` — Native/configuration summary only: never contact a Designer host

Example: `ace doctor --quick`

Returns: `{native, designer:{host, altium, bridge, ...}, ready:{native, designer}}`

Notes: Use --quick for offline/native tasks (no host contact). Full doctor probes Designer; run when a designer command fails with DESIGNER_UNAVAILABLE. Native commands never need anything installed.


## inspect

Identify any Altium file and summarise it (dispatches to the right `info` command)

### `ace inspect`
_native/read_ — Identify any Altium file and summarise it (dispatches to the right `info` command)

- `<file>` (required) — Input Altium document path; quote spaces

Example: `ace inspect Board.PcbDoc`

Returns: `same as the matching `<kind> info` command plus {kind}`

Notes: Use when you do not know what a file is.


## raw

Low-level access to any compound document: streams and records

### `ace raw streams`
_native/read_ — List every stream in a compound document (.SchDoc/.SchLib/.PcbDoc/.PcbLib/.IntLib)

- `<file>` (required) — Input Altium document path; quote spaces

Example: `ace raw streams Board.PcbDoc`

Returns: `{count, items:[{path, size}]}`

Notes: Escape hatch: see every stream even if no dedicated command decodes it.

### `ace raw records`
_native/read_ — Decode a stream as length-prefixed records (text records shown as property maps)

- `<file>` (required) — Input Altium document path; quote spaces
- `<stream>` (required) — Stream path, e.g. `FileHeader`, `Board6/Data`, `Nets6/Data`
- `--hex` — Show binary records as hex (otherwise just their size)

Example: `ace raw records Sheet1.SchDoc FileHeader --where props.RECORD=1`

Returns: `{stream, count, leftover_bytes, items:[{index, kind, size, props?|hex?}]}`

### `ace raw dump`
_native/export_ — Copy one stream's bytes to a file (or hex dump it when no --out is given)

- `<file>` (required) — Input Altium document path; quote spaces
- `<stream>` (required) — Compound stream path from `ace raw streams`
- `--out` — Write bytes here instead of printing hex
- `--max` (default `512`) — Hex dump at most this many bytes

Example: `ace raw dump Board.PcbDoc Board6/Data --out board6.bin`

Returns: `{stream, size, out?|hex?}`


## project

Project files (.PrjPcb): documents, parameters, variants, output settings

### `ace project info`
_native/read_ — Summary: documents by kind, variants, parameters, key compile options

- `<project>` (required) — Project (.PrjPcb) path; resolves all referenced documents

Example: `ace project info Board.PrjPcb`

Returns: `{name, path, documents:{schematic:N,...}, missing_documents:[...], variants:[names], parameters:N, options:{...}}`

### `ace project documents`
_native/read_ — Documents in the project with resolved paths and whether they exist

- `<project>` (required) — Project (.PrjPcb) path; resolves all referenced documents
- `--kind` — Only this kind (schematic, pcb, schlib, pcblib, outjob...)
- `--generated` — Include generated (output) documents

Example: `ace project documents Board.PrjPcb --kind schematic`

Returns: `{count, items:[{index, rel, path, kind, exists, unique_id, generated?}]}`

Notes: Paths are resolved case-insensitively; exists=false means the project references a missing file.

### `ace project parameters`
_native/read_ — Project-level parameters (usable as =Name special strings on sheets/PCB)

- `<project>` (required) — Project (.PrjPcb) path; resolves all referenced documents

Example: `ace project parameters Board.PrjPcb`

Returns: `{count, items:[{name, value}]}`

### `ace project variants`
_native/read_ — Assembly variants with their per-designator variations

- `<project>` (required) — Project (.PrjPcb) path; resolves all referenced documents

Example: `ace project variants Board.PrjPcb`

Returns: `{count, items:[{index, name, unique_id, allow_fabrication, variations:[{designator, kind, alternate_part}], parameters, param_variations}]}`

Notes: kind: not_fitted | fitted (parameter-only) | alternate.

### `ace project variant`
_native/read_ — One variant in full

- `<project>` (required) — Project (.PrjPcb) path; resolves all referenced documents
- `<name>` (required) — Exact object name from the corresponding list command

Example: `ace project variant Board.PrjPcb "No LCD"`

Returns: `variant object`

Notes: Name, unique id or 1-based index.

### `ace project outputs`
_native/read_ — Output groups stored in the project itself (netlists, reports, etc.)

- `<project>` (required) — Project (.PrjPcb) path; resolves all referenced documents

Example: `ace project outputs Board.PrjPcb`

Returns: `{count, items:[{group, index, type, name, document, variant, default}]}`

Notes: Fabrication outputs usually live in .OutJob files instead: `ace outjob outputs`.

### `ace project settings`
_native/read_ — Raw INI sections (every setting Altium stores: ERC matrix, comparator, annotate...)

- `<project>` (required) — Project (.PrjPcb) path; resolves all referenced documents
- `--section` — Only this section (e.g. `Design`, `Electrical Rules Check`)
- `--names` — Only list section names

Example: `ace project settings Board.PrjPcb --section Design`

Returns: `{count, items:[{section, entries:{...}}]}`

### `ace project set-parameter`
_native/write_ — Set (or add) a project parameter

- `<project>` (required) — Project (.PrjPcb) path; resolves all referenced documents
- `<name>` (required) — Exact object name from the corresponding list command
- `<value>` (required) — New field/parameter value; include units for lengths (e.g. 0.2mm)

Example: `ace project set-parameter Board.PrjPcb Revision B`

Returns: `{changed, before, after, backup}`

### `ace project remove-parameter`
_native/write_ — Remove a project parameter

- `<project>` (required) — Project (.PrjPcb) path; resolves all referenced documents
- `<name>` (required) — Exact object name from the corresponding list command

Example: `ace project remove-parameter Board.PrjPcb Revision`

Returns: `{changed, backup}`

### `ace project set-option`
_native/write_ — Set any raw project setting `[section] key=value` (escape hatch)

- `<project>` (required) — Project (.PrjPcb) path; resolves all referenced documents
- `<section>` (required) — INI section name from settings
- `<key>` (required) — Altium property key
- `<value>` (required) — New field/parameter value; include units for lengths (e.g. 0.2mm)

Example: `ace project set-option Board.PrjPcb Design OutputPath Outputs`

Returns: `{changed, before, after, backup}`

Notes: Raw edit; prefer a dedicated command when one exists.

### `ace project add-document`
_native/write_ — Add an existing document to the project

- `<project>` (required) — Project (.PrjPcb) path; resolves all referenced documents
- `<document>` (required) — document for this operation; see the example and notes

Example: `ace project add-document Board.PrjPcb Power.SchDoc`

Returns: `{added:{index, rel}, backup}`

### `ace project remove-document`
_native/write_ — Remove a document from the project (the file itself is kept)

- `<project>` (required) — Project (.PrjPcb) path; resolves all referenced documents
- `<document>` (required) — document for this operation; see the example and notes

Example: `ace project remove-document Board.PrjPcb Old.SchDoc`

Returns: `{removed:{index, rel}, backup}`

Notes: Matches by file name or relative path.

### `ace project add-variant`
_native/write_ — Add an empty assembly variant

- `<project>` (required) — Project (.PrjPcb) path; resolves all referenced documents
- `<name>` (required) — Exact object name from the corresponding list command
- `--allow-fabrication` — Allow fabrication outputs for this variant

Example: `ace project add-variant Board.PrjPcb Lite`

Returns: `{changed, variant:{index, name, unique_id, unique_id_note?}, backup, verified, verification:{candidate_reparsed,written,read_back}}`

Notes: Then mark parts with `project set-variation` / `project set-variant-param`; `ace bom --variant` applies it. A dry-run returns unique_id:null with unique_id_note because IDs are assigned on apply; verification separates candidate reparse from disk read-back.

### `ace project remove-variant`
_native/write_ — Remove an assembly variant

- `<project>` (required) — Project (.PrjPcb) path; resolves all referenced documents
- `<variant>` (required) — Assembly variant name, unique ID or 1-based index

Example: `ace project remove-variant Board.PrjPcb Lite`

Returns: `{changed, removed, backup, verified}`

### `ace project set-variation`
_native/write_ — Set how a variant treats one part: not fitted (default), fitted, or an alternate part

- `<project>` (required) — Project (.PrjPcb) path; resolves all referenced documents
- `<variant>` (required) — Assembly variant name, unique ID or 1-based index
- `<designator>` (required) — Component designator from the component list, e.g. R5; quote 'U?'
- `--fitted` — Fitted with parameter changes only (instead of not fitted)
- `--alternate` — Replace the part with this library reference
- `--unique-id` — Unique-id path of the part (`\SHEETID\COMPID`); default: taken from the PCB or schematic

Example: `ace project set-variation Board.PrjPcb Lite R12`

Returns: `{changed, variant, designator, before:variation|null, after:variation, raw:{before,after}, unique_id_source, backup, verified, verification:{candidate_reparsed,written,read_back}}`

Notes: Default not fitted; --fitted or --alternate <LibRef>. The unique-id path comes from the PCB component link (SOURCEUNIQUEID) or the schematic part.

### `ace project set-variant-param`
_native/write_ — Set a parameter value of one part in a variant (e.g. Comment=22k)

- `<project>` (required) — Project (.PrjPcb) path; resolves all referenced documents
- `<variant>` (required) — Assembly variant name, unique ID or 1-based index
- `<designator>` (required) — Component designator from the component list, e.g. R5; quote 'U?'
- `<name>` (required) — Exact object name from the corresponding list command
- `<value>` (required) — New field/parameter value; include units for lengths (e.g. 0.2mm)

Example: `ace project set-variant-param Board.PrjPcb Lite R1 Comment 22k`

Returns: `{changed, variant, designator, name, before, after, backup, verified}`

### `ace project clear-variation`
_native/write_ — Remove every variation (fitting and parameters) of one part from a variant

- `<project>` (required) — Project (.PrjPcb) path; resolves all referenced documents
- `<variant>` (required) — Assembly variant name, unique ID or 1-based index
- `<designator>` (required) — Component designator from the component list, e.g. R5; quote 'U?'

Example: `ace project clear-variation Board.PrjPcb Lite R12`

Returns: `{changed, removed, backup, verified}`

Notes: removed counts both component variations and parameter variations.

### `ace project find`
_native/read_ — Find .PrjPcb projects under a directory

- `<dir>` (default `.`) — Directory path
- `--depth` (default `6`) — Maximum directory depth
- `--include-hidden` — Include macOS resource forks and hidden directories

Example: `ace project find ~/designs`

Returns: `{count, items:[{path, documents}]}`


## outjob

Output job files (.OutJob): inspect and edit outputs and medium settings

### `ace outjob info`
_native/read_ — Summary: group settings, mediums and outputs

- `<outjob>` (required) — Output job (.OutJob) path

Example: `ace outjob info Fab.OutJob`

Returns: `{group:{...}, mediums:[...], outputs:N, by_type:{...}}`

### `ace outjob outputs`
_native/read_ — Outputs with type, category, source document and which mediums they run in

- `<outjob>` (required) — Output job (.OutJob) path

Example: `ace outjob outputs Fab.OutJob --where output_type~gerber`

Returns: `{count, items:[{index, name, output_type, category, document, variant, enabled_for:[media], config_items}]}`

### `ace outjob output`
_native/read_ — One output with its decoded configuration records

- `<outjob>` (required) — Output job (.OutJob) path
- `<output>` (required) — output for this operation; see the example and notes

Example: `ace outjob output Fab.OutJob "Gerber X2"`

Returns: `{output:{...}, config:[{Record, ...}]}`

### `ace outjob mediums`
_native/read_ — Output mediums (folders, PDF publishers, printers) with their settings

- `<outjob>` (required) — Output job (.OutJob) path

Example: `ace outjob mediums Fab.OutJob`

Returns: `{count, items:[{index, name, kind, settings}]}`

### `ace outjob enable`
_native/write_ — Enable or disable an output for a medium

- `<outjob>` (required) — Output job (.OutJob) path
- `<output>` (required) — Output index, name or type (a type selects every output of that type)
- `--medium` (required) — Medium name or index
- `--off` — Disable instead of enable

Example: `ace outjob enable Fab.OutJob "NC Drill" --medium "Gerber folder"`

Returns: `{changed:[{output, medium, enabled}], backup}`

### `ace outjob set-document`
_native/write_ — Point outputs at another source document (e.g. after renaming the PcbDoc)

- `<outjob>` (required) — Output job (.OutJob) path
- `<output>` (required) — Output index, name or type; `all` for every output
- `<document>` (required) — document for this operation; see the example and notes

Example: `ace outjob set-document Fab.OutJob all Board.PcbDoc`

Returns: `{changed:[{output, before, after}], backup}`

### `ace outjob set-variant`
_native/write_ — Set the variant used by the whole job

- `<outjob>` (required) — Output job (.OutJob) path
- `<variant>` (required) — Assembly variant name, unique ID or 1-based index

Example: `ace outjob set-variant Fab.OutJob "No LCD"`

Returns: `{before, after, backup}`

Notes: Use `[No Variations]` to clear.


## sch

Schematic documents (.SchDoc)

### `ace sch info`
_native/read_ — Summary: sheet size/style, document parameters, record counts, components, warnings

- `<file>` (required) — Input Altium document path; quote spaces

Example: `ace sch info Power.SchDoc`

Returns: `{file, header, sheet:{size,width,height,orientation,fonts...}, parameters:{name:value}, record_counts:{type:n}, components, nets, sheet_symbols, ports, power_ports, net_labels, storage_files, warnings_count}`

Notes: Start here for a sheet. `parameters` are the title-block fields.

### `ace sch components`
_native/read_ — Placed components (one row per sub-part instance) with footprint and parameters

- `<file>` (required) — Input Altium document path; quote spaces

Example: `ace sch components Power.SchDoc --where footprint~0603`

Returns: `{count, items:[{index, designator, part?, display_designator, lib_ref, comment, footprint?, library?, description, kind, parameters:{name:value}, location, orientation, mirrored, unique_id, pins}]}`

Notes: One row per placed sub-part (U1A, U1B share designator U1). `index` is the component record.

### `ace sch component`
_native/read_ — One component in full: pins with nets, every model (footprint/sim/SI/3D), parameters

- `<file>` (required) — Input Altium document path; quote spaces
- `<designator>` (required) — Designator (`U1` = every sub-part, `U1A` = one sub-part)
- `--project` — Name pin nets project-wide using this .PrjPcb (default: sheet scope)

Example: `ace sch component Power.SchDoc U3 --project Board.PrjPcb`

Returns: `{count, items:[component with pins:[{designator,name,electrical,hot_end,net...}], models:[{kind,name,current,datafiles,pin_map}], parameters:[{name,value,resolved?,hidden}]}]}`

Notes: `U1` returns every sub-part instance, `U1A` only that one. Pin nets are sheet-scope names unless --project is given (then project-wide names; pins on no net have none).

### `ace sch pins`
_native/read_ — Pins with electrical type, hot-end location and net

- `<file>` (required) — Input Altium document path; quote spaces
- `--component` — Only pins of this component
- `--project` — Name pin nets project-wide using this .PrjPcb (default: sheet scope)
- `--unconnected` — Only pins without a net in this scope (native approximation, not ERC)

Example: `ace sch pins Power.SchDoc --component U3 --project Board.PrjPcb`

Returns: `{count, items:[{component, index, pin, designator, connected, name, electrical, hot_end, location, orientation, length, hidden, net}]}`

Notes: net = sheet-scope name, or the project-wide name with --project. Native approximation of Altium connectivity (rules in altium-doc sch/connect.rs); `ace designer` compile is the authority. pin is the pin number (designator is a deprecated alias); net is always present (null if unconnected), connected is boolean. --unconnected filters within the selected scope.

### `ace sch nets`
_native/read_ — Sheet-local nets (this sheet only; use `sch project-nets` across a project)

- `<file>` (required) — Input Altium document path; quote spaces

Example: `ace sch nets Power.SchDoc --where name=GND`

Returns: `{count, single_node, dangling_wires, items:[{name, auto_named?, name_source, nodes:[{designator,pin,pin_name,pin_record}], net_labels, power_ports, ports, sheet_entries, other_names, wires}]}`

Notes: Sheet scope with Altium default naming options. Cross-sheet joins: `sch project-nets`. Native approximation; `ace designer` compile is the authority.

### `ace sch project-nets`
_native/read_ — Project-wide nets across all schematics, following the project's net identifier scope

- `<project>` (required) — Project (.PrjPcb) path; resolves all referenced documents

Example: `ace sch project-nets Board.PrjPcb --where name~^Net`

Returns: `{scope, configured_scope, count, single_node, items:[{name, auto_named?, name_source, nodes:[{designator,pin,pin_name}], sheets, other_names}]}`

Notes: Follows [Design] HierarchyMode and naming options. Node pins are footprint pad numbers (pin maps applied), pin_name the schematic pin name. Unnamed single-pin nets are left out unless NetlistSinglePinNets. Matches the sample PCBs' net names; still a native approximation, `ace designer` compile is the authority.

### `ace sch net-labels`
_native/read_ — Net labels

- `<file>` (required) — Input Altium document path; quote spaces

Example: `ace sch net-labels Power.SchDoc`

Returns: `{count, items:[{index, name, location, orientation, on_bus?, net}]}`

Notes: on_bus=true: the label names a bus, not a net.

### `ace sch power-ports`
_native/read_ — Power ports (net name and symbol style)

- `<file>` (required) — Input Altium document path; quote spaces

Example: `ace sch power-ports Power.SchDoc`

Returns: `{count, items:[{index, name, style, show_net_name, location, orientation, net}]}`

### `ace sch ports`
_native/read_ — Ports (name, IO type, harness type, net)

- `<file>` (required) — Input Altium document path; quote spaces

Example: `ace sch ports Sensor.SchDoc`

Returns: `{count, items:[{index, name, io_type, style, location, width, ends, harness_type?, net}]}`

### `ace sch sheet-symbols`
_native/read_ — Sheet symbols (child file, sheet name) with their entries

- `<file>` (required) — Input Altium document path; quote spaces

Example: `ace sch sheet-symbols Top.SchDoc`

Returns: `{count, items:[{index, sheet_name, file_name, location, width, height, repeat?, entries:[{index, name, io_type, side, location, net}]}]}`

Notes: file_name is the child sheet; `sch hierarchy` resolves the tree.

### `ace sch wires`
_native/read_ — Wires with their vertices and net

- `<file>` (required) — Input Altium document path; quote spaces

Example: `ace sch wires Power.SchDoc`

Returns: `{count, items:[{index, points, net}]}`

### `ace sch buses`
_native/read_ — Buses with their labels and bus entries

- `<file>` (required) — Input Altium document path; quote spaces

Example: `ace sch buses Top.SchDoc`

Returns: `{count, entries:[...], items:[{index, points, labels, entries}]}`

Notes: Bus names like D[0..7] are expanded to member nets D0..D7 across sheets.

### `ace sch junctions`
_native/read_ — Junctions

- `<file>` (required) — Input Altium document path; quote spaces

Example: `ace sch junctions Power.SchDoc`

Returns: `{count, items:[{index, location, net}]}`

### `ace sch no-erc`
_native/read_ — No-ERC markers

- `<file>` (required) — Input Altium document path; quote spaces

Example: `ace sch no-erc Power.SchDoc`

Returns: `{count, items:[{index, location, active, suppress_all, net?}]}`

### `ace sch texts`
_native/read_ — Free text: labels, notes, text frames, hyperlinks

- `<file>` (required) — Input Altium document path; quote spaces

Example: `ace sch texts Power.SchDoc --where kind=note`

Returns: `{count, items:[{index, kind, text, location, owner?}]}`

Notes: kind: label | text_frame | note | hyperlink. owner = component designator for symbol text.

### `ace sch parameters`
_native/read_ — Document parameters (title block fields)

- `<file>` (required) — Input Altium document path; quote spaces

Example: `ace sch parameters Power.SchDoc`

Returns: `{count, items:[{index, name, value, resolved?, hidden}]}`

Notes: value `*` = not set. Change with `sch set-sheet-param`.

### `ace sch directives`
_native/read_ — Directives: parameter sets (net class / diff pair / rules), no-ERC, compile masks, blankets

- `<file>` (required) — Input Altium document path; quote spaces

Example: `ace sch directives Power.SchDoc`

Returns: `{count, items:[{index, kind, name?, location?, parameters:{...}, net?}]}`

Notes: kind: parameter_set | no_erc | compile_mask | blanket.

### `ace sch images`
_native/export_ — Images, with the embedded files from the Storage stream

- `<file>` (required) — Input Altium document path; quote spaces
- `--extract` — Write the embedded files into this directory

Example: `ace sch images Logo.SchDoc --extract ./img`

Returns: `{count, storage:[{index, name, format, size}], extracted?:[paths], items:[{index, file_name, embedded, storage_name?}]}`

Notes: Writes files only with --extract (honours --dry-run).

### `ace sch records`
_native/read_ — Every record with its type name, owner and raw properties

- `<file>` (required) — Input Altium document path; quote spaces
- `--type` — Only this record type (name such as `wire` or number such as `27`)

Example: `ace sch records Power.SchDoc --type wire`

Returns: `{count, items:[{index, stream, record, type, owner?, owner_designator?, props}]}`

Notes: Escape hatch: every record, raw. --type accepts a name or number.

### `ace sch find`
_native/read_ — Search every text property (case-insensitive substring)

- `<file>` (required) — Input Altium document path; quote spaces
- `<needle>` (required) — Text to look for

Example: `ace sch find Power.SchDoc 100nF`

Returns: `{count, items:[{index, type, owner?, key, value, stream}]}`

### `ace sch locate`
_native/read_ — Locate components across all project sheets; use each sheet/index in follow-up edits

- `<project>` (required) — Project containing the schematic sheets
- `<designators>` (required, repeatable) — Component designators, comma-separated or separate arguments

Example: `ace sch locate Board.PrjPcb R20,BF1`

Returns: `{count, items:[{designator,sheet,index,unique_id,comment,footprint}]}`

Notes: Searches every existing schematic; warnings identify missing project files. Use the returned sheet path with sch set-param. Multiple rows can represent sub-parts or repeated designators.

### `ace sch hierarchy`
_native/read_ — Sheet tree of a project from its sheet symbols (roots, repeated sheets, scope)

- `<project>` (required) — Project (.PrjPcb) path; resolves all referenced documents

Example: `ace sch hierarchy Board.PrjPcb`

Returns: `{configured_scope, scope, roots:[{file, sheet_name?, exists, in_project, repeated?, repeat?, components, children:[...]}], repeated_sheets, warnings}`

Notes: Roots = project sheets no sheet symbol places.

### `ace sch set-param`
_native/write_ — Set (or create) a component parameter

- `<file>` (required) — Input Altium document path; quote spaces
- `<designator>` (required) — Component designator from the component list, e.g. R5; quote 'U?'
- `<name>` (required) — Exact object name from the corresponding list command
- `<value>` (required) — New field/parameter value; include units for lengths (e.g. 0.2mm)
- `--literal` — Explicitly replace a special-string reference such as =Value with literal text

Example: `ace sch set-param Power.SchDoc R5 MPN RC0603FR-0710KL`

Returns: `{changed, changes:[{index, action, designator, key, before, after}], backup, verified}`

Notes: Creates a hidden parameter (copying a sibling's style) when missing. Applies to every sub-part instance. Replacing a =Param reference needs --literal; --dry-run warns and marks special_string.

### `ace sch remove-param`
_native/write_ — Remove a component parameter

- `<file>` (required) — Input Altium document path; quote spaces
- `<designator>` (required) — Component designator from the component list, e.g. R5; quote 'U?'
- `<name>` (required) — Exact object name from the corresponding list command

Example: `ace sch remove-param Power.SchDoc R5 MPN`

Returns: `{changed, changes, renumbered, backup, verified}`

Notes: Later records' OWNERINDEX are renumbered (reported).

### `ace sch set-comment`
_native/write_ — Set the Comment parameter

- `<file>` (required) — Input Altium document path; quote spaces
- `<designator>` (required) — Component designator from the component list, e.g. R5; quote 'U?'
- `<value>` (required) — New field/parameter value; include units for lengths (e.g. 0.2mm)
- `--literal` — Explicitly replace a special-string reference such as =Value with literal text

Example: `ace sch set-comment Power.SchDoc R5 10k`

Returns: `{changed, changes, backup, verified}`

Notes: Special strings such as =Value are protected: set the Value parameter instead, or explicitly use --literal. Dry runs report special_string without writing.

### `ace sch set-footprint`
_native/write_ — Rename the current footprint model

- `<file>` (required) — Input Altium document path; quote spaces
- `<designator>` (required) — Component designator from the component list, e.g. R5; quote 'U?'
- `<footprint>` (required) — Footprint name from `ace pcblib list`

Example: `ace sch set-footprint Power.SchDoc R5 0603`

Returns: `{changed, changes, backup, verified}`

Notes: Renames the current footprint model; refuses (UNSAFE) when there is none.

### `ace sch set-designator`
_native/write_ — Change a designator

- `<file>` (required) — Input Altium document path; quote spaces
- `<designator>` (required) — Component designator from the component list, e.g. R5; quote 'U?'
- `<new>` (required) — New object name/designator

Example: `ace sch set-designator Power.SchDoc R5 R50`

Returns: `{changed, changes, backup, verified}`

Notes: Refuses when the new designator is already used on the sheet.

### `ace sch set-sheet-param`
_native/write_ — Set (or create) a document parameter, e.g. `Revision`

- `<file>` (required) — Input Altium document path; quote spaces
- `<name>` (required) — Exact object name from the corresponding list command
- `<value>` (required) — New field/parameter value; include units for lengths (e.g. 0.2mm)

Example: `ace sch set-sheet-param Power.SchDoc Revision B`

Returns: `{changed, changes, backup, verified}`


## schlib

Schematic symbol libraries (.SchLib)

### `ace schlib info`
_native/read_ — Summary: symbol count, pins, fonts, embedded files

- `<file>` (required) — Input Altium document path; quote spaces

Example: `ace schlib info Symbols.SchLib`

Returns: `{file, header, symbols, pins, models, fonts:[...], storage_files, warnings_count}`

### `ace schlib list`
_native/read_ — Symbols with part count, pin count, description and footprint

- `<file>` (required) — Input Altium document path; quote spaces

Example: `ace schlib list Symbols.SchLib --where footprint~SOIC`

Returns: `{count, items:[{name, storage, description, part_count, pins, designator?, comment?, footprint?, kind}]}`

Notes: `name` is the reference for `schlib show/pins`.

### `ace schlib show`
_native/read_ — One symbol in full: pins, parameters, models (footprint links), drawing primitives

- `<file>` (required) — Input Altium document path; quote spaces
- `<symbol>` (required) — Symbol name from `ace schlib list`

Example: `ace schlib show Symbols.SchLib LM317`

Returns: `{name, storage, description, part_count, pins:[...], parameters:[...], models:[{kind,name,current,datafiles,pin_map}], primitives:[{index,type,part_id,location?,corner?,points?...}], streams}`

Notes: Pin and primitive coordinates are symbol-local.

### `ace schlib pins`
_native/read_ — Pins of one symbol (binary pin records decoded)

- `<file>` (required) — Input Altium document path; quote spaces
- `<symbol>` (required) — Symbol name from `ace schlib list`

Example: `ace schlib pins Symbols.SchLib LM317`

Returns: `{count, items:[{index, designator, name, electrical, part_id, location, hot_end, length, orientation, hidden, extra?, binary}]}`

Notes: extra = per-pin side streams (PinPackageLength, PinSymbolLineWidth...).

### `ace schlib find`
_native/read_ — Search every text property of every symbol (case-insensitive substring)

- `<file>` (required) — Input Altium document path; quote spaces
- `<needle>` (required) — Text to look for

Example: `ace schlib find Symbols.SchLib SOIC-8`

Returns: `{count, items:[{index, type, owner, key, value, stream}]}`

Notes: owner/stream = symbol name.

### `ace schlib set-param`
_native/write_ — Set (or add) a parameter of one symbol, e.g. MPN or Comment

- `<file>` (required) — Input Altium document path; quote spaces
- `<symbol>` (required) — Symbol name from `ace schlib list`
- `<name>` (required) — Exact object name from the corresponding list command
- `<value>` (required) — New field/parameter value; include units for lengths (e.g. 0.2mm)

Example: `ace schlib set-param Symbols.SchLib LM317 MPN LM317DCYR`

Returns: `{changed, file, changes:[{symbol, stream, record, action, key, before, after}], backup, verified}`

Notes: New parameters are hidden and copy a sibling parameter's position/font. Placed instances on sheets keep their own copy: update them with `sch set-param` (or Altium's Update From Libraries).

### `ace schlib set-description`
_native/write_ — Set a symbol's description

- `<file>` (required) — Input Altium document path; quote spaces
- `<symbol>` (required) — Symbol name from `ace schlib list`
- `<description>` (required) — description for this operation; see the example and notes

Example: `ace schlib set-description Symbols.SchLib LM317 "1.5A adjustable regulator"`

Returns: `{changed, file, changes, backup, verified}`

Notes: Updates the component record and the library header entry.


## pcb

PCB documents (.PcbDoc)

### `ace pcb info`
_native/read_ — Board summary: size, outline, layers, stackup thickness, object counts, file version

- `<file>` (required) — Input Altium document path; quote spaces

Example: `ace pcb info Board.PcbDoc`

Returns: `{file, format, board:{origin, width, height, bbox, area, outline_vertices}, copper_layers, stackup:{source, total_thickness}, components:{total, top, bottom}, nets, counts:{pads, vias, tracks, ...}, streams:{Pads6:N, ...}}`

Notes: Counts come from each storage's Header stream; every other command decodes the records.

### `ace pcb layers`
_native/read_ — Every enabled or used layer: id, Altium name, user name, kind, copper thickness, objects

- `<file>` (required) — Input Altium document path; quote spaces
- `--all` — Include disabled and display-only layers too

Example: `ace pcb layers Board.PcbDoc --where kind=mechanical`

Returns: `{count, items:[{layer, id, legacy_id, user_name, kind, enabled, used_by_prims, copper_thickness, mech_kind, stack_index, objects}]}`

Notes: layer is the Altium name used everywhere else (TopLayer, MidLayer1, Mechanical13, TopOverlay, MultiLayer); --all adds disabled/system layers.

### `ace pcb stackup`
_native/read_ — Physical layer stack top to bottom: thickness, material, dielectric constant, total

- `<file>` (required) — Input Altium document path; quote spaces

Example: `ace pcb stackup Board.PcbDoc --units mil`

Returns: `{source, total_thickness, board_thickness, count, items:[{index, name, layer, kind, thickness, material, dielectric_constant, loss_tangent, dielectric_type, component_side}]}`

### `ace pcb components`
_native/read_ — Placed components with footprint, comment, side, position, rotation, schematic link

- `<file>` (required) — Input Altium document path; quote spaces
- `--side` — Only `top` or `bottom`

Example: `ace pcb components Board.PcbDoc --side bottom --fields designator,footprint,x,y`

Returns: `{count, items:[{index, designator, footprint, comment, description, side, x, y, rotation, locked, height, kind, source_unique_id, sch_unique_id, source_library, footprint_library, parameters, pads}]}`

Notes: sch_unique_id matches the schematic component UNIQUEID (sch↔pcb link).

### `ace pcb component`
_native/read_ — One component in full: its pads (with nets), 3D bodies, texts and parameters

- `<file>` (required) — Input Altium document path; quote spaces
- `<designator>` (required) — Component designator from the component list, e.g. R5; quote 'U?'

Example: `ace pcb component Board.PcbDoc U1`

Returns: `{component:{...}, pads:[{number, net, shape, size, hole_size, ...}], bodies:[...], texts:[...], primitives:{tracks, arcs, ...}}`

Notes: Designator match is case-insensitive; `#N` selects by index.

### `ace pcb nets`
_native/read_ — Nets with pad/node counts and routed length

- `<file>` (required) — Input Altium document path; quote spaces

Example: `ace pcb nets Board.PcbDoc --where name~^GND`

Returns: `{count, items:[{index, name, pads, nodes, tracks, vias, arcs, routed_length, unique_id}]}`

### `ace pcb net`
_native/read_ — One net: pads/nodes, track/via/arc counts, routed length, layers, classes, pours

- `<file>` (required) — Input Altium document path; quote spaces
- `<name>` (required) — Exact object name from the corresponding list command

Example: `ace pcb net Board.PcbDoc GND`

Returns: `{index, name, node_count, pads:[{node, component, number, layer, x, y}], tracks, arcs, vias, fills, regions, polygons, routed_length, layers, classes}`

### `ace pcb classes`
_native/read_ — Net, component, pad, layer, diff-pair, polygon and xSignal classes with members

- `<file>` (required) — Input Altium document path; quote spaces
- `--kind` — Only this kind (net, component, pad, layer, diff_pair, polygon, from_to, xsignal)

Example: `ace pcb classes Board.PcbDoc --kind net`

Returns: `{count, items:[{index, name, kind, kind_code, members, superclass, auto_generated, stream}]}`

### `ace pcb rules`
_native/read_ — Every design rule: kind, name, enabled, priority, scopes and decoded settings

- `<file>` (required) — Input Altium document path; quote spaces
- `--kind` — Only this rule kind (Clearance, Width, RoutingVias, HoleSize, ...)

Example: `ace pcb rules Board.PcbDoc --kind Clearance`

Returns: `{count, items:[{index, name, kind, kind_code, enabled, priority, scope1, scope2, values:{gap|minlimit|...}}]}`

Notes: Lengths in values are converted to --units; keys are Altium's, lower-cased.

### `ace pcb rule`
_native/read_ — One design rule by name

- `<file>` (required) — Input Altium document path; quote spaces
- `<name>` (required) — Exact object name from the corresponding list command

Example: `ace pcb rule Board.PcbDoc Clearance`

Returns: `rule object`

Notes: Exact name, then case-insensitive.

### `ace pcb pads`
_native/read_ — Pads: number, shape, size, hole, plating, layer, net, rotation, position, mask expansion

- `<file>` (required) — Input Altium document path; quote spaces
- `--component` — Component designator from the component list, e.g. R5; quote 'U?'
- `--net` — Net name from a nets/netlist command
- `--layer` — layer for this operation; see the example and notes

Example: `ace pcb pads Board.PcbDoc --component U1 --fields number,net,shape,size`

Returns: `{count, items:[{index, number, component, net, layer, x, y, rotation, shape, size:{x,y}, mode, hole_size, hole_shape, slot_length, plated, paste_mask_expansion, solder_mask_expansion, ...}]}`

### `ace pcb vias`
_native/read_ — Vias: position, diameter, hole, layer span, tenting, net

- `<file>` (required) — Input Altium document path; quote spaces
- `--net` — Net name from a nets/netlist command

Example: `ace pcb vias Board.PcbDoc --net GND`

Returns: `{count, items:[{index, net, x, y, diameter, hole_size, start_layer, end_layer, mode, tented_top, tented_bottom, solder_mask_expansion?}]}`

### `ace pcb tracks`
_native/read_ — Track segments: endpoints, width, length, layer, net

- `<file>` (required) — Input Altium document path; quote spaces
- `--net` — Net name from a nets/netlist command
- `--layer` — layer for this operation; see the example and notes

Example: `ace pcb tracks Board.PcbDoc --net CLK --layer TopLayer`

Returns: `{count, total_length, items:[{index, layer, net, component, start, end, width, length, polygon?, keepout?}]}`

Notes: polygon set = part of a hatched pour, not routing.

### `ace pcb arcs`
_native/read_ — Arcs: centre, radius, angles, width, length, layer, net

- `<file>` (required) — Input Altium document path; quote spaces
- `--net` — Net name from a nets/netlist command
- `--layer` — layer for this operation; see the example and notes

Example: `ace pcb arcs Board.PcbDoc --layer TopOverlay`

Returns: `{count, total_length, items:[{index, layer, net, center, radius, start_angle, end_angle, width, start, end, length}]}`

### `ace pcb fills`
_native/read_ — Rectangular fills

- `<file>` (required) — Input Altium document path; quote spaces
- `--layer` — layer for this operation; see the example and notes

Example: `ace pcb fills Board.PcbDoc`

Returns: `{count, items:[{index, layer, net, corner1, corner2, rotation, keepout?}]}`

### `ace pcb regions`
_native/read_ — Regions: copper regions, poured polygon pieces, cutouts, keepouts, board cutouts, shape-based regions and board regions

- `<file>` (required) — Input Altium document path; quote spaces
- `--layer` — layer for this operation; see the example and notes
- `--kind` — Only this kind (copper, polygon_cutout, board_cutout, dashed_outline, cavity_definition)
- `--source` — Only this source: shape_based_regions (default list, arc-exact), board_regions, or regions (the straight-edge Regions6 copies of the shape-based regions)
- `--brief` — Omit outline vertices (counts, area and bbox only)

Example: `ace pcb regions Board.PcbDoc --kind board_cutout`

Returns: `{count, items:[{index, source, layer, net, component, kind, polygon, keepout, keepout_restrictions, outline:[{x,y,arc?}], holes, area, bbox}]}`

Notes: Poured polygon copper is regions with polygon set; --brief drops vertices.

### `ace pcb polygons`
_native/read_ — Polygon pours: net, layer, hatch and pour settings, outline

- `<file>` (required) — Input Altium document path; quote spaces

Example: `ace pcb polygons Board.PcbDoc`

Returns: `{count, items:[{index, name, layer, net, polygon_type, hatch_style, pour_over, remove_dead, grid_size, track_width, pour_index, shelved, outline:{vertices, area, bbox}, poured_regions}]}`

### `ace pcb texts`
_native/read_ — Free and component texts with resolved text, font, height, position, rotation

- `<file>` (required) — Input Altium document path; quote spaces
- `--layer` — layer for this operation; see the example and notes

Example: `ace pcb texts Board.PcbDoc --layer TopOverlay`

Returns: `{count, items:[{index, layer, component, text, x, y, height, rotation, mirrored, font_type, font_name, is_designator, is_comment, frame?}]}`

Notes: Text comes from WideStrings6 when present (Unicode), else the legacy 8-bit copy.

### `ace pcb dimensions`
_native/read_ — Dimension objects

- `<file>` (required) — Input Altium document path; quote spaces

Example: `ace pcb dimensions Board.PcbDoc`

Returns: `{count, items:[{index, layer, kind, start, references, text_points, height, unit, precision, ...}]}`

### `ace pcb bodies`
_native/read_ — 3D component bodies: component, model, layer, heights, placement

- `<file>` (required) — Input Altium document path; quote spaces
- `--component` — Component designator from the component list, e.g. R5; quote 'U?'

Example: `ace pcb bodies Board.PcbDoc --component U1`

Returns: `{count, items:[{index, component, layer, model_id, model_name, model_embedded, standoff_height, overall_height, height, model_rotation_z, outline}]}`

### `ace pcb models`
_native/export_ — Embedded 3D models; `--extract DIR` writes the decompressed STEP files

- `<file>` (required) — Input Altium document path; quote spaces
- `--extract` — Write every embedded model into this directory

Example: `ace pcb models Board.PcbDoc --extract ./step`

Returns: `{count, items:[{index, id, name, embedded, stream, compressed_size, used_by, extracted?:{path, size}}], extract_dir?}`

Notes: Without --extract nothing is written; honours --dry-run.

### `ace pcb holes`
_native/read_ — Drill table from pads and vias: size, shape, plating, count

- `<file>` (required) — Input Altium document path; quote spaces

Example: `ace pcb holes Board.PcbDoc --units mm`

Returns: `{count, total, plated, non_plated, items:[{diameter, shape, slot_length, plated, count, pads, vias, spans}]}`

### `ace pcb diffpairs`
_native/read_ — Differential pairs (DifferentialPairs6 records and diff-pair classes)

- `<file>` (required) — Input Altium document path; quote spaces

Example: `ace pcb diffpairs Board.PcbDoc`

Returns: `{count, items:[{index, name, properties}], classes:[{name, members}]}`

Notes: DifferentialPairs6 records are shown as raw properties (no sample has any).

### `ace pcb rooms`
_native/read_ — Rooms (RoomDefinition rules) with their outlines

- `<file>` (required) — Input Altium document path; quote spaces

Example: `ace pcb rooms Board.PcbDoc`

Returns: `{count, items:[{name, enabled, layer, scope, confinement, lock_components, outline}]}`

### `ace pcb outline`
_native/read_ — Board outline vertices (with arc edges), area, perimeter, bounding box

- `<file>` (required) — Input Altium document path; quote spaces

Example: `ace pcb outline Board.PcbDoc --units mm`

Returns: `{vertices:[{x,y,arc?:{center,radius,start_angle,end_angle}}], vertex_count, arcs, area, perimeter, bbox, board_cutouts}`

Notes: An arc on vertex i shapes the edge from vertex i to i+1.

### `ace pcb stats`
_native/read_ — Object counts per type per layer

- `<file>` (required) — Input Altium document path; quote spaces

Example: `ace pcb stats Board.PcbDoc`

Returns: `{count, items:[{layer, kind, pads, tracks, arcs, fills, regions, texts, polygons, bodies, dimensions, vias, total}], totals}`

### `ace pcb lengths`
_native/read_ — Routed length per net from tracks and arcs

- `<file>` (required) — Input Altium document path; quote spaces
- `--class` — Only nets of this net class

Example: `ace pcb lengths Board.PcbDoc --class DDR --where routed_length!=0`

Returns: `{count, items:[{net, routed_length, track_length, arc_length, tracks, arcs, vias, layers, by_layer}]}`

Notes: Excludes polygon pour tracks and keepouts; via barrels are not counted.

### `ace pcb set-rule`
_native/write_ — Set one field of a design rule (gap, minlimit, maxlimit, enabled, priority, scope1...)

- `<file>` (required) — Input Altium document path; quote spaces
- `<rule>` (required) — Rule name (see `pcb rules`)
- `<key>` (required) — Field as `pcb rules` shows it under values (gap, minlimit...) or enabled, priority, scope1, scope2, comment, name
- `<value>` (required) — New value; lengths need a unit (0.2mm, 8mil), booleans true/false

Example: `ace pcb set-rule Board.PcbDoc Clearance gap 0.2mm`

Returns: `{changed, file, changes:[{stream, record, action, target, key, before, after}], backup, verified}`

Notes: Changes retain raw before/after strings and add typed before_value/after_value with unit for lengths. verification distinguishes candidate reparse from disk read-back. Only fields the rule already has. Lengths are written in mil like Altium does; priorities stay unique per kind. New rules and rule kinds need `ace designer`.

### `ace pcb set-component`
_native/write_ — Set a non-geometric component field: description, kind, locked, designator_visible, comment_visible, height, footprint_description

- `<file>` (required) — Input Altium document path; quote spaces
- `<designator>` (required) — Component designator from the component list, e.g. R5; quote 'U?'
- `<field>` (required) — Field name to change; see notes for supported fields
- `<value>` (required) — New field/parameter value; include units for lengths (e.g. 0.2mm)

Example: `ace pcb set-component Board.PcbDoc U1 kind "Standard (No BOM)"`

Returns: `{changed, file, changes, backup, verified}`

Notes: Position, rotation, side, footprint and the designator/comment texts are geometry/binary records: use `ace designer` (or change the schematic and update the PCB).

### `ace pcb set-param`
_native/write_ — Set (or add) a component parameter

- `<file>` (required) — Input Altium document path; quote spaces
- `<designator>` (required) — Component designator from the component list, e.g. R5; quote 'U?'
- `<name>` (required) — Exact object name from the corresponding list command
- `<value>` (required) — New field/parameter value; include units for lengths (e.g. 0.2mm)

Example: `ace pcb set-param Board.PcbDoc U1 MPN LM2596SX-5.0`

Returns: `{changed, file, changes, backup, verified}`

Notes: Parameters live in PrimitiveParameters keyed by the component's unique id.

### `ace pcb remove-param`
_native/write_ — Remove a component parameter

- `<file>` (required) — Input Altium document path; quote spaces
- `<designator>` (required) — Component designator from the component list, e.g. R5; quote 'U?'
- `<name>` (required) — Exact object name from the corresponding list command

Example: `ace pcb remove-param Board.PcbDoc U1 Notepad`

Returns: `{changed, file, changes, backup, verified}`

### `ace pcb class-add`
_native/write_ — Add members (nets, designators...) to an existing class

- `<file>` (required) — Input Altium document path; quote spaces
- `<class>` (required) — class for this operation; see the example and notes
- `<members>` (required, repeatable) — members for this operation; see the example and notes
- `--kind` — Class kind when several classes share the name (net, component, ...)

Example: `ace pcb class-add Board.PcbDoc PWR 5V 3V3`

Returns: `{changed, file, changes:[{action:add_member, target, after}], backup, verified}`

Notes: Members must exist (nets for net classes, designators for component classes).

### `ace pcb class-remove`
_native/write_ — Remove members from a class

- `<file>` (required) — Input Altium document path; quote spaces
- `<class>` (required) — class for this operation; see the example and notes
- `<members>` (required, repeatable) — members for this operation; see the example and notes
- `--kind` — kind for this operation; see the example and notes

Example: `ace pcb class-remove Board.PcbDoc PWR 3V3`

Returns: `{changed, file, changes:[{action:remove_member, target, before}], backup, verified}`

### `ace pcb add-class`
_native/write_ — Create a class (net by default) with optional initial members

- `<file>` (required) — Input Altium document path; quote spaces
- `<name>` (required) — Exact object name from the corresponding list command
- `<members>` (repeatable) — members for this operation; see the example and notes
- `--kind` (default `net`) — net, component, pad, layer, from_to, diff_pair, polygon

Example: `ace pcb add-class Board.PcbDoc DDR DQ0 DQ1 DQ2 --kind net`

Returns: `{changed, file, changes:[{action:create, record, target}], backup, verified}`

Notes: Copies the record layout of an existing user class; Classes6/Header count is updated.

### `ace pcb svg`
_native/export_ — Write an SVG preview of chosen layers

- `<file>` (required) — Input Altium document path; quote spaces
- `--out` (required) — Output file
- `--layers` (repeatable) — Layers to draw (comma-separated names); default: copper, overlays, keep-out, multi-layer and the board outline
- `--text-boxes` — Draw texts as their bounding boxes

Example: `ace pcb svg Board.PcbDoc --out board.svg --layers TopLayer,TopOverlay`

Returns: `{out, layers:[...], width, height, elements}`

Notes: Preview only (no clearances, polygons drawn from their poured regions).


## pcblib

Footprint libraries (.PcbLib)

### `ace pcblib find`
_native/read_ — Search footprint names across libraries or directories, optionally including pads

- `<pattern>` (required) — Case-insensitive substring of the footprint name, e.g. 0603
- `<paths>` (required, repeatable) — .PcbLib files or directories to search recursively
- `--pads` — Include the complete pad objects for follow-up analysis

Example: `ace pcblib find 0603 ./libraries --pads`

Returns: `{libraries_scanned,total_footprints,count,items:[{file,name,pads,pad_numbers,pad_number_values,pad_items?,...}],skipped,incomplete}`

Notes: Case-insensitive substring search across .PcbLib files; skips hidden directories and symlinks. IntLib containers and parse failures appear in skipped and warnings, never as an empty successful search. pad_numbers is the legacy count; pad_number_values lists the distinct numbers.

### `ace pcblib info`
_native/read_ — Summary of the document: footprints, models, layers, file version

- `<file>` (required) — Input Altium document path; quote spaces

Example: `ace pcblib info Footprints.PcbLib`

Returns: `{file, format, footprints, models, embedded_models, layers:[enabled layer names], stackup:{source, total_thickness}}`

### `ace pcblib list`
_native/read_ — Footprints with pad count, description, height, extent and 3D models

- `<file>` (required) — Input Altium document path; quote spaces

Example: `ace pcblib list Footprints.PcbLib --where mount=smd --fields name,pads`

Returns: `{count, items:[{index, name, pads, pad_numbers, description, height, primitives:{...}, bbox, mount, models}]}`

### `ace pcblib show`
_native/read_ — One footprint in full: pads, primitive counts, bodies, parameters, texts

- `<file>` (required) — Input Altium document path; quote spaces
- `<footprint>` (required) — Footprint name from `ace pcblib list`
- `--all` — Include every primitive (tracks, arcs, fills, regions) too

Example: `ace pcblib show Footprints.PcbLib SOIC-8`

Returns: `{footprint:{index, name, description, height, parameters, pads:N, ...}, pads:[...], bodies:[...], texts:[...], tracks?, arcs?, fills?, regions?}`

Notes: Name match is exact, then case-insensitive; `#N` selects by index.

### `ace pcblib pads`
_native/read_ — Pads of one footprint

- `<file>` (required) — Input Altium document path; quote spaces
- `<footprint>` (required) — Footprint name from `ace pcblib list`

Example: `ace pcblib pads Footprints.PcbLib R0603`

Returns: `{footprint, count, items:[{index, number, layer, x, y, rotation, shape, size, hole_size, plated, ...}]}`

Notes: Coordinates are relative to the footprint origin.

### `ace pcblib models`
_native/export_ — Embedded 3D models; `--extract DIR` writes the decompressed STEP files

- `<file>` (required) — Input Altium document path; quote spaces
- `--extract` — extract for this operation; see the example and notes

Example: `ace pcblib models Footprints.PcbLib --extract ./step`

Returns: `{count, items:[{index, id, name, embedded, stream, compressed_size, used_by, extracted?:{path, size}}], extract_dir?}`

Notes: Without --extract nothing is written; honours --dry-run.

### `ace pcblib svg`
_native/export_ — Write an SVG preview of one footprint

- `<file>` (required) — Input Altium document path; quote spaces
- `<footprint>` (required) — Footprint name from `ace pcblib list`
- `--out` (required) — out for this operation; see the example and notes
- `--layers` (repeatable) — Layers to draw (comma-separated); default: copper, overlays, keep-out, multi-layer

Example: `ace pcblib svg Footprints.PcbLib SOIC-8 --out soic8.svg`

Returns: `{out, footprint, layers, width, height, elements}`

### `ace pcblib rename`
_native/write_ — Rename a footprint: the library list, its storage and the names inside it

- `<file>` (required) — Input Altium document path; quote spaces
- `<footprint>` (required) — Footprint name from `ace pcblib list`
- `<name>` (required) — Exact object name from the corresponding list command

Example: `ace pcblib rename Footprints.PcbLib SOIC-8 SOIC-8N`

Returns: `{changed, file, changes:[{target, stream, action, key, before, after}], backup, verified, footprints}`

Notes: Also moves the footprint storage (name with / as _, cut at 31 characters) and the PATTERN parameter. Case-only renames keep the storage spelling. Other footprints keep their bytes.

### `ace pcblib set-param`
_native/write_ — Set a footprint parameter: DESCRIPTION, HEIGHT (with a unit) or any other Parameters key

- `<file>` (required) — Input Altium document path; quote spaces
- `<footprint>` (required) — Footprint name from `ace pcblib list`
- `<name>` (required) — Exact object name from the corresponding list command
- `<value>` (required) — New field/parameter value; include units for lengths (e.g. 0.2mm)

Example: `ace pcblib set-param Footprints.PcbLib SOIC-8 HEIGHT 2.5mm`

Returns: `{changed, file, changes, backup, verified, footprints}`

Notes: Keys are Altium's: DESCRIPTION, HEIGHT (a length with a unit), PATTERN, or any user parameter. Only the field's bytes change.

### `ace pcblib remove`
_native/write_ — Delete a footprint and its storage

- `<file>` (required) — Input Altium document path; quote spaces
- `<footprint>` (required) — Footprint name from `ace pcblib list`

Example: `ace pcblib remove Footprints.PcbLib SOIC-8`

Returns: `{changed, file, changes, backup, verified, footprints}`

Notes: 3D models the footprint used stay in the library.

### `ace pcblib copy`
_native/write_ — Copy a footprint from another .PcbLib, with the 3D models it uses

- `<file>` (required) — Input Altium document path; quote spaces
- `<source>` (required) — Library to copy from
- `<footprint>` (required) — Footprint name from `ace pcblib list`
- `--name` — Name in this library (default: the source name)

Example: `ace pcblib copy Footprints.PcbLib Other.PcbLib SOIC-8 --name SOIC-8N`

Returns: `{changed, file, changes, backup, verified, footprints}`

Notes: Copies the footprint's streams byte for byte; embedded models it references are copied when the library lacks them. Linked models keep their references and produce a warning. Use --name when the name is taken.


## bom

Bill of materials of a project, sheet or PCB: grouped lines, variants, CSV/JSON export

### `ace bom`
_native/read_ — Bill of materials of a project, sheet or PCB: grouped lines, variants, CSV/JSON export

- `<input>` (required) — .PrjPcb (all sheets), .SchDoc or .PcbDoc
- `--source` (one of sch|pcb) — For a project: read parts from the schematics (default) or the PCB
- `--pcb` — PCB document to use with --source pcb when the project has several
- `--group-by` (repeatable, default `comment,footprint`) — Fields that make one BOM line: comment, footprint, lib_ref, description, value, mpn, manufacturer, supplier_pn, kind, designator (one line per part) or any parameter name
- `--variant` — Apply this assembly variant (name, unique id or 1-based index)
- `--project` — Project holding the variants when the input is a sheet or PCB
- `--include-dnp` — Keep the variant's not-fitted parts (marked dnp) instead of dropping them
- `--include-excluded` — Keep parts excluded from the BOM by their component kind (marked excluded)
- `--columns` (repeatable) — Columns of the CSV file (default: line,quantity,designator_ranges,comment,footprint,value, manufacturer,mpn,supplier_pn,description,dnp). Any parameter name works
- `--out` — Write the BOM to this file (.csv or .json; see --format)
- `--format` (one of csv|json) — File format for --out (default: from the extension, else csv)

Example: `ace bom Board.PrjPcb --variant Lite --group-by comment,footprint,mpn --out bom.csv`

Returns: `{input, source, documents, documents_declared?, documents_found?, missing_documents?, incomplete, group_by, variant?, total_parts, unique_lines, count, items:[{line, quantity, designators, designator_ranges, comment, footprint, lib_ref, description, value?, manufacturer?, mpn?, supplier_pn?, parameters, inconsistent?, dnp?, excluded?}], dnp?, excluded?, varied?, out?}`

Notes: Project input uses every schematic (sub-parts merged, channels expanded). Variants: not-fitted parts are dropped (or marked with --include-dnp), parameter variations applied; alternate parts are flagged (resolve them with the designer BOM). --out writes CSV/JSON (honours --dry-run). `--group-by designator` gives one line per part. value uses Value/Val then comment (with a warning). CSV columns: line,quantity,designator_ranges,designator_list,comment,footprint,lib_ref,description,value,manufacturer,mpn,supplier_pn,dnp,excluded or any parameter; designator/designators remain aliases for designator_ranges. documents_declared/documents_found count schematic sheets; incomplete and missing_documents expose missing project dependencies.


## netlist

Nets with their nodes (or one row per pin with --pins); Protel/CSV/JSON export

### `ace netlist`
_native/read_ — Nets with their nodes (or one row per pin with --pins); Protel/CSV/JSON export

- `<input>` (required) — .PrjPcb (project-wide nets), .SchDoc (sheet nets) or .PcbDoc (PCB nets)
- `--source` (one of sch|pcb) — For a project: nets from the schematics (default) or the PCB
- `--pcb` — PCB document to use with --source pcb when the project has several
- `--pins` — One item per pin: {designator, pin, pin_name, net, node}
- `--connected` — Leave out nets without nodes (PCB nets that no pad uses)
- `--out` — Write the netlist to this file
- `--format` (one of protel|csv|json) — File format for --out (default: protel; .csv/.json by extension)

Example: `ace netlist Board.PrjPcb --where name=GND`

Returns: `{input, source, documents, documents_declared?, documents_found?, missing_documents?, incomplete, count, items:[{name, node_count, nodes:["R1.1"...], auto_named?, name_source?}] | --pins: items:[{designator, pin, pin_name?, net, node}], parts, out?}`

Notes: Schematic nets are a native approximation of Altium's compiler (`ace designer compile` is the authority); node pins are footprint pad numbers so they match the PCB. incomplete/missing_documents identify partial projects. --out writes protel (default), csv or json.


## check

Native design checks: annotation, duplicates, footprints, nets, schematic vs PCB sync

### `ace check`
_native/read_ — Native design checks: annotation, duplicates, footprints, nets, schematic vs PCB sync

- `<input>` — .PrjPcb (schematics + PCB), .SchDoc or .PcbDoc
- `--pcb` — PCB to compare a project's schematics with (default: the project's PCB; with several, the one sharing the most designators with the schematics)
- `--only` (repeatable) — Run only these check codes (comma-separated; see --list)
- `--skip` (repeatable) — Skip these check codes
- `--severity` (default `info`) — Minimum severity to report: info, warning or error
- `--require` (repeatable) — Parameters every BOM part must have (e.g. MPN,Manufacturer) -> missing-parameter
- `--baseline` — Compare with a complete saved `ace check` JSON envelope; gates count new findings
- `--fail-on` (one of none|warning|error, default `none`) — Exit 9 after emitting the normal report when this severity or higher is present
- `--list` — List the check codes with scope, meaning and fix hint instead of checking

Example: `ace check Board.PrjPcb --severity warning --require MPN`

Returns: `{input, schematic?, pcb?, incomplete, missing_documents?, documents_declared?, documents_found?, passed, errors, warnings, infos, by_code, by_scope, check_options, gate?, baseline?, new?, resolved?, unchanged?, passed_vs_baseline?, count, items:[{severity, code, scope, message, refs, hint}]} | --list: {count, items:[{code, scope, meaning, hint}]}`

Notes: Fast native pre-flight, not ERC/DRC (use `ace designer erc|drc`). passed=false when any error-severity finding remains after --only/--skip/--severity. --fail-on error|warning preserves the report and exits 9; by_code/by_scope are tallies before --limit. --baseline compares complete check envelopes by code/scope/severity/refs and reports new/resolved/unchanged; gates then count only new findings. refs are designators, nets or designator.pin nodes.


## diff

Compare two designs (revisions, sheet vs PCB) or a file against a git revision

### `ace diff`
_native/read_ — Compare two designs (revisions, sheet vs PCB) or a file against a git revision

- `<a>` (required) — Before: .PrjPcb, .SchDoc or .PcbDoc
- `<b>` — After (same kind, or a PCB to compare a project/sheet with). Omit with --rev
- `--rev` — Compare A as committed at this git revision (before) with A on disk (after)
- `--summary` — Return the summary without individual change rows
- `--by` (one of part) — Group changed part fields into one row per part
- `--ignore-field` (repeatable) — Ignore fields matching this regular expression (repeatable)
- `--only` (repeatable) — Compare only these fields (comma-separated; parameter names also work)
- `--match` (one of auto|designator|unique-id, default `auto`) — Match parts by unique ID first, then designator; never guess ambiguous IDs
- `--exit-code` — Exit 9 when designs differ (the normal JSON report is preserved)
- `--geometry` — Include position/rotation/side changes (PCB vs PCB only)
- `--source` (one of sch|pcb) — For projects: compare their schematics (default) or their PCBs

Example: `ace diff Board.PrjPcb --rev HEAD~1`

Returns: `{a, b, identical, incomplete?, coverage, gate?, comparison_fields?, summary:{parts_added, parts_removed, parts_changed, parts_renamed, by_field, nets_added, nets_removed, nets_renamed, nets_changed, rules_*?}, count, items:[{change, ref, field?, before?, after?, before_ref?, unique_id?, unit?, side?, fields?, added_nodes?, removed_nodes?}]}`

Notes: change: part_added|part_removed|part_renamed|part_changed|net_added|net_removed|net_renamed|net_changed|rule_added|rule_removed|rule_changed. A schematic/project vs a PCB compares parts and connectivity (not geometry). Project variants/settings are always compared. PcbLib/SchLib compare decoded fields by name (unknown binary streams excluded). Project inputs expose incomplete and per-side coverage; restore missing_documents before trusting a diff. --rev needs git on PATH and the files committed. --exit-code preserves the report and exits 9 on differences. --match auto uses unambiguous unique IDs before designators; --match designator reproduces add/remove behavior. --summary omits items; --by part groups fields; --ignore-field REGEX/--only FIELDS scope the comparison before --exit-code. Rule values keep JSON types and units.


## designer

Drive a real Altium Designer (local Windows, Windows over SSH, or Wine): compile, ERC, DRC, outputs, processes, live edits

### `ace designer setup`
_designer/control_ — Installation guide: every step with done/todo, who must do it, and the commands

- `--apply` — Run one automatable step (e.g. `bottles`, `bottle-env`, `fixes`, `bridge`)

Example: `ace designer setup`

Returns: `{done, total, next, count, items:[{id, title, done, who:auto|user, detail, commands}]} or {applied:[{command, exit}], steps}`

Notes: Start here when DESIGNER_UNAVAILABLE. who=user steps need a human (sudo, Altium sign-in, GUI installer).

### `ace designer config`
_designer/control_ — Show or change ~/.config/altium-cli-extended/designer.json

- `--set` (repeatable) — Set a key, e.g. `bottle=Altium`, `request_timeout_s=900`, `runtime=wine`

Example: `ace designer config --set host=ssh --set ssh_host=192.168.0.20 --set ssh_user=me`

Returns: `{path, config}`

### `ace designer status`
_designer/read_ — Is Altium running; where it is installed; is the bridge installed

Example: `ace designer status`

Returns: `{running, pid?, altium, prefix, runtime, bridge_installed, bridge_outdated}`

Notes: Cheap: no bridge round-trip.

### `ace designer start`
_designer/control_ — Launch Altium. With --wait, block until the bridge answers (first start is slow)

- `--wait` — wait for this operation; see the example and notes
- `--timeout` — Seconds to wait with --wait

Example: `ace designer start --wait`

Returns: `{started|ready, ...}`

Notes: First start takes minutes; later commands reuse the running Altium.

### `ace designer stop`
_designer/control_ — Close Altium (saves all documents first unless --force)

- `--force` — Kill without saving

Example: `ace designer stop`

Returns: `{stopped, signalled, saved}`

### `ace designer ping`
_designer/read_ — Round-trip through the bridge: product version, open projects, focused document

- `--timeout` — timeout for this operation; see the example and notes

Example: `ace designer ping`

Returns: `{protocol, product_version, projects:[...], focused_document?}`

Notes: Use to confirm Altium is licensed and responsive.

### `ace designer install-bridge`
_designer/control_ — Install or refresh the bridge DelphiScript project inside the Wine prefix

Example: `ace designer install-bridge`

Returns: `{count, items:[paths]}`

Notes: Done automatically by every designer command when missing or outdated.

### `ace designer grant`
_designer/control_ — Wine/Bottles only: let Altium inside the Flatpak sandbox read a host directory

- `<dir>` (required) — Directory path

Example: `ace designer grant ~/designs`

Returns: `{changed, granted}`

Notes: Wine/Bottles host only (Flatpak sandbox).

### `ace designer push`
_designer/control_ — Send a project (and the sheets it references) to Altium's side now

- `<file>` (required) — Input Altium document path; quote spaces

Example: `ace designer push Board.PrjPcb`

Returns: `{win_root, win_file, copied, sent:[files]}`

Notes: SSH host: copies only changed files (implicit in every designer command). Local/Wine hosts read files in place.

### `ace designer pull`
_designer/control_ — Bring back files Altium changed in a project since it was last sent (SSH host)

- `<file>` (required) — Input Altium document path; quote spaces

Example: `ace designer pull Board.PrjPcb`

Returns: `{count, items:[local paths]}`

Notes: SSH host: after edits in Altium (process/script/save-all), copies changed files back. Local/Wine: lists files changed since the last command.

### `ace designer screenshot`
_designer/read_ — PNG of the desktop Altium runs on — see the dialog it is waiting on

- `--out` (default `altium-screen.png`) — out for this operation; see the example and notes

Example: `ace designer screenshot -o screen.png`

Returns: `{path}`

Notes: Use when a designer command times out: shows the dialog Altium is waiting on (local or SSH host).

### `ace designer dialogs`
_designer/control_ — Dialogs Altium shows right now (a modal one blocks every request); optionally answer it

- `--press` (repeatable) — Press this button of the modal dialog (first match wins; repeatable). `*` closes it

Example: `ace designer dialogs --press Cancel`

Returns: `{running, main_title, blocked, windows:[{title, class, texts, buttons, enabled}], pressed?}`

Notes: blocked=true means a modal dialog waits for an answer; designer commands also detect this themselves and fail with the dialog text. Needs the Windows desktop session (local or SSH host).

### `ace designer reset`
_designer/control_ — Unstick Altium without restarting: dismiss modal dialogs (Cancel first) and stop a script halted in the debugger

Example: `ace designer reset`

Returns: `{pressed:[{dialog, pressed}], script_stopped, blocked, dialogs}`

Notes: Use after a TIMEOUT or a 'waiting on a dialog' error. Presses Cancel/Close/No/OK in that order of preference, then Run > Stop for a halted script.

### `ace designer documents`
_designer/read_ — Projects and documents currently open in Altium

Example: `ace designer documents`

Returns: `{projects:[{project, documents:[{path, kind}]}]}`

### `ace designer open`
_designer/control_ — Open a project or document in Altium

- `<file>` (required) — Input Altium document path; quote spaces

Example: `ace designer open Board.PrjPcb`

Returns: `{opened}`

### `ace designer compile`
_designer/read_ — Compile a project in Altium; returns every compiler/ERC violation with its objects

- `<project>` (required) — Project (.PrjPcb) path; resolves all referenced documents
- `--timeout` — timeout for this operation; see the example and notes

Example: `ace designer compile Board.PrjPcb --where severity=error`

Returns: `{project, documents, errors, warnings, count, items:[{level, severity, kind, descriptor, detail, objects:[{kind, descriptor, document, x, y}]}]}`

Notes: Authoritative connectivity/ERC from Altium itself. Paths point at your local files.

### `ace designer erc`
_designer/read_ — Electrical rules check (compile, then violations at warning level or above)

- `<project>` (required) — Project (.PrjPcb) path; resolves all referenced documents
- `--all` — Include violations below warning level
- `--timeout` — timeout for this operation; see the example and notes

Example: `ace designer erc Board.PrjPcb --where level=2`

Returns: `{count, items:[violation]} (warnings and up)`

Notes: level: 1 warning, 2 error, 3 fatal. --all includes no-report items.

### `ace designer drc`
_designer/read_ — Design rule check on a PCB (or the project's only PCB); returns every violation

- `<file>` (required) — A .PcbDoc, or a .PrjPcb with exactly one PCB
- `--process` — Override the process that starts DRC (default: PCB:DesignRuleCheck, whose dialog is answered automatically)
- `--params` — Parameters for --process, `Key=Value|Key=Value`
- `--timeout` — timeout for this operation; see the example and notes

Example: `ace designer drc Board.PcbDoc --fields rule,description,layer`

Returns: `{pcb, count, items:[{name, description, rule, rule_kind, layer, bbox, primitive1:{object, descriptor, layer, net, bbox}, primitive2}]}`

Notes: Batch DRC with the board's rules; nothing is saved.

### `ace designer netlist`
_designer/read_ — The compiled netlist from Altium: nets with pins, components with parameters

- `<project>` (required) — Project (.PrjPcb) path; resolves all referenced documents
- `--timeout` — timeout for this operation; see the example and notes

Example: `ace designer netlist Board.PrjPcb --where name=GND`

Returns: `{count, items:[{name, pins:[{designator, pin, name}]}], components:[{designator, comment, footprint, lib_ref, library, unique_id, parameters}]}`

Notes: Compiled by Altium (hierarchy, buses, harnesses, power ports): use when the native `netlist` approximation is not enough.

### `ace designer variants`
_designer/read_ — Project variants as Altium compiles them (with variations) and the current one

- `<project>` (required) — Project (.PrjPcb) path; resolves all referenced documents

Example: `ace designer variants Board.PrjPcb`

Returns: `{count, items:[{name, variations:[{designator, kind:fitted|not_fitted|alternate}]}], current}`

Notes: Pass a name to `designer export --variant`.

### `ace designer annotate`
_designer/write_ — Annotate schematic designators (quiet = only `?` designators), or board-level / back annotation; Altium saves and the changed sheets are copied back

- `<project>` (required) — Project (.PrjPcb) path; resolves all referenced documents
- `--mode` (one of quiet|all|reset|reset-duplicates|board|back, default `quiet`) — mode for this operation; see the example and notes
- `--timeout` — timeout for this operation; see the example and notes

Example: `ace designer annotate Board.PrjPcb --mode quiet`

Returns: `{mode, changes:[{before, after}], pulled:[files]}`

Notes: Altium saves the sheets; on an SSH host they are copied back (pulled). --dry-run is not available (Altium decides the numbering).

### `ace designer outjob`
_designer/export_ — Run an existing output job medium (Gerber, NC drill, ODB++, IPC-2581, BOM, PnP, STEP, PDF...)

- `<project>` (required) — Project (.PrjPcb) path; resolves all referenced documents
- `<outjob>` (required) — Output job (.OutJob) path
- `--medium` — Output medium name (see `ace outjob mediums`); default: the job's target medium
- `--pdf` — Publish to PDF instead of generating files
- `--timeout` — timeout for this operation; see the example and notes

Example: `ace designer outjob Board.PrjPcb Fab.OutJob --medium "Gerber folder"`

Returns: `{outjob, mediums, count, items:[generated files]}`

Notes: Inspect the job first with `ace outjob outputs/mediums`; generated files are found by modification time under the project folder (and copied back from an SSH host).

### `ace designer export`
_designer/export_ — Generate fabrication/assembly/documentation outputs without an OutJob of yours (a temporary one is generated); files come back under Project Outputs/altium-cli-extended/

- `<project>` (required) — Project (.PrjPcb) path; resolves all referenced documents
- `--kind` (required, repeatable) — Output kinds, comma separated or repeated: gerber, gerber-x2, drill, odb, ipc2581, pick-place, bom, sch-pdf, pcb-pdf, pcb-3d-pdf, step; or `fab` (gerber,drill), `assembly` (pick-place,bom,step), `all`
- `--pcb` — PCB document for board outputs when the project has several
- `--variant` — Variant for BOM / pick-and-place (see `ace designer variants`)
- `--timeout` — timeout for this operation; see the example and notes

Example: `ace designer export Board.PrjPcb --kind gerber,drill,pick-place,bom --variant Lite`

Returns: `{out_dir, count, items:[{path, size, format, kind}], by_format:{...}}`

Notes: No OutJob needed: a temporary one is added to the project for the run and removed (your project file is not modified). Presets: fab, assembly, docs, all.

### `ace designer pcb components`
_designer/read_ — Components as Altium has them now: position, rotation, side, lock, bounding box

- `<pcb>` (required) — PCB document (.PcbDoc) path

Example: `ace designer pcb components Board.PcbDoc --where side=bottom`

Returns: `{pcb, count, items:[{designator, comment, footprint, x, y, rotation, side, locked, selected, bbox}]}`

Notes: Live state, including unsaved edits.

### `ace designer pcb info`
_designer/read_ — Board statistics and live per-net routed length, via and pin counts, unrouted connections

- `<pcb>` (required) — PCB document (.PcbDoc) path

Example: `ace designer pcb info Board.PcbDoc`

Returns: `{components, nets, tracks, arcs, vias, pads, polygons, regions, fills, texts, rules, classes, unrouted_connections, board_outline, origin, net_lengths:[{name, routed_length, vias, pins}]}`

Notes: routed_length/unrouted_connections are computed by Altium (the native parser approximates).

### `ace designer pcb move`
_designer/write_ — Move components: absolute (--x/--y) or relative (--dx/--dy). Lengths take mm/mil suffixes; bare numbers use --units

- `<pcb>` (required) — PCB document (.PcbDoc) path
- `<designators>` (required, repeatable) — Designators, comma separated
- `--x` — x for this operation; see the example and notes
- `--y` — y for this operation; see the example and notes
- `--dx` — dx for this operation; see the example and notes
- `--dy` — dy for this operation; see the example and notes
- `--rotation` — Also set the rotation (degrees)

Example: `ace designer pcb move Board.PcbDoc U1,U2 --dx 2mm`

Returns: `{dry_run, items:[{before, after, changed}], pulled}`

Notes: Saved by Altium and copied back. --dry-run shows `before` without changing anything.

### `ace designer pcb rotate`
_designer/write_ — Set component rotation (degrees, absolute)

- `<pcb>` (required) — PCB document (.PcbDoc) path
- `<designators>` (required, repeatable) — designators for this operation; see the example and notes
- `--angle` (required) — angle for this operation; see the example and notes

Example: `ace designer pcb rotate Board.PcbDoc U1 --angle 90`

Returns: `{dry_run, items:[{before, after, changed}], pulled}`

### `ace designer pcb flip`
_designer/write_ — Put components on the top or bottom side (Altium flips the footprint)

- `<pcb>` (required) — PCB document (.PcbDoc) path
- `<designators>` (required, repeatable) — designators for this operation; see the example and notes
- `--side` (required, one of top|bottom) — side for this operation; see the example and notes

Example: `ace designer pcb flip Board.PcbDoc C5 --side bottom`

Returns: `{dry_run, items:[{before, after, changed}], pulled}`

Notes: Altium mirrors the footprint and swaps layers like Edit > Flip.

### `ace designer pcb lock`
_designer/write_ — Lock components (or unlock with --off)

- `<pcb>` (required) — PCB document (.PcbDoc) path
- `<designators>` (required, repeatable) — designators for this operation; see the example and notes
- `--off` — off for this operation; see the example and notes

Example: `ace designer pcb lock Board.PcbDoc J1,J2`

Returns: `{dry_run, items:[{before, after, changed}], pulled}`

Notes: --off unlocks.

### `ace designer pcb polygons`
_designer/write_ — List polygon pours, or repour / shelve / restore them

- `<pcb>` (required) — PCB document (.PcbDoc) path
- `--action` (one of list|repour|repour-modified|shelve|restore, default `list`) — action for this operation; see the example and notes

Example: `ace designer pcb polygons Board.PcbDoc --action repour`

Returns: `{action, before:[{name, layer, net, poured, enabled, bbox}], after?, pulled}`

Notes: action list is read-only; the others save the board.

### `ace designer pcb rules`
_designer/read_ — Design rules as Altium holds them

- `<pcb>` (required) — PCB document (.PcbDoc) path

Example: `ace designer pcb rules Board.PcbDoc`

Returns: `{count, items:[{name, kind, enabled, priority, scope1, scope2, comment, descriptor}]}`

### `ace designer pcb rule`
_designer/write_ — Create or edit a design rule (clearance gap, width min/max/preferred, scopes, priority)

- `<pcb>` (required) — PCB document (.PcbDoc) path
- `<name>` (required) — Rule name (an existing rule is edited)
- `--kind` — Kind for a new rule: clearance, width, hole_size, via_style, short_circuit, unrouted, routing_layers, routing_priority, differential_pairs, length, matched_lengths, component_clearance, hole_to_hole, solder_mask, paste_mask, polygon_connect, plane_connect
- `--scope1` — First object matching query, e.g. `InNet('GND')` or `All`
- `--scope2` — scope2 for this operation; see the example and notes
- `--priority` — priority for this operation; see the example and notes
- `--enabled` (one of true|false) — enabled for this operation; see the example and notes
- `--comment` — comment for this operation; see the example and notes
- `--gap` — Clearance gap
- `--min` — Width / hole size minimum
- `--max` — Width / hole size maximum
- `--preferred` — Preferred width
- `--delete` — Delete the rule instead

Example: `ace designer pcb rule Board.PcbDoc HV --kind clearance --scope1 "InNetClass('HV')" --gap 1mm --priority 1`

Returns: `{before, after, created, dry_run, pulled}`

Notes: Edits an existing rule by name, or creates one of --kind. Values: --gap (clearance), --min/--max/--preferred (width, hole size).

### `ace designer pcb classes`
_designer/read_ — Object classes (net, component, ...) with members

- `<pcb>` (required) — PCB document (.PcbDoc) path

Example: `ace designer pcb classes Board.PcbDoc --where kind=net`

Returns: `{count, items:[{name, kind, builtin, members}]}`

### `ace designer pcb class`
_designer/write_ — Create or edit a class: add/remove members, or delete it

- `<pcb>` (required) — PCB document (.PcbDoc) path
- `<name>` (required) — Exact object name from the corresponding list command
- `--kind` (one of net|component|pad|layer|differential_pair, default `net`) — kind for this operation; see the example and notes
- `--add` (repeatable) — add for this operation; see the example and notes
- `--remove` (repeatable) — remove for this operation; see the example and notes
- `--delete` — delete for this operation; see the example and notes

Example: `ace designer pcb class Board.PcbDoc HV --add VBUS,VIN`

Returns: `{before, after, created, dry_run, pulled}`

Notes: --delete removes the class.

### `ace designer pcb select`
_designer/control_ — Select components and/or net copper in Altium (for a following process), optionally zoom

- `<pcb>` (required) — PCB document (.PcbDoc) path
- `--designators` (repeatable) — designators for this operation; see the example and notes
- `--nets` (repeatable) — nets for this operation; see the example and notes
- `--clear` — Deselect everything first
- `--zoom` — zoom for this operation; see the example and notes

Example: `ace designer pcb select Board.PcbDoc --nets GND --clear --zoom`

Returns: `{selected, selected_total}`

Notes: Selection only (not saved); combine with `designer process` for selection-based Altium commands.

### `ace designer sch components`
_designer/read_ — Components on a sheet as Altium has them now, with parameters

- `<sch>` (required) — Schematic document (.SchDoc) path

Example: `ace designer sch components Power.SchDoc`

Returns: `{sch, count, items:[{designator, comment, lib_ref, x, y, orientation, parameters}]}`

Notes: Live state.

### `ace designer sch param`
_designer/write_ — Set (or add, or --delete) a component parameter

- `<sch>` (required) — Schematic document (.SchDoc) path
- `<designator>` (required) — Component designator from the component list, e.g. R5; quote 'U?'
- `<name>` (required) — Exact object name from the corresponding list command
- `<value>` — New field/parameter value; include units for lengths (e.g. 0.2mm)
- `--visible` — Show the new parameter on the sheet
- `--delete` — delete for this operation; see the example and notes

Example: `ace designer sch param Power.SchDoc R1 Tolerance 1%`

Returns: `{before, after, component, dry_run, pulled}`

Notes: Adds the parameter when missing (hidden unless --visible); --delete removes it.

### `ace designer sch place`
_designer/write_ — Place a component from a library (.SchLib / .IntLib)

- `<sch>` (required) — Schematic document (.SchDoc) path
- `--lib` (required) — Library file (path inside the project folder, or one Altium has installed)
- `--lib-ref` (required) — Library reference (symbol name)
- `--x` (required) — x for this operation; see the example and notes
- `--y` (required) — y for this operation; see the example and notes
- `--rotation` (default `0`) — rotation for this operation; see the example and notes
- `--designator` — Component designator from the component list, e.g. R5; quote 'U?'

Example: `ace designer sch place Power.SchDoc --lib Lib.SchLib --lib-ref RES --x 50mm --y 40mm --designator R9`

Returns: `{component?, dry_run, pulled}`

Notes: Uses Altium's library placement (footprint and parameters come from the library).

### `ace designer sch net-label`
_designer/write_ — Place a net label

- `<sch>` (required) — Schematic document (.SchDoc) path
- `<text>` (required) — text for this operation; see the example and notes
- `--x` (required) — x for this operation; see the example and notes
- `--y` (required) — y for this operation; see the example and notes
- `--rotation` (default `0`) — rotation for this operation; see the example and notes

Example: `ace designer sch net-label Power.SchDoc VBUS --x 1000mil --y 2000mil`

Returns: `{placed, text, dry_run, pulled}`

### `ace designer sch power-port`
_designer/write_ — Place a power port

- `<sch>` (required) — Schematic document (.SchDoc) path
- `<text>` (required) — Net name, e.g. `GND`, `+3V3`
- `--x` (required) — x for this operation; see the example and notes
- `--y` (required) — y for this operation; see the example and notes
- `--style` (one of bar|arrow|wave|circle|ground|signal_ground|earth, default `bar`) — style for this operation; see the example and notes
- `--rotation` (default `0`) — rotation for this operation; see the example and notes

Example: `ace designer sch power-port Power.SchDoc GND --x 1000mil --y 1500mil --style ground`

Returns: `{placed, text, dry_run, pulled}`

### `ace designer sch wire`
_designer/write_ — Place a wire through two or more points: `--point 0,0 --point 10mm,0`

- `<sch>` (required) — Schematic document (.SchDoc) path
- `--point` (required, repeatable) — points for this operation; see the example and notes

Example: `ace designer sch wire Power.SchDoc --point 1000mil,2000mil --point 1500mil,2000mil`

Returns: `{vertices, dry_run, pulled}`

Notes: Points snap to nothing: use grid-aligned coordinates (multiples of 100 mil) so wires connect to pins.

### `ace designer process`
_designer/control_ — Run any Altium process (Server:Process) with parameters — the universal escape hatch

- `<name>` (required) — e.g. `WorkspaceManager:Compile`, `PCB:Zoom`, `Sch:Annotate`
- `--param` (repeatable) — Process parameter, repeatable: `--param Action=All`
- `--timeout` — timeout for this operation; see the example and notes

Example: `ace designer process WorkspaceManager:Compile --param ObjectKind=Project`

Returns: `{process, params}`

Notes: Any Altium process; see Altium's process reference. Parameters may not contain '|'.

### `ace designer script`
_designer/control_ — Run your own DelphiScript procedure inside Altium

- `<file>` (required) — .pas file
- `--proc-name` (default `Main`) — Procedure to call
- `--timeout` — timeout for this operation; see the example and notes

Example: `ace designer script MyTool.pas --proc-name Main`

Returns: `whatever the script writes to its job user.json, else null`

Notes: Runtime errors are caught and reported; the procedure must take no arguments.

### `ace designer save-all`
_designer/write_ — Save every modified document in Altium

Example: `ace designer save-all`

Returns: `{saved}`

### `ace designer close-all`
_designer/control_ — Close every document in Altium

Example: `ace designer close-all`

Returns: `{closed}`

### `ace designer call`
_designer/control_ — Low level: send one bridge operation (see `ops` in assets/Bridge.pas) with parameters. Files given with --file are staged and their parameter set to Altium's path

- `<op>` (required) — op for this operation; see the example and notes
- `--param` (repeatable) — Parameter, repeatable: `--param mode=quiet`
- `--file` (repeatable) — File parameter, repeatable: `--file pcb=Board.PcbDoc`
- `--pull` — Copy changed files back afterwards
- `--timeout` — timeout for this operation; see the example and notes

Example: `ace designer call pcb_info --file pcb=Board.PcbDoc`

Returns: `the operation's data, plus _job and pulled`

Notes: Escape hatch to the bridge operations behind the typed commands (ops: ping, documents, open, close, save, compile, netlist, variants, annotate, drc, outjob, process, save_all, close_all, pcb_components, pcb_edit, pcb_polygons, pcb_rules, pcb_rule, pcb_classes, pcb_select, pcb_info, sch_components, sch_param, sch_place, sch_label, sch_wire). Coordinates are Altium internal units.

### `ace designer jobs`
_designer/read_ — Recent bridge jobs with their request, status and log (for debugging)

- `--last` (default `10`) — last for this operation; see the example and notes

Example: `ace designer jobs --last 3`

Returns: `{count, items:[{id, op, ok?, error?, log_tail}]}`

## Errors

| code | category | exit | meaning |
|---|---|---|---|
| `USAGE` | input | 2 | bad arguments or unknown command; read `ace describe <cmd>` |
| `FILE_NOT_FOUND` | input | 3 | input path does not exist |
| `WRONG_FILE_TYPE` | input | 4 | file is not the Altium document type this command expects |
| `PARSE` | design | 5 | file is the right type but could not be decoded |
| `NOT_FOUND` | input | 6 | named object (designator, net, stream, symbol...) not in the file |
| `AMBIGUOUS` | input | 7 | query matched several objects; narrow it |
| `UNSAFE` | validation | 8 | write refused to protect the design (locked, open in Altium, unverifiable) |
| `GATE_FAILED` | design | 9 | command succeeded but an explicit gate matched; see data.gate |
| `UNSUPPORTED_NATIVE` | input | 14 | operation requires Altium Designer; follow the hint |
| `DESIGNER_UNAVAILABLE` | environment | 10 | Altium Designer host (local, SSH or Wine) or its bridge not set up; run `ace doctor` |
| `DESIGNER_FAILED` | crash | 11 | Altium ran but the operation failed; see error.details |
| `TIMEOUT` | timeout | 12 | Altium did not answer in time; it may be busy or stuck on a dialog |
| `IO` | environment | 13 | file system error |
| `INTERNAL` | internal | 70 | bug in altium-cli-extended; please report with the command line |
