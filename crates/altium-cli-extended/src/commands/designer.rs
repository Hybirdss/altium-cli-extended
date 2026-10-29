//! `ace designer ...` (alias `ad`) — drive the real Altium Designer under Wine.

use std::path::{Path, PathBuf};

use clap::Subcommand;
use serde_json::{Value, json};

use altium_core::error::{Error, ErrorCode, Result};
use altium_core::Coord;
use altium_designer::bridge::{Request, Response};
use altium_designer::config::{self, Config};
use altium_designer::ops::Designer;
use altium_designer::{install, setup, wine};
use altium_doc::project::Project;
use altium_format::DocKind;

use crate::ctx::Ctx;
use crate::meta::{Backend::*, CmdMeta, Effect::*, m};
use crate::output::{Out, list, list_with};

#[derive(Subcommand, Debug)]
pub enum DesignerCmd {
    /// Installation guide: every step with done/todo, who must do it, and the commands.
    Setup {
        /// Run one automatable step (e.g. `bottles`, `bottle-env`, `fixes`, `bridge`).
        #[arg(long)]
        apply: Option<String>,
    },
    /// Show or change ~/.config/altium-cli-extended/designer.json.
    Config {
        /// Set a key, e.g. `bottle=Altium`, `request_timeout_s=900`, `runtime=wine`.
        #[arg(long = "set", value_name = "KEY=VALUE")]
        set: Vec<String>,
    },
    /// Is Altium running; where it is installed; is the bridge installed.
    Status,
    /// Launch Altium. With --wait, block until the bridge answers (first start is slow).
    Start {
        #[arg(long)]
        wait: bool,
        /// Seconds to wait with --wait.
        #[arg(long)]
        timeout: Option<u64>,
    },
    /// Close Altium (saves all documents first unless --force).
    Stop {
        /// Kill without saving.
        #[arg(long)]
        force: bool,
    },
    /// Round-trip through the bridge: product version, open projects, focused document.
    Ping {
        #[arg(long)]
        timeout: Option<u64>,
    },
    /// Install or refresh the bridge DelphiScript project inside the Wine prefix.
    InstallBridge,
    /// Wine/Bottles only: let Altium inside the Flatpak sandbox read a host directory.
    Grant { dir: PathBuf },
    /// Send a project (and the sheets it references) to Altium's side now.
    Push { file: PathBuf },
    /// Bring back files Altium changed in a project since it was last sent (SSH host).
    Pull { file: PathBuf },
    /// PNG of the desktop Altium runs on — see the dialog it is waiting on.
    Screenshot {
        #[arg(long, short, default_value = "altium-screen.png")]
        out: PathBuf,
    },
    /// Dialogs Altium shows right now (a modal one blocks every request); optionally answer it.
    Dialogs {
        /// Press this button of the modal dialog (first match wins; repeatable). `*` closes it.
        #[arg(long)]
        press: Vec<String>,
    },
    /// Unstick Altium without restarting: dismiss modal dialogs (Cancel first) and stop a
    /// script halted in the debugger.
    Reset,
    /// Projects and documents currently open in Altium.
    Documents,
    /// Open a project or document in Altium.
    Open { file: PathBuf },
    /// Compile a project in Altium; returns every compiler/ERC violation with its objects.
    Compile {
        project: PathBuf,
        #[arg(long)]
        timeout: Option<u64>,
    },
    /// Electrical rules check (compile, then violations at warning level or above).
    Erc {
        project: PathBuf,
        /// Include violations below warning level.
        #[arg(long)]
        all: bool,
        #[arg(long)]
        timeout: Option<u64>,
    },
    /// Design rule check on a PCB (or the project's only PCB); returns every violation.
    Drc {
        /// A .PcbDoc, or a .PrjPcb with exactly one PCB.
        file: PathBuf,
        /// Override the process that starts DRC (default: PCB:DesignRuleCheck, whose dialog is
        /// answered automatically).
        #[arg(long)]
        process: Option<String>,
        /// Parameters for --process, `Key=Value|Key=Value`.
        #[arg(long)]
        params: Option<String>,
        #[arg(long)]
        timeout: Option<u64>,
    },
    /// The compiled netlist from Altium: nets with pins, components with parameters.
    Netlist {
        project: PathBuf,
        #[arg(long)]
        timeout: Option<u64>,
    },
    /// Project variants as Altium compiles them (with variations) and the current one.
    Variants { project: PathBuf },
    /// Annotate schematic designators (quiet = only `?` designators), or board-level / back
    /// annotation; Altium saves and the changed sheets are copied back.
    Annotate {
        project: PathBuf,
        #[arg(long, value_enum, default_value_t = AnnotateMode::Quiet)]
        mode: AnnotateMode,
        #[arg(long)]
        timeout: Option<u64>,
    },
    /// Run an existing output job medium (Gerber, NC drill, ODB++, IPC-2581, BOM, PnP, STEP, PDF...).
    Outjob {
        project: PathBuf,
        outjob: PathBuf,
        /// Output medium name (see `ace outjob mediums`); default: the job's target medium.
        #[arg(long)]
        medium: Option<String>,
        /// Publish to PDF instead of generating files.
        #[arg(long)]
        pdf: bool,
        #[arg(long)]
        timeout: Option<u64>,
    },
    /// Generate fabrication/assembly/documentation outputs without an OutJob of yours
    /// (a temporary one is generated); files come back under Project Outputs/altium-cli-extended/.
    Export {
        project: PathBuf,
        /// Output kinds, comma separated or repeated: gerber, gerber-x2, drill, odb, ipc2581,
        /// pick-place, bom, sch-pdf, pcb-pdf, pcb-3d-pdf, step; or `fab` (gerber,drill),
        /// `assembly` (pick-place,bom,step), `all`.
        #[arg(long, value_delimiter = ',', required = true)]
        kind: Vec<String>,
        /// PCB document for board outputs when the project has several.
        #[arg(long)]
        pcb: Option<PathBuf>,
        /// Variant for BOM / pick-and-place (see `ace designer variants`).
        #[arg(long)]
        variant: Option<String>,
        #[arg(long)]
        timeout: Option<u64>,
    },
    /// Live PCB operations in Altium (positions, polygons, rules, classes, selection, stats).
    #[command(subcommand)]
    Pcb(PcbCmd),
    /// Live schematic operations in Altium (components, parameters, placement, labels, wires).
    #[command(subcommand)]
    Sch(SchCmd),
    /// Run any Altium process (Server:Process) with parameters — the universal escape hatch.
    Process {
        /// e.g. `WorkspaceManager:Compile`, `PCB:Zoom`, `Sch:Annotate`.
        name: String,
        /// Process parameter, repeatable: `--param Action=All`.
        #[arg(long = "param", value_name = "KEY=VALUE")]
        params: Vec<String>,
        #[arg(long)]
        timeout: Option<u64>,
    },
    /// Run your own DelphiScript procedure inside Altium.
    Script {
        /// .pas file.
        file: PathBuf,
        /// Procedure to call.
        #[arg(long, default_value = "Main")]
        proc_name: String,
        #[arg(long)]
        timeout: Option<u64>,
    },
    /// Save every modified document in Altium.
    SaveAll,
    /// Close every document in Altium.
    CloseAll,
    /// Low level: send one bridge operation (see `ops` in assets/Bridge.pas) with parameters.
    /// Files given with --file are staged and their parameter set to Altium's path.
    Call {
        op: String,
        /// Parameter, repeatable: `--param mode=quiet`.
        #[arg(long = "param", value_name = "KEY=VALUE")]
        params: Vec<String>,
        /// File parameter, repeatable: `--file pcb=Board.PcbDoc`.
        #[arg(long = "file", value_name = "KEY=PATH")]
        files: Vec<String>,
        /// Copy changed files back afterwards.
        #[arg(long)]
        pull: bool,
        #[arg(long)]
        timeout: Option<u64>,
    },
    /// Recent bridge jobs with their request, status and log (for debugging).
    Jobs {
        #[arg(long, default_value_t = 10)]
        last: usize,
    },
}

#[derive(clap::ValueEnum, Clone, Copy, Debug)]
pub enum AnnotateMode {
    /// Only designators ending in `?`.
    Quiet,
    /// Re-annotate every component.
    All,
    /// Reset every designator to `?`.
    Reset,
    /// Reset duplicated designators to `?`.
    ResetDuplicates,
    /// Board-level annotation (physical designators of multi-channel designs).
    Board,
    /// Back-annotate schematics from the PCB (after PCB re-annotation).
    Back,
}

impl AnnotateMode {
    fn op(self) -> &'static str {
        match self {
            AnnotateMode::Quiet => "quiet",
            AnnotateMode::All => "all",
            AnnotateMode::Reset => "reset",
            AnnotateMode::ResetDuplicates => "reset_duplicates",
            AnnotateMode::Board => "board",
            AnnotateMode::Back => "back",
        }
    }
}

#[derive(Subcommand, Debug)]
pub enum PcbCmd {
    /// Components as Altium has them now: position, rotation, side, lock, bounding box.
    Components { pcb: PathBuf },
    /// Board statistics and live per-net routed length, via and pin counts, unrouted connections.
    Info { pcb: PathBuf },
    /// Move components: absolute (--x/--y) or relative (--dx/--dy). Lengths take mm/mil
    /// suffixes; bare numbers use --units.
    Move {
        pcb: PathBuf,
        /// Designators, comma separated.
        #[arg(value_delimiter = ',', required = true)]
        designators: Vec<String>,
        #[arg(long, allow_hyphen_values = true)]
        x: Option<String>,
        #[arg(long, allow_hyphen_values = true)]
        y: Option<String>,
        #[arg(long, allow_hyphen_values = true)]
        dx: Option<String>,
        #[arg(long, allow_hyphen_values = true)]
        dy: Option<String>,
        /// Also set the rotation (degrees).
        #[arg(long, allow_hyphen_values = true)]
        rotation: Option<f64>,
    },
    /// Set component rotation (degrees, absolute).
    Rotate {
        pcb: PathBuf,
        #[arg(value_delimiter = ',', required = true)]
        designators: Vec<String>,
        #[arg(long, allow_hyphen_values = true)]
        angle: f64,
    },
    /// Put components on the top or bottom side (Altium flips the footprint).
    Flip {
        pcb: PathBuf,
        #[arg(value_delimiter = ',', required = true)]
        designators: Vec<String>,
        #[arg(long, value_parser = ["top", "bottom"])]
        side: String,
    },
    /// Lock components (or unlock with --off).
    Lock {
        pcb: PathBuf,
        #[arg(value_delimiter = ',', required = true)]
        designators: Vec<String>,
        #[arg(long)]
        off: bool,
    },
    /// List polygon pours, or repour / shelve / restore them.
    Polygons {
        pcb: PathBuf,
        #[arg(long, value_parser = ["list", "repour", "repour-modified", "shelve", "restore"], default_value = "list")]
        action: String,
    },
    /// Design rules as Altium holds them.
    Rules { pcb: PathBuf },
    /// Create or edit a design rule (clearance gap, width min/max/preferred, scopes, priority).
    Rule {
        pcb: PathBuf,
        /// Rule name (an existing rule is edited).
        name: String,
        /// Kind for a new rule: clearance, width, hole_size, via_style, short_circuit, unrouted,
        /// routing_layers, routing_priority, differential_pairs, length, matched_lengths,
        /// component_clearance, hole_to_hole, solder_mask, paste_mask, polygon_connect, plane_connect.
        #[arg(long)]
        kind: Option<String>,
        /// First object matching query, e.g. `InNet('GND')` or `All`.
        #[arg(long)]
        scope1: Option<String>,
        #[arg(long)]
        scope2: Option<String>,
        #[arg(long)]
        priority: Option<u32>,
        #[arg(long)]
        enabled: Option<bool>,
        #[arg(long)]
        comment: Option<String>,
        /// Clearance gap.
        #[arg(long)]
        gap: Option<String>,
        /// Width / hole size minimum.
        #[arg(long)]
        min: Option<String>,
        /// Width / hole size maximum.
        #[arg(long)]
        max: Option<String>,
        /// Preferred width.
        #[arg(long)]
        preferred: Option<String>,
        /// Delete the rule instead.
        #[arg(long)]
        delete: bool,
    },
    /// Object classes (net, component, ...) with members.
    Classes { pcb: PathBuf },
    /// Create or edit a class: add/remove members, or delete it.
    Class {
        pcb: PathBuf,
        name: String,
        #[arg(long, value_parser = ["net", "component", "pad", "layer", "differential_pair"], default_value = "net")]
        kind: String,
        #[arg(long, value_delimiter = ',')]
        add: Vec<String>,
        #[arg(long, value_delimiter = ',')]
        remove: Vec<String>,
        #[arg(long)]
        delete: bool,
    },
    /// Select components and/or net copper in Altium (for a following process), optionally zoom.
    Select {
        pcb: PathBuf,
        #[arg(long, value_delimiter = ',')]
        designators: Vec<String>,
        #[arg(long, value_delimiter = ',')]
        nets: Vec<String>,
        /// Deselect everything first.
        #[arg(long)]
        clear: bool,
        #[arg(long)]
        zoom: bool,
    },
}

#[derive(Subcommand, Debug)]
pub enum SchCmd {
    /// Components on a sheet as Altium has them now, with parameters.
    Components { sch: PathBuf },
    /// Set (or add, or --delete) a component parameter.
    Param {
        sch: PathBuf,
        designator: String,
        name: String,
        value: Option<String>,
        /// Show the new parameter on the sheet.
        #[arg(long)]
        visible: bool,
        #[arg(long)]
        delete: bool,
    },
    /// Place a component from a library (.SchLib / .IntLib).
    Place {
        sch: PathBuf,
        /// Library file (path inside the project folder, or one Altium has installed).
        #[arg(long)]
        lib: String,
        /// Library reference (symbol name).
        #[arg(long)]
        lib_ref: String,
        #[arg(long, allow_hyphen_values = true)]
        x: String,
        #[arg(long, allow_hyphen_values = true)]
        y: String,
        #[arg(long, default_value_t = 0)]
        rotation: u32,
        #[arg(long)]
        designator: Option<String>,
    },
    /// Place a net label.
    NetLabel {
        sch: PathBuf,
        text: String,
        #[arg(long, allow_hyphen_values = true)]
        x: String,
        #[arg(long, allow_hyphen_values = true)]
        y: String,
        #[arg(long, default_value_t = 0)]
        rotation: u32,
    },
    /// Place a power port.
    PowerPort {
        sch: PathBuf,
        /// Net name, e.g. `GND`, `+3V3`.
        text: String,
        #[arg(long, allow_hyphen_values = true)]
        x: String,
        #[arg(long, allow_hyphen_values = true)]
        y: String,
        #[arg(long, value_parser = ["bar", "arrow", "wave", "circle", "ground", "signal_ground", "earth"], default_value = "bar")]
        style: String,
        #[arg(long, default_value_t = 0)]
        rotation: u32,
    },
    /// Place a wire through two or more points: `--point 0,0 --point 10mm,0`.
    Wire {
        sch: PathBuf,
        #[arg(long = "point", required = true, num_args = 1)]
        points: Vec<String>,
    },
}

pub const META: &[CmdMeta] = &[
    m("designer setup", Designer, Control, "{done, total, next, count, items:[{id, title, done, who:auto|user, detail, commands}]} or {applied:[{command, exit}], steps}",
      "ace designer setup", "Start here when DESIGNER_UNAVAILABLE. who=user steps need a human (sudo, Altium sign-in, GUI installer)."),
    m("designer config", Designer, Control, "{path, config}", "ace designer config --set host=ssh --set ssh_host=192.168.0.20 --set ssh_user=me", ""),
    m("designer status", Designer, Read, "{running, pid?, altium, prefix, runtime, bridge_installed, bridge_outdated}", "ace designer status", "Cheap: no bridge round-trip."),
    m("designer start", Designer, Control, "{started|ready, ...}", "ace designer start --wait", "First start takes minutes; later commands reuse the running Altium."),
    m("designer stop", Designer, Control, "{stopped, signalled, saved}", "ace designer stop", ""),
    m("designer ping", Designer, Read, "{protocol, product_version, projects:[...], focused_document?}", "ace designer ping", "Use to confirm Altium is licensed and responsive."),
    m("designer install-bridge", Designer, Control, "{count, items:[paths]}", "ace designer install-bridge", "Done automatically by every designer command when missing or outdated."),
    m("designer grant", Designer, Control, "{changed, granted}", "ace designer grant ~/designs", "Wine/Bottles host only (Flatpak sandbox)."),
    m("designer push", Designer, Control, "{win_root, win_file, copied, sent:[files]}", "ace designer push Board.PrjPcb",
      "SSH host: copies only changed files (implicit in every designer command). Local/Wine hosts read files in place."),
    m("designer pull", Designer, Control, "{count, items:[local paths]}", "ace designer pull Board.PrjPcb",
      "SSH host: after edits in Altium (process/script/save-all), copies changed files back. Local/Wine: lists files changed since the last command."),
    m("designer screenshot", Designer, Read, "{path}", "ace designer screenshot -o screen.png",
      "Use when a designer command times out: shows the dialog Altium is waiting on (local or SSH host)."),
    m("designer dialogs", Designer, Control, "{running, main_title, blocked, windows:[{title, class, texts, buttons, enabled}], pressed?}",
      "ace designer dialogs --press Cancel",
      "blocked=true means a modal dialog waits for an answer; designer commands also detect this themselves and fail with the dialog text. Needs the Windows desktop session (local or SSH host)."),
    m("designer reset", Designer, Control, "{pressed:[{dialog, pressed}], script_stopped, blocked, dialogs}", "ace designer reset",
      "Use after a TIMEOUT or a 'waiting on a dialog' error. Presses Cancel/Close/No/OK in that order of preference, then Run > Stop for a halted script."),
    m("designer documents", Designer, Read, "{projects:[{project, documents:[{path, kind}]}]}", "ace designer documents", ""),
    m("designer open", Designer, Control, "{opened}", "ace designer open Board.PrjPcb", ""),
    m("designer compile", Designer, Read, "{project, documents, errors, warnings, count, items:[{level, severity, kind, descriptor, detail, objects:[{kind, descriptor, document, x, y}]}]}",
      "ace designer compile Board.PrjPcb --where severity=error", "Authoritative connectivity/ERC from Altium itself. Paths point at your local files."),
    m("designer erc", Designer, Read, "{count, items:[violation]} (warnings and up)", "ace designer erc Board.PrjPcb --where level=2",
      "level: 1 warning, 2 error, 3 fatal. --all includes no-report items."),
    m("designer drc", Designer, Read, "{pcb, count, items:[{name, description, rule, rule_kind, layer, bbox, primitive1:{object, descriptor, layer, net, bbox}, primitive2}]}",
      "ace designer drc Board.PcbDoc --fields rule,description,layer", "Batch DRC with the board's rules; nothing is saved."),
    m("designer netlist", Designer, Read, "{count, items:[{name, pins:[{designator, pin, name}]}], components:[{designator, comment, footprint, lib_ref, library, unique_id, parameters}]}",
      "ace designer netlist Board.PrjPcb --where name=GND", "Compiled by Altium (hierarchy, buses, harnesses, power ports): use when the native `netlist` approximation is not enough."),
    m("designer variants", Designer, Read, "{count, items:[{name, variations:[{designator, kind:fitted|not_fitted|alternate}]}], current}",
      "ace designer variants Board.PrjPcb", "Pass a name to `designer export --variant`."),
    m("designer annotate", Designer, Write, "{mode, changes:[{before, after}], pulled:[files]}", "ace designer annotate Board.PrjPcb --mode quiet",
      "Altium saves the sheets; on an SSH host they are copied back (pulled). --dry-run is not available (Altium decides the numbering)."),
    m("designer outjob", Designer, Export, "{outjob, mediums, count, items:[generated files]}",
      "ace designer outjob Board.PrjPcb Fab.OutJob --medium \"Gerber folder\"",
      "Inspect the job first with `ace outjob outputs/mediums`; generated files are found by modification time under the project folder (and copied back from an SSH host)."),
    m("designer export", Designer, Export, "{out_dir, count, items:[{path, size, format, kind}], by_format:{...}}",
      "ace designer export Board.PrjPcb --kind gerber,drill,pick-place,bom --variant Lite",
      "No OutJob needed: a temporary one is added to the project for the run and removed (your project file is not modified). Presets: fab, assembly, docs, all."),
    m("designer pcb components", Designer, Read, "{pcb, count, items:[{designator, comment, footprint, x, y, rotation, side, locked, selected, bbox}]}",
      "ace designer pcb components Board.PcbDoc --where side=bottom", "Live state, including unsaved edits."),
    m("designer pcb info", Designer, Read, "{components, nets, tracks, arcs, vias, pads, polygons, regions, fills, texts, rules, classes, unrouted_connections, board_outline, origin, net_lengths:[{name, routed_length, vias, pins}]}",
      "ace designer pcb info Board.PcbDoc", "routed_length/unrouted_connections are computed by Altium (the native parser approximates)."),
    m("designer pcb move", Designer, Write, "{dry_run, items:[{before, after, changed}], pulled}", "ace designer pcb move Board.PcbDoc U1,U2 --dx 2mm",
      "Saved by Altium and copied back. --dry-run shows `before` without changing anything."),
    m("designer pcb rotate", Designer, Write, "{dry_run, items:[{before, after, changed}], pulled}", "ace designer pcb rotate Board.PcbDoc U1 --angle 90", ""),
    m("designer pcb flip", Designer, Write, "{dry_run, items:[{before, after, changed}], pulled}", "ace designer pcb flip Board.PcbDoc C5 --side bottom",
      "Altium mirrors the footprint and swaps layers like Edit > Flip."),
    m("designer pcb lock", Designer, Write, "{dry_run, items:[{before, after, changed}], pulled}", "ace designer pcb lock Board.PcbDoc J1,J2", "--off unlocks."),
    m("designer pcb polygons", Designer, Write, "{action, before:[{name, layer, net, poured, enabled, bbox}], after?, pulled}",
      "ace designer pcb polygons Board.PcbDoc --action repour", "action list is read-only; the others save the board."),
    m("designer pcb rules", Designer, Read, "{count, items:[{name, kind, enabled, priority, scope1, scope2, comment, descriptor}]}", "ace designer pcb rules Board.PcbDoc", ""),
    m("designer pcb rule", Designer, Write, "{before, after, created, dry_run, pulled}",
      "ace designer pcb rule Board.PcbDoc HV --kind clearance --scope1 \"InNetClass('HV')\" --gap 1mm --priority 1",
      "Edits an existing rule by name, or creates one of --kind. Values: --gap (clearance), --min/--max/--preferred (width, hole size)."),
    m("designer pcb classes", Designer, Read, "{count, items:[{name, kind, builtin, members}]}", "ace designer pcb classes Board.PcbDoc --where kind=net", ""),
    m("designer pcb class", Designer, Write, "{before, after, created, dry_run, pulled}", "ace designer pcb class Board.PcbDoc HV --add VBUS,VIN", "--delete removes the class."),
    m("designer pcb select", Designer, Control, "{selected, selected_total}", "ace designer pcb select Board.PcbDoc --nets GND --clear --zoom",
      "Selection only (not saved); combine with `designer process` for selection-based Altium commands."),
    m("designer sch components", Designer, Read, "{sch, count, items:[{designator, comment, lib_ref, x, y, orientation, parameters}]}", "ace designer sch components Power.SchDoc", "Live state."),
    m("designer sch param", Designer, Write, "{before, after, component, dry_run, pulled}", "ace designer sch param Power.SchDoc R1 Tolerance 1%",
      "Adds the parameter when missing (hidden unless --visible); --delete removes it."),
    m("designer sch place", Designer, Write, "{component?, dry_run, pulled}", "ace designer sch place Power.SchDoc --lib Lib.SchLib --lib-ref RES --x 50mm --y 40mm --designator R9",
      "Uses Altium's library placement (footprint and parameters come from the library)."),
    m("designer sch net-label", Designer, Write, "{placed, text, dry_run, pulled}", "ace designer sch net-label Power.SchDoc VBUS --x 1000mil --y 2000mil", ""),
    m("designer sch power-port", Designer, Write, "{placed, text, dry_run, pulled}", "ace designer sch power-port Power.SchDoc GND --x 1000mil --y 1500mil --style ground", ""),
    m("designer sch wire", Designer, Write, "{vertices, dry_run, pulled}", "ace designer sch wire Power.SchDoc --point 1000mil,2000mil --point 1500mil,2000mil",
      "Points snap to nothing: use grid-aligned coordinates (multiples of 100 mil) so wires connect to pins."),
    m("designer process", Designer, Control, "{process, params}", "ace designer process WorkspaceManager:Compile --param ObjectKind=Project",
      "Any Altium process; see Altium's process reference. Parameters may not contain '|'."),
    m("designer script", Designer, Control, "whatever the script writes to its job user.json, else null",
      "ace designer script MyTool.pas --proc-name Main", "Runtime errors are caught and reported; the procedure must take no arguments."),
    m("designer save-all", Designer, Write, "{saved}", "ace designer save-all", ""),
    m("designer close-all", Designer, Control, "{closed}", "ace designer close-all", ""),
    m("designer call", Designer, Control, "the operation's data, plus _job and pulled", "ace designer call pcb_info --file pcb=Board.PcbDoc",
      "Escape hatch to the bridge operations behind the typed commands (ops: ping, documents, open, close, save, compile, netlist, variants, annotate, drc, outjob, process, save_all, close_all, pcb_components, pcb_edit, pcb_polygons, pcb_rules, pcb_rule, pcb_classes, pcb_select, pcb_info, sch_components, sch_param, sch_place, sch_label, sch_wire). Coordinates are Altium internal units."),
    m("designer jobs", Designer, Read, "{count, items:[{id, op, ok?, error?, log_tail}]}", "ace designer jobs --last 3", ""),
];

fn with_job(resp: Response) -> Result<Out> {
    let mut data = resp.data;
    if let Value::Object(o) = &mut data {
        o.insert("_job".into(), json!({"id": resp.job, "elapsed_ms": resp.elapsed_ms}));
    }
    Out::new(data)
}

/// Response data plus `_job` plus the members of `extra`.
fn with_job_extra(resp: Response, extra: Value) -> Result<Out> {
    let mut data = resp.data;
    if !data.is_object() {
        data = json!({"result": data});
    }
    if let (Value::Object(o), Value::Object(e)) = (&mut data, extra) {
        o.extend(e);
        o.insert("_job".into(), json!({"id": resp.job, "elapsed_ms": resp.elapsed_ms}));
    }
    Out::new(data)
}

/// Turn a `violations` array in the response into a standard list.
fn violations_list(resp: Response, keep: impl Fn(&Value) -> bool) -> Result<Out> {
    let items: Vec<Value> = resp
        .data
        .get("violations")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|v| keep(v))
        .collect();
    let mut extra = resp.data.clone();
    if let Value::Object(o) = &mut extra {
        o.remove("violations");
        o.insert("_job".into(), json!({"id": resp.job, "elapsed_ms": resp.elapsed_ms}));
    }
    Out::new(list_with(&items, extra)?)
}

/// `data[key]` (an array) as the standard list; the other members stay alongside.
fn take_list(resp: Response, key: &str, extra: Value) -> Result<Out> {
    let mut data = resp.data;
    let items = data.as_object_mut().and_then(|o| o.remove(key)).and_then(|v| v.as_array().cloned()).unwrap_or_default();
    if let (Value::Object(o), Value::Object(e)) = (&mut data, extra) {
        o.extend(e);
        o.insert("_job".into(), json!({"id": resp.job, "elapsed_ms": resp.elapsed_ms}));
    }
    Out::new(list_with(&items, data)?)
}

/// Files Altium touches as a side effect (logs, history, previews), not outputs.
fn is_altium_noise(p: &Path) -> bool {
    let s = p.to_string_lossy();
    s.contains("__Previews") || s.contains("/History/") || s.contains("\\History\\") || s.ends_with(".PrjPcbStructure")
        || p.extension().is_some_and(|e| e.eq_ignore_ascii_case("log") || e.eq_ignore_ascii_case("OutJob"))
}

/// `{path, size, format}` for a generated file; `format` from the extension (Gerber layer
/// extensions, drill, ODB++, PDF, STEP, CSV...).
fn file_info(p: &Path, kind: Option<&str>) -> Value {
    let ext = p.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    let name = p.file_name().map(|n| n.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    let format = match ext.as_str() {
        "gtl" | "gbl" | "gto" | "gbo" | "gts" | "gbs" | "gtp" | "gbp" | "gko" | "gm1" | "gpt" | "gpb" | "gd1" | "gg1" | "apr" => "gerber",
        e if e.starts_with('g') && e.len() <= 4 && e[1..].chars().all(|c| c.is_ascii_alphanumeric()) && !matches!(e, "gif") => "gerber",
        "gbr" => if name.contains("x2") { "gerber-x2" } else { "gerber" },
        "gbrjob" => "gerber-x2-job",
        "drl" | "txt" if name.contains("drill") || name.contains("-plated") || name.contains("-nonplated") => "nc-drill",
        "drl" | "drr" | "lst" => "nc-drill",
        "tgz" | "tar" => "odb++",
        "xml" | "cvg" if name.contains("2581") || kind == Some("ipc2581") => "ipc-2581",
        "pdf" => "pdf",
        "step" | "stp" => "step",
        "csv" => "csv",
        "xls" | "xlsx" => "spreadsheet",
        "html" | "htm" => "html",
        "txt" => "text",
        "rul" => "drc-rules",
        "extrep" | "rep" => "report",
        _ => "other",
    };
    let size = std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
    let mut v = json!({"path": p, "size": size, "format": format});
    if let Some(k) = kind {
        v["kind"] = json!(k);
    }
    v
}

fn export(ctx: &Ctx, project: &Path, kinds: &[String], pcb: Option<&Path>, variant: Option<&str>, timeout: Option<u64>) -> Result<Out> {
    use altium_doc::outjob::{NewOutput, OUTPUT_KINDS, OutputKind};
    let project = ctx.input(project)?;
    if DocKind::of(&project) != DocKind::Project {
        return Err(Error::new(ErrorCode::WrongFileType, "export needs the .PrjPcb").hint("outputs are generated for a project"));
    }
    let mut ks: Vec<OutputKind> = vec![];
    for k in kinds {
        let more: Vec<OutputKind> = match k.to_ascii_lowercase().as_str() {
            "all" => OUTPUT_KINDS.to_vec(),
            "fab" | "fabrication" => vec![OutputKind::Gerber, OutputKind::Drill],
            "assembly" => vec![OutputKind::PickPlace, OutputKind::Bom, OutputKind::Step],
            "docs" | "documentation" => vec![OutputKind::SchPdf, OutputKind::PcbPdf],
            other => vec![OutputKind::parse(other)?],
        };
        for m in more {
            if !ks.contains(&m) {
                ks.push(m);
            }
        }
    }
    let prj = Project::load(&project)?;
    let pcb_name = match pcb {
        Some(p) => Some(ctx.input(p)?.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()),
        None => match prj.pcbs().as_slice() {
            [] => None,
            [one] => one.file_name().map(|n| n.to_string_lossy().into_owned()),
            many => {
                return Err(Error::new(ErrorCode::Ambiguous, "the project has several PCBs; pass --pcb")
                    .details(json!(many)));
            }
        },
    };
    if ks.iter().any(|k| k.needs_pcb()) && pcb_name.is_none() {
        return Err(Error::not_found("the project has no PCB document for board outputs"));
    }
    if let Some(v) = variant {
        prj.variant(v)?;
    }
    let outputs: Vec<NewOutput> = ks
        .iter()
        .map(|k| NewOutput { kind: *k, document: if k.needs_pcb() { pcb_name.clone().unwrap_or_default() } else { String::new() } })
        .collect();
    if ctx.dry_run {
        return Out::new(json!({"dry_run": true, "kinds": ks.iter().map(|k| k.name()).collect::<Vec<_>>(), "pcb": pcb_name, "variant": variant}));
    }
    let (c, out_dir) = designer(ctx)?.export(&project, outputs, variant, timeout)?;
    let items: Vec<Value> = c.pulled.iter().map(|p| file_info(p, kind_of_output(p, &out_dir))).collect();
    let mut by_format = serde_json::Map::new();
    for i in &items {
        let f = i["format"].as_str().unwrap_or("other").to_owned();
        let e = by_format.entry(f).or_insert(json!(0));
        *e = json!(e.as_u64().unwrap_or(0) + 1);
    }
    let produced: std::collections::BTreeSet<&str> = items.iter().filter_map(|i| i.get("kind").and_then(Value::as_str)).collect();
    let missing: Vec<&str> = ks.iter().map(|k| k.name()).filter(|k| {
        !produced.contains(k) && !(OutputKind::parse(k).is_ok_and(|o| o.is_pdf()) && items.iter().any(|i| i["format"] == "pdf"))
    }).collect();
    let mut out = Out::new(list_with(&items, json!({
        "out_dir": out_dir,
        "kinds": ks.iter().map(|k| k.name()).collect::<Vec<_>>(),
        "variant": variant,
        "by_format": by_format,
        "missing": missing,
        "_job": {"id": c.resp.job, "elapsed_ms": c.resp.elapsed_ms},
    }))?)?;
    if !missing.is_empty() {
        out = out.warn(format!("no files for: {} (see error.details of `ace designer jobs --last 1`, or the screenshot)", missing.join(", ")));
    }
    Ok(out)
}

/// Generated files sit in a folder named after the output (`UseOutputName`): map it back to the
/// output kind.
fn kind_of_output(p: &Path, out_dir: &Path) -> Option<&'static str> {
    let rel = p.strip_prefix(out_dir).ok()?;
    let first = rel.components().next()?.as_os_str().to_string_lossy().to_ascii_lowercase();
    altium_doc::outjob::OUTPUT_KINDS.iter().map(|k| k.name()).find(|n| *n == first)
}

/// Parse a length argument: `10mm`, `400mil`, `0.5in`, or a bare number in the --units unit.
fn len_arg(s: &str) -> Result<i64> {
    let t = s.trim().to_ascii_lowercase();
    let (num, unit) = if let Some(n) = t.strip_suffix("mm") {
        (n, "mm")
    } else if let Some(n) = t.strip_suffix("mil") {
        (n, "mil")
    } else if let Some(n) = t.strip_suffix("in") {
        (n, "in")
    } else {
        (t.as_str(), altium_core::units::output_unit().name())
    };
    let v: f64 = num.trim().parse().map_err(|_| Error::usage(format!("not a length: {s:?}")).hint("e.g. 2.5mm, 100mil, or a number in --units"))?;
    Ok(match unit {
        "mm" => Coord::from_mm(v).0,
        "mil" => Coord::from_mil(v).0,
        "in" => Coord::from_mil(v * 1000.0).0,
        _ => v.round() as i64,
    })
}

fn designators_req(op: &str, ds: &[String]) -> Request {
    Request::new(op).list("d", &ds.iter().map(|d| d.trim().to_owned()).collect::<Vec<_>>())
}

/// Run a PCB/schematic operation and shape its answer (`list_key` becomes the list).
fn live(c: altium_designer::ops::Called, list_key: Option<&str>) -> Result<Out> {
    let pulled = json!({"pulled": c.pulled});
    match list_key {
        Some(k) => take_list(c.resp, k, pulled),
        None => with_job_extra(c.resp, pulled),
    }
}

fn run_pcb(c: PcbCmd, ctx: &Ctx) -> Result<Out> {
    let d = || designer(ctx);
    let dry = if ctx.dry_run { "1" } else { "0" };
    match c {
        PcbCmd::Components { pcb } => live(d()?.pcb_op(&ctx.input(&pcb)?, Request::new("pcb_components"), false, None)?, Some("components")),
        PcbCmd::Info { pcb } => live(d()?.pcb_op(&ctx.input(&pcb)?, Request::new("pcb_info"), false, None)?, None),
        PcbCmd::Move { pcb, designators, x, y, dx, dy, rotation } => {
            if x.is_none() && y.is_none() && dx.is_none() && dy.is_none() && rotation.is_none() {
                return Err(Error::usage("nothing to do").hint("give --x/--y (absolute), --dx/--dy (relative) and/or --rotation"));
            }
            let mut r = designators_req("pcb_edit", &designators).param("dry_run", dry);
            for i in 1..=designators.len() {
                for (k, v) in [("x", &x), ("y", &y), ("dx", &dx), ("dy", &dy)] {
                    if let Some(v) = v {
                        r = r.param(&format!("{k}{i}"), len_arg(v)?.to_string());
                    }
                }
                if let Some(a) = rotation {
                    r = r.param(&format!("rot{i}"), a.to_string());
                }
            }
            live(d()?.pcb_op(&ctx.input(&pcb)?, r, !ctx.dry_run, None)?, Some("items"))
        }
        PcbCmd::Rotate { pcb, designators, angle } => {
            let mut r = designators_req("pcb_edit", &designators).param("dry_run", dry);
            for i in 1..=designators.len() {
                r = r.param(&format!("rot{i}"), angle.to_string());
            }
            live(d()?.pcb_op(&ctx.input(&pcb)?, r, !ctx.dry_run, None)?, Some("items"))
        }
        PcbCmd::Flip { pcb, designators, side } => {
            let mut r = designators_req("pcb_edit", &designators).param("dry_run", dry);
            for i in 1..=designators.len() {
                r = r.param(&format!("side{i}"), side.clone());
            }
            live(d()?.pcb_op(&ctx.input(&pcb)?, r, !ctx.dry_run, None)?, Some("items"))
        }
        PcbCmd::Lock { pcb, designators, off } => {
            let mut r = designators_req("pcb_edit", &designators).param("dry_run", dry);
            for i in 1..=designators.len() {
                r = r.param(&format!("lock{i}"), if off { "0" } else { "1" });
            }
            live(d()?.pcb_op(&ctx.input(&pcb)?, r, !ctx.dry_run, None)?, Some("items"))
        }
        PcbCmd::Polygons { pcb, action } => {
            let write = action != "list" && !ctx.dry_run;
            let act = if ctx.dry_run { "list".to_owned() } else { action.replace('-', "_") };
            live(d()?.pcb_op(&ctx.input(&pcb)?, Request::new("pcb_polygons").param("action", act), write, None)?, None)
        }
        PcbCmd::Rules { pcb } => live(d()?.pcb_op(&ctx.input(&pcb)?, Request::new("pcb_rules"), false, None)?, Some("rules")),
        PcbCmd::Rule { pcb, name, kind, scope1, scope2, priority, enabled, comment, gap, min, max, preferred, delete } => {
            let mut r = Request::new("pcb_rule").param("name", name).param("dry_run", dry).opt("kind", kind).opt("scope1", scope1).opt("scope2", scope2)
                .opt("priority", priority.map(|p| p.to_string())).opt("enabled", enabled.map(|b| if b { "1" } else { "0" })).opt("comment", comment);
            for (k, v) in [("gap", gap), ("min", min), ("max", max), ("preferred", preferred)] {
                if let Some(v) = v {
                    r = r.param(k, len_arg(&v)?.to_string());
                }
            }
            if delete {
                r = r.param("delete", "1");
            }
            live(d()?.pcb_op(&ctx.input(&pcb)?, r, !ctx.dry_run, None)?, None)
        }
        PcbCmd::Classes { pcb } => live(d()?.pcb_op(&ctx.input(&pcb)?, Request::new("pcb_classes"), false, None)?, Some("classes")),
        PcbCmd::Class { pcb, name, kind, add, remove, delete } => {
            let mut r = Request::new("pcb_classes").param("name", name).param("kind", kind).param("dry_run", dry).list("add", &add).list("remove", &remove);
            if delete {
                r = r.param("delete", "1");
            }
            live(d()?.pcb_op(&ctx.input(&pcb)?, r, !ctx.dry_run, None)?, None)
        }
        PcbCmd::Select { pcb, designators, nets, clear, zoom } => {
            let r = designators_req("pcb_select", &designators).list("net", &nets).param("clear", if clear { "1" } else { "0" }).param("zoom", if zoom { "1" } else { "0" });
            live(d()?.pcb_op(&ctx.input(&pcb)?, r, false, None)?, None)
        }
    }
}

fn run_sch(c: SchCmd, ctx: &Ctx) -> Result<Out> {
    let d = || designer(ctx);
    let dry = if ctx.dry_run { "1" } else { "0" };
    match c {
        SchCmd::Components { sch } => live(d()?.sch_op(&ctx.input(&sch)?, Request::new("sch_components"), false, None)?, Some("components")),
        SchCmd::Param { sch, designator, name, value, visible, delete } => {
            if value.is_none() && !delete {
                return Err(Error::usage("give a value, or --delete"));
            }
            let mut r = Request::new("sch_param").param("designator", designator).param("name", name).param("dry_run", dry).opt("value", value);
            if visible {
                r = r.param("visible", "1");
            }
            if delete {
                r = r.param("delete", "1");
            }
            live(d()?.sch_op(&ctx.input(&sch)?, r, !ctx.dry_run, None)?, None)
        }
        SchCmd::Place { sch, lib, lib_ref, x, y, rotation, designator } => {
            let sch = ctx.input(&sch)?;
            let mut r = Request::new("sch_place").param("lib_ref", lib_ref).param("x", len_arg(&x)?.to_string()).param("y", len_arg(&y)?.to_string())
                .param("rot", rotation.to_string()).param("dry_run", dry).opt("designator", designator);
            let lib_path = Path::new(&lib);
            if lib_path.exists() {
                let c = d()?.call_with(r, &[("sch", sch.as_path()), ("lib", lib_path)], !ctx.dry_run, None)?;
                return live(c, None);
            }
            r = r.param("lib", lib);
            live(d()?.sch_op(&sch, r, !ctx.dry_run, None)?, None)
        }
        SchCmd::NetLabel { sch, text, x, y, rotation } => {
            let r = Request::new("sch_label").param("what", "net_label").param("text", text).param("x", len_arg(&x)?.to_string()).param("y", len_arg(&y)?.to_string())
                .param("rot", rotation.to_string()).param("dry_run", dry);
            live(d()?.sch_op(&ctx.input(&sch)?, r, !ctx.dry_run, None)?, None)
        }
        SchCmd::PowerPort { sch, text, x, y, style, rotation } => {
            let r = Request::new("sch_label").param("what", "power").param("text", text).param("style", style).param("x", len_arg(&x)?.to_string())
                .param("y", len_arg(&y)?.to_string()).param("rot", rotation.to_string()).param("dry_run", dry);
            live(d()?.sch_op(&ctx.input(&sch)?, r, !ctx.dry_run, None)?, None)
        }
        SchCmd::Wire { sch, points } => {
            let mut xs = vec![];
            let mut ys = vec![];
            for p in &points {
                let (x, y) = p.split_once(',').ok_or_else(|| Error::usage(format!("point {p:?} must be X,Y")))?;
                xs.push(len_arg(x)?.to_string());
                ys.push(len_arg(y)?.to_string());
            }
            if xs.len() < 2 {
                return Err(Error::usage("a wire needs at least two --point"));
            }
            let r = Request::new("sch_wire").list("x", &xs).list("y", &ys).param("dry_run", dry);
            live(d()?.sch_op(&ctx.input(&sch)?, r, !ctx.dry_run, None)?, None)
        }
    }
}

fn single_pcb(file: &Path) -> Result<PathBuf> {
    match DocKind::of(file) {
        DocKind::Pcb => Ok(file.to_path_buf()),
        DocKind::Project => {
            let p = Project::load(file)?;
            match p.pcbs().as_slice() {
                [one] => Ok(one.clone()),
                [] => Err(Error::not_found("the project has no PCB document")),
                many => Err(Error::new(ErrorCode::Ambiguous, "the project has several PCBs; pass the .PcbDoc")
                    .details(json!(many))),
            }
        }
        _ => Err(Error::new(ErrorCode::WrongFileType, format!("{} is not a .PcbDoc or .PrjPcb", file.display()))),
    }
}

fn kv(s: &str) -> Result<(String, String)> {
    s.split_once('=')
        .map(|(k, v)| (k.trim().to_owned(), v.to_owned()))
        .ok_or_else(|| Error::usage(format!("expected KEY=VALUE, got {s:?}")))
}

fn designer(ctx: &Ctx) -> Result<Designer> {
    Ok(Designer::open()?.with_write_options(altium_format::fsx::WriteOpts { dry_run: ctx.dry_run, no_backup: ctx.no_backup }))
}

pub fn run(c: DesignerCmd, ctx: &Ctx) -> Result<Out> {
    let opaque = matches!(&c, DesignerCmd::Process { .. } | DesignerCmd::Script { .. } | DesignerCmd::Call { .. });
    let mutates = opaque || matches!(&c, DesignerCmd::SaveAll | DesignerCmd::Pull { .. } | DesignerCmd::Reset |
        DesignerCmd::CloseAll | DesignerCmd::Open { .. } | DesignerCmd::Push { .. } | DesignerCmd::InstallBridge |
        DesignerCmd::Annotate { .. } | DesignerCmd::Outjob { .. } | DesignerCmd::Export { .. } | DesignerCmd::Stop { .. });
    if mutates && ctx.dry_run {
        return Out::new(json!({"dry_run": true, "would_run": format!("{c:?}")}));
    }
    if opaque && !ctx.no_backup {
        return Err(Error::new(ErrorCode::Unsafe, "arbitrary designer processes, scripts and calls cannot guarantee file backups")
            .hint("use a typed designer command for automatic .bak protection, or explicitly pass --no-backup for a script you trust"));
    }
    if mutates && ctx.read_only {
        return Err(Error::new(ErrorCode::Unsafe, "read-only session refuses designer writes")
            .hint("use --dry-run to preview the command"));
    }
    match c {
        DesignerCmd::Setup { apply } => {
            let cfg = Config::load()?;
            if let Some(step) = apply {
                if ctx.dry_run {
                    let s = setup::steps(&cfg).into_iter().find(|s| s.id == step);
                    return Out::new(json!({"dry_run": true, "step": s}));
                }
                let ran = setup::apply(&cfg, &step)?;
                let items: Vec<Value> = ran.iter().map(|(c, code)| json!({"command": c, "exit": code})).collect();
                return Out::new(json!({"applied": items, "steps": setup::steps(&cfg)}));
            }
            let steps = setup::steps(&cfg);
            let done = steps.iter().filter(|s| s.done).count();
            let next = steps.iter().find(|s| !s.done && s.id != "sign-in").map(|s| s.id);
            Out::new(list_with(&steps, json!({"done": done, "total": steps.len(), "next": next}))?)
        }
        DesignerCmd::Config { set } => {
            let mut cfg = Config::load()?;
            if !set.is_empty() {
                let mut v = serde_json::to_value(&cfg)?;
                for s in &set {
                    let (k, val) = kv(s)?;
                    let parsed = val.parse::<u64>().map(Value::from).unwrap_or(Value::String(val));
                    v[k] = parsed;
                }
                cfg = serde_json::from_value(v).map_err(|e| {
                    Error::usage(format!("bad config: {e}"))
                        .hint("keys: host (local|ssh|wine), altium_exe, ssh_host, ssh_port, ssh_user, ssh_key, bottle, prefix, wine, altium_dir, start_timeout_s, request_timeout_s")
                })?;
                if !ctx.dry_run {
                    cfg.save()?;
                }
            }
            Out::new(json!({"path": config::path(), "config": cfg}))
        }
        DesignerCmd::Status => Out::new(designer(ctx)?.status()?),
        DesignerCmd::Start { wait, timeout } => Out::new(designer(ctx)?.start(wait, timeout)?),
        DesignerCmd::Stop { force } => Out::new(designer(ctx)?.stop(force)?),
        DesignerCmd::Ping { timeout } => with_job(designer(ctx)?.ping(timeout)?),
        DesignerCmd::InstallBridge => {
            let d = designer(ctx)?;
            Out::new(list(&d.bridge().install()?)?)
        }
        DesignerCmd::Grant { dir } => {
            let cfg = Config::load()?;
            if altium_designer::ops::host_choice(&cfg) != "wine" {
                return Out::new(json!({"changed": false, "reason": "only the Wine/Bottles host has a sandbox"}));
            }
            let inst = install::discover(&cfg).installation.ok_or_else(|| Error::new(ErrorCode::DesignerUnavailable, "no Wine installation found"))?;
            Out::new(wine::WineHost::new(inst).grant(&ctx.input(&dir)?)?)
        }
        DesignerCmd::Push { file } => Out::new(designer(ctx)?.stage(&ctx.input(&file)?)?),
        DesignerCmd::Pull { file } => {
            let files = designer(ctx)?.pull(&ctx.input(&file)?)?;
            Out::new(list(&files)?)
        }
        DesignerCmd::Screenshot { out } => Out::new(json!({"path": designer(ctx)?.screenshot(&out)?})),
        DesignerCmd::Dialogs { press } => {
            if !press.is_empty() && ctx.dry_run {
                return Out::new(json!({"dry_run": true, "would_press": press}));
            }
            Out::new(designer(ctx)?.dialogs(&press)?)
        }
        DesignerCmd::Reset => Out::new(designer(ctx)?.reset()?),
        DesignerCmd::Documents => with_job(designer(ctx)?.documents()?),
        DesignerCmd::Open { file } => with_job(designer(ctx)?.open_doc(&ctx.input(&file)?)?.resp),
        DesignerCmd::Compile { project, timeout } => {
            let c = designer(ctx)?.compile(&ctx.input(&project)?, timeout)?;
            violations_list(c.resp, |_| true)
        }
        DesignerCmd::Erc { project, all, timeout } => {
            let c = designer(ctx)?.compile(&ctx.input(&project)?, timeout)?;
            violations_list(c.resp, |v| all || v.get("level").and_then(Value::as_i64).is_none_or(|l| l >= 1))
        }
        DesignerCmd::Drc { file, process, params, timeout } => {
            let pcb = single_pcb(&ctx.input(&file)?)?;
            let c = designer(ctx)?.drc(&pcb, process.as_deref(), params.as_deref(), timeout)?;
            violations_list(c.resp, |_| true)
        }
        DesignerCmd::Netlist { project, timeout } => {
            let c = designer(ctx)?.netlist(&ctx.input(&project)?, timeout)?;
            take_list(c.resp, "nets", json!({}))
        }
        DesignerCmd::Variants { project } => {
            let c = designer(ctx)?.variants(&ctx.input(&project)?)?;
            take_list(c.resp, "variants", json!({}))
        }
        DesignerCmd::Annotate { project, mode, timeout } => {
            if ctx.dry_run {
                return Err(Error::usage("annotate cannot be previewed: Altium decides the numbering").hint("run `ace designer compile` to see designators needing annotation"));
            }
            let c = designer(ctx)?.annotate(&ctx.input(&project)?, mode.op(), timeout)?;
            with_job_extra(c.resp, json!({"pulled": c.pulled}))
        }
        DesignerCmd::Outjob { project, outjob, medium, pdf, timeout } => {
            let project = ctx.input(&project)?;
            let outjob = ctx.input(&outjob)?;
            let action = if pdf { "PublishToPDF" } else { "Run" };
            let c = designer(ctx)?.outjob(&project, &outjob, medium.as_deref(), action, timeout)?;
            let files: Vec<&PathBuf> = c.pulled.iter().filter(|p| !is_altium_noise(p)).collect();
            let items: Vec<Value> = files.iter().map(|p| file_info(p, None)).collect();
            let mut out = Out::new(list_with(&items, json!({"outjob": outjob, "mediums": c.resp.data.get("mediums"), "_job": {"id": c.resp.job, "elapsed_ms": c.resp.elapsed_ms}}))?)?;
            if items.is_empty() {
                out = out.warn("Altium produced no new or changed files under the project folder; check the medium name and the job's output path");
            }
            Ok(out)
        }
        DesignerCmd::Export { project, kind, pcb, variant, timeout } => export(ctx, &project, &kind, pcb.as_deref(), variant.as_deref(), timeout),
        DesignerCmd::Pcb(c) => run_pcb(c, ctx),
        DesignerCmd::Sch(c) => run_sch(c, ctx),
        DesignerCmd::Process { name, params, timeout } => {
            let ps = params.iter().map(|p| kv(p)).collect::<Result<Vec<_>>>()?;
            with_job(designer(ctx)?.process(&name, &ps, timeout)?)
        }
        DesignerCmd::Script { file, proc_name, timeout } => {
            with_job(designer(ctx)?.script(&ctx.input(&file)?, &proc_name, timeout)?)
        }
        DesignerCmd::SaveAll => with_job(designer(ctx)?.save_all()?),
        DesignerCmd::CloseAll => with_job(designer(ctx)?.close_all()?),
        DesignerCmd::Call { op, params, files, pull, timeout } => {
            let mut req = altium_designer::bridge::Request::new(&op);
            for p in &params {
                let (k, v) = kv(p)?;
                req = req.param(&k, v);
            }
            let fs: Vec<(String, PathBuf)> = files.iter().map(|f| kv(f).and_then(|(k, v)| Ok((k, ctx.input(Path::new(&v))?)))).collect::<Result<_>>()?;
            let refs: Vec<(&str, &Path)> = fs.iter().map(|(k, p)| (k.as_str(), p.as_path())).collect();
            let c = designer(ctx)?.call_with(req, &refs, pull, timeout)?;
            with_job_extra(c.resp, if pull { json!({"pulled": c.pulled}) } else { json!({}) })
        }
        DesignerCmd::Jobs { last } => {
            let d = designer(ctx)?;
            let h = d.host.as_ref();
            let jobs = altium_designer::bridge::Bridge::jobs_dir();
            let mut ids: Vec<String> = h.list(&jobs)?.into_iter().filter(|n| n.chars().next().is_some_and(|c| c.is_ascii_digit())).collect();
            ids.sort();
            let mut items = vec![];
            for id in ids.iter().rev().take(last) {
                let dir = format!(r"{jobs}\{id}");
                let text = |f: &str| h.read(&format!(r"{dir}\{f}")).ok().flatten().map(|b| String::from_utf8_lossy(&b).into_owned());
                let req = text("request.ini").unwrap_or_default();
                let op = req.trim_start_matches('\u{feff}').lines().find_map(|l| l.strip_prefix("op=")).unwrap_or("").to_owned();
                let resp: Option<Value> = text("response.json").and_then(|t| serde_json::from_str(t.trim_start_matches('\u{feff}')).ok());
                let log: Vec<String> = text("log.txt").unwrap_or_default().lines().rev().take(5).map(str::to_owned).collect();
                items.push(json!({
                    "id": id,
                    "op": op,
                    "done": resp.is_some(),
                    "ok": resp.as_ref().and_then(|r| r.get("ok")).cloned(),
                    "error": resp.as_ref().and_then(|r| r.get("error")).cloned(),
                    "log_tail": log,
                    "dir": dir,
                }));
            }
            Out::new(list(&items)?)
        }
    }
}

