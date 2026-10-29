//! `.OutJob` output job containers.
//!
//! Layout: `[OutputGroup1]` holds `OutputMediumN` (+ `_Type`), and per output `OutputTypeN`,
//! `OutputNameN`, `OutputCategoryN`, `OutputDocumentPathN`, `OutputVariantNameN`,
//! `OutputEnabledN`, `OutputEnabledN_OutputMediumM`, plus `ConfigurationN_ItemK` settings.

use std::path::{Path, PathBuf};

use indexmap::IndexMap;
use serde::Serialize;

use altium_core::error::{Error, ErrorCode, Result};
use altium_format::DocKind;
use altium_format::ini::{Ini, pipe_props};

pub struct OutJob {
    pub path: PathBuf,
    pub ini: Ini,
}

#[derive(Debug, Clone, Serialize)]
pub struct Medium {
    pub index: u32,
    pub name: String,
    /// `GeneratedFiles` (folder), `Publish` (PDF), `Printer`, `Video`, ...
    pub kind: String,
    /// Medium settings from `[PublishSettings]` / `[GeneratedFilesSettings]` keyed `..N`.
    pub settings: IndexMap<String, String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Output {
    pub index: u32,
    pub name: String,
    /// Altium output type (`Gerber X2`, `NC Drill`, `ODB++`, `BOM_PartType`, `Pick Place`...).
    pub output_type: String,
    pub category: String,
    /// Source document (`[Project Physical Documents]`, a file name, or empty = project).
    pub document: String,
    pub variant: String,
    /// Mediums this output is enabled for (by medium name).
    pub enabled_for: Vec<String>,
    /// Number of configuration records (the per-output settings Altium stores).
    pub config_items: usize,
}

impl OutJob {
    pub fn load(path: &Path) -> Result<OutJob> {
        if DocKind::of(path) != DocKind::OutJob {
            return Err(Error::new(ErrorCode::WrongFileType, format!("{} is not an .OutJob", path.display())));
        }
        let ini = Ini::load(path)?;
        if ini.section("OutputGroup1").is_none() {
            return Err(Error::parse(format!("{} has no [OutputGroup1]", path.display())));
        }
        Ok(OutJob { path: path.to_path_buf(), ini })
    }

    fn group(&self) -> &altium_format::ini::Section {
        self.ini.section("OutputGroup1").expect("checked in load")
    }

    pub fn group_info(&self) -> IndexMap<String, String> {
        let g = self.group();
        ["Name", "Description", "TargetOutputMedium", "VariantName", "VariantScope", "CurrentConfigurationName"]
            .iter()
            .map(|k| (k.to_string(), g.str(k)))
            .collect()
    }

    pub fn mediums(&self) -> Vec<Medium> {
        let g = self.group();
        let mut out = vec![];
        for i in 1.. {
            let Some(name) = g.get(&format!("OutputMedium{i}")) else { break };
            let kind = g.str(&format!("OutputMedium{i}_Type"));
            // Settings keys end with the medium's (global) index, in either section.
            let mut settings = IndexMap::new();
            for section in ["GeneratedFilesSettings", "PublishSettings"] {
                let Some(s) = self.ini.section(section) else { continue };
                for (k, v) in &s.entries {
                    if let Some(base) = k.strip_suffix(&i.to_string())
                        && base.chars().last().is_some_and(|c| !c.is_ascii_digit()) {
                            settings.entry(base.to_owned()).or_insert_with(|| v.clone());
                        }
                }
            }
            out.push(Medium { index: i, name: name.to_owned(), kind, settings });
        }
        out
    }

    pub fn outputs(&self) -> Vec<Output> {
        let g = self.group();
        let mediums = self.mediums();
        let mut out = vec![];
        for i in 1.. {
            let Some(t) = g.get(&format!("OutputType{i}")) else { break };
            let enabled_for = mediums
                .iter()
                .filter(|m| g.get(&format!("OutputEnabled{i}_OutputMedium{}", m.index)) == Some("1"))
                .map(|m| m.name.clone())
                .collect();
            let config_items = g
                .entries
                .iter()
                .filter(|(k, _)| k.starts_with(&format!("Configuration{i}_Item")))
                .count();
            out.push(Output {
                index: i,
                name: g.str(&format!("OutputName{i}")),
                output_type: t.to_owned(),
                category: g.str(&format!("OutputCategory{i}")),
                document: g.str(&format!("OutputDocumentPath{i}")),
                variant: g.str(&format!("OutputVariantName{i}")),
                enabled_for,
                config_items,
            });
        }
        out
    }

    /// Configuration records of one output, decoded from `Record=...|K=V` strings.
    pub fn output_config(&self, index: u32) -> Vec<IndexMap<String, String>> {
        let g = self.group();
        (1..)
            .map_while(|k| g.get(&format!("Configuration{index}_Item{k}")))
            .map(pipe_props)
            .collect()
    }

    /// Find an output by 1-based index, exact name, or type.
    pub fn find_output(&self, sel: &str) -> Result<Vec<Output>> {
        let all = self.outputs();
        if let Ok(n) = sel.parse::<u32>()
            && let Some(o) = all.iter().find(|o| o.index == n) {
                return Ok(vec![o.clone()]);
            }
        let by_name: Vec<Output> = all.iter().filter(|o| o.name.eq_ignore_ascii_case(sel)).cloned().collect();
        if !by_name.is_empty() {
            return Ok(by_name);
        }
        let by_type: Vec<Output> = all.iter().filter(|o| o.output_type.eq_ignore_ascii_case(sel)).cloned().collect();
        if !by_type.is_empty() {
            return Ok(by_type);
        }
        Err(Error::not_found(format!("no output {sel:?}"))
            .hint("select by 1-based index, output name or output type; see `ace outjob outputs`"))
    }

    pub fn find_medium(&self, sel: &str) -> Result<Medium> {
        let ms = self.mediums();
        let names: Vec<String> = ms.iter().map(|m| m.name.clone()).collect();
        ms.into_iter()
            .find(|m| m.name.eq_ignore_ascii_case(sel) || m.index.to_string() == sel)
            .ok_or_else(|| Error::not_found(format!("no output medium {sel:?}")).hint(format!("mediums: {names:?}")))
    }

    pub fn set_enabled(&mut self, output: u32, medium: u32, on: bool) {
        let g = self.ini.section_mut("OutputGroup1").expect("checked in load");
        g.set(&format!("OutputEnabled{output}_OutputMedium{medium}"), if on { "1" } else { "0" });
    }
}

// ---------------------------------------------------------------------------------------------
// Writing: a new OutJob from a list of standard outputs.

/// Standard outputs `ace designer export` (and `ace outjob create`) can generate. Settings
/// come from Altium's own templates (`OutputJobs\Fabrication.OutJob`, `Assembly.OutJob`), with
/// every possible layer enabled for Gerber (Altium skips layers the board does not use).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum OutputKind {
    /// RS-274X Gerber, one file per layer.
    Gerber,
    /// Gerber X2 (attributes, job file).
    GerberX2,
    /// Excellon NC drill files.
    Drill,
    /// ODB++ archive.
    Odb,
    /// IPC-2581 (rev B).
    Ipc2581,
    /// Pick-and-place (CSV and text).
    PickPlace,
    /// Bill of materials (Altium BOM_PartType report).
    Bom,
    /// Schematic sheets as PDF.
    SchPdf,
    /// PCB composite print as PDF.
    PcbPdf,
    /// PCB 3D view print as PDF.
    Pcb3dPdf,
    /// STEP 3D model of the board.
    Step,
}

pub const OUTPUT_KINDS: &[OutputKind] = &[
    OutputKind::Gerber,
    OutputKind::GerberX2,
    OutputKind::Drill,
    OutputKind::Odb,
    OutputKind::Ipc2581,
    OutputKind::PickPlace,
    OutputKind::Bom,
    OutputKind::SchPdf,
    OutputKind::PcbPdf,
    OutputKind::Pcb3dPdf,
    OutputKind::Step,
];

impl OutputKind {
    pub fn name(self) -> &'static str {
        match self {
            OutputKind::Gerber => "gerber",
            OutputKind::GerberX2 => "gerber-x2",
            OutputKind::Drill => "drill",
            OutputKind::Odb => "odb",
            OutputKind::Ipc2581 => "ipc2581",
            OutputKind::PickPlace => "pick-place",
            OutputKind::Bom => "bom",
            OutputKind::SchPdf => "sch-pdf",
            OutputKind::PcbPdf => "pcb-pdf",
            OutputKind::Pcb3dPdf => "pcb-3d-pdf",
            OutputKind::Step => "step",
        }
    }

    pub fn parse(s: &str) -> Result<OutputKind> {
        let k = s.trim().to_ascii_lowercase().replace('_', "-");
        let alias = match k.as_str() {
            "gerber-rs274x" | "rs274x" | "gerbers" => "gerber",
            "x2" => "gerber-x2",
            "nc-drill" | "ncdrill" | "excellon" => "drill",
            "odb++" | "odbpp" => "odb",
            "ipc-2581" | "ipc" => "ipc2581",
            "pnp" | "pick-and-place" | "centroid" => "pick-place",
            "schematic-pdf" | "schematic" => "sch-pdf",
            "pcb-print" => "pcb-pdf",
            "3d-pdf" | "pcb-3d" => "pcb-3d-pdf",
            other => other,
        };
        OUTPUT_KINDS.iter().copied().find(|o| o.name() == alias).ok_or_else(|| {
            Error::new(ErrorCode::Usage, format!("unknown output kind {s:?}"))
                .hint(format!("kinds: {}", OUTPUT_KINDS.iter().map(|o| o.name()).collect::<Vec<_>>().join(", ")))
        })
    }

    /// Altium's `OutputType` name.
    pub fn output_type(self) -> &'static str {
        match self {
            OutputKind::Gerber => "Gerber",
            OutputKind::GerberX2 => "Gerber X2",
            OutputKind::Drill => "NC Drill",
            OutputKind::Odb => "ODB",
            OutputKind::Ipc2581 => "IPC2581",
            OutputKind::PickPlace => "Pick Place",
            OutputKind::Bom => "BOM_PartType",
            OutputKind::SchPdf => "Schematic Print",
            OutputKind::PcbPdf => "PCB Print",
            OutputKind::Pcb3dPdf => "PCB 3D Print",
            OutputKind::Step => "ExportSTEP",
        }
    }

    pub fn category(self) -> &'static str {
        match self {
            OutputKind::Gerber | OutputKind::GerberX2 | OutputKind::Drill | OutputKind::Odb | OutputKind::Ipc2581 => "Fabrication",
            OutputKind::PickPlace => "Assembly",
            OutputKind::Bom => "Report",
            OutputKind::SchPdf | OutputKind::PcbPdf | OutputKind::Pcb3dPdf => "Documentation",
            OutputKind::Step => "Export",
        }
    }

    /// Printed outputs go to a PDF publisher medium; the rest to a generated-files folder.
    pub fn is_pdf(self) -> bool {
        matches!(self, OutputKind::SchPdf | OutputKind::PcbPdf | OutputKind::Pcb3dPdf)
    }

    /// Needs a PCB document (everything except the schematic print and the BOM).
    pub fn needs_pcb(self) -> bool {
        !matches!(self, OutputKind::SchPdf | OutputKind::Bom)
    }

    fn config(self) -> Vec<String> {
        match self {
            OutputKind::Gerber => vec![format!(
                "AddToAllLayerClasses.Set= |AddToAllPlots.Set=SerializeLayerHash.Version~2,ClassName~TLayerToBoolean|CentrePlots=False|DrillDrawingSymbol=GraphicsSymbol|DrillDrawingSymbolSize=500000|EmbeddedApertures=True|FilmBorderSize=10000000|FilmXSize=200000000|FilmYSize=160000000|FlashAllFills=False|FlashPadShapes=True|G54OnApertureChange=False|GenerateDRCRulesFile=True|GenerateReliefShapes=True|GerberUnit=Imperial|IncludeUnconnectedMidLayerPads=False|LayerClassesMirror.Set= |LayerClassesPlot.Set= |LeadingAndTrailingZeroesMode=SuppressLeadingZeroes|MaxApertureSize=2500000|MinusApertureTolerance=50|Mirror.Set=SerializeLayerHash.Version~2,ClassName~TLayerToBoolean|MirrorDrillDrawingPlots=False|MirrorDrillGuidePlots=False|NumberOfDecimals=5|OptimizeChangeLocationCommands=True|OriginPosition=Relative|Panelize=False|Plot.Set={}|PlotPositivePlaneLayers=False|PlotUsedDrillDrawingLayerPairs=False|PlotUsedDrillGuideLayerPairs=False|PlusApertureTolerance=50|Record=GerberView|SoftwareArcs=False|Sorted=False",
                layer_set(true)
            )],
            OutputKind::GerberX2 => vec![format!(
                "FileComment= |FileSubject=Autodetect|GenerateDRCRulesFile=True|GerberUnit=Metric|MinusApertureTolerance=39|NumberOfDecimals=5|OptimizeChangeLocationCommands=True|PlotBackDrillingPairs=False|PlotBlindViasPairs=False|PlotBoardProfile=True|PlotDrillDrawingPairs=False|PlotDrillGuidePairs=False|PlotNPTHPairs=False|PlotPTHPairs=False|PlotX2.Set={}|PlusApertureTolerance=39|Record=GerberX2View|Sorted=False",
                layer_set(false)
            )],
            OutputKind::Drill => vec!["BoardEdgeRoutToolDia=2000000|GenerateBoardEdgeRout=False|GenerateDrilledSlotsG85=False|GenerateEIADrillFile=False|GenerateSeparatePlatedNonPlatedFiles=True|NumberOfDecimals=5|NumberOfUnits=2|OptimizeChangeLocationCommands=True|OriginPosition=Relative|Record=DrillView|Units=Imperial|ZeroesMode=SuppressTrailingZeroes".into()],
            OutputKind::Odb => vec![],
            OutputKind::Ipc2581 => vec!["Record=IPC2581View|IPC2581Version=B|MeasurementSystem=Metric|FloatingPointPrecision=6|IsWiseSupport=False".into()],
            OutputKind::PickPlace => vec!["Record=PickPlaceView|Units=Metric|GenerateCSVFormat=True|GenerateTextFormat=False|ShowUnits=True|Separator=.|ExcludeFilterParam=False|IncludeVariations=True|Filter= |FilterActive=False|Column#1=Name:Designator,Fixed:True,Metric:False,Visible:True,Sort:None,Position:0|Column#2=Name:Comment,Fixed:True,Metric:False,Visible:True,Sort:None,Position:1|Column#3=Name:Layer,Fixed:True,Metric:False,Visible:True,Sort:None,Position:2|Column#4=Name:Footprint,Fixed:True,Metric:False,Visible:True,Sort:None,Position:3|Column#5=Name:Center-X,Fixed:True,Metric:True,Visible:True,Sort:None,Position:4|Column#6=Name:Center-Y,Fixed:True,Metric:True,Visible:True,Sort:None,Position:5|Column#7=Name:Rotation,Fixed:True,Metric:False,Visible:True,Sort:None,Position:6|Column#8=Name:Description,Fixed:True,Metric:False,Visible:True,Sort:None,Position:7|Column#9=Name:ComponentKind,Fixed:True,Metric:False,Visible:False,Sort:None,Position:8|Column#10=Name:Height,Fixed:True,Metric:True,Visible:False,Sort:None,Position:9|Column#11=Name:Variation,Fixed:True,Metric:False,Visible:False,Sort:None,Position:10".into()],
            OutputKind::Bom => vec![
                "OpenExported=False|AddToProject=False|ForceFit=False|NotFitted=False|Database=False|IncludePCBData=False|ShowExportOptions=False|TemplateFilename=|BatchMode=5|FormWidth=1185|FormHeight=665|SupplierProdQty=1|SupplierAutoQty=False|SupplierUseCachedPricing=False|SupplierCurrency=<none>".into(),
            ],
            OutputKind::SchPdf => vec!["Record=SchPrintView|ShowNoERC=True|ShowParamSet=True|ShowProbe=True|ShowBlanket=True|ShowNote=True|ShowNoteCollapsed=True|ShowOpenEnds=True|ExpandDesignator=True|ExpandNetLabel=False|ExpandPort=False|ExpandSheetNum=False|ExpandDocNum=False|PrintArea=0|PrintAreaRect.X1=0|PrintAreaRect.Y1=0|PrintAreaRect.X2=0|PrintAreaRect.Y2=0".into()],
            OutputKind::PcbPdf => {
                let mut v = vec![
                    "DesignatorDisplayMode=Physical|PrintArea=DesignExtent|PrintAreaLowerLeftCornerX=0|PrintAreaLowerLeftCornerY=0|PrintAreaUpperRightCornerX=0|PrintAreaUpperRightCornerY=0|Record=PcbPrintView".to_owned(),
                    "IncludeBottomLayerComponents=True|IncludeMultiLayerComponents=True|IncludeTopLayerComponents=True|IncludeViewports=True|Index=0|Mirror=False|Name=Multilayer Composite Print|PadNumberFontSize=14|Record=PcbPrintOut|ShowHoles=False|ShowPadNets=False|ShowPadNumbers=False|SubstituteFonts=False".to_owned(),
                ];
                for layer in ["TopOverlay", "TopLayer", "BottomLayer", "MultiLayer", "BottomOverlay", "KeepOutLayer", "Mechanical1"] {
                    v.push(format!("CArc=Full|CFill=Full|Comment=Full|Coordinate=Full|CPad=Full|CRegion=Full|CText=Full|CTrack=Full|CVia=Full|Designator=Full|Dimension=Full|DLayer1=TopLayer|DLayer2=BottomLayer|FArc=Full|FFill=Full|FPad=Full|FRegion=Full|FText=Full|FTrack=Full|FVia=Full|Layer={layer}|Polygon=Full|PrintOutIndex=0|Record=PcbPrintLayer"));
                }
                v
            }
            OutputKind::Pcb3dPdf => vec!["Record=Pcb3DPrintView|ResX=300|ResY=300|ViewX=0|ViewY=0|LookAtX=0|LookAtY=0|LookAtZ=0|QuatX=-0.291343718767166|QuatY=-0.308464288711548|QuatZ=0.118618682026863|QuatW=0.897718250751495|Zoom=0|UnitsPercent=True|UnitsDPI=True|LockResAspect=True|ViewConfigType=.config_3d|CustomCamera=False|ViewFromTop=True".into()],
            OutputKind::Step => vec!["Record=ExportSTEPView|ExportComponentOptions=0|ExportModelsOption=2|ExportHolesOption=0|CanSelectPrimitives=False|IncludeMechanicalPadHoles=False|IncludeElectricalPadHoles=False|IncludeFreePadHoles=False|ExportFoldedBoard=True|ExportFoldedBoardRate=100|ComponentSuffixType=0|ComponentSuffix= |ExportCopperOption=0|ExportCopperLayer=1|ExportPadAndViaBarrelsOnly=False|IgnoreBoardCopperLayerColors=False|ExportAsSinglePart=False".into()],
        }
    }

    fn page_options(self) -> Option<&'static str> {
        self.is_pdf().then_some("Record=PageOptions|CenterHorizontal=True|CenterVertical=True|PrintScale=1.00|XCorrection=1.00|YCorrection=1.00|PrintKind=1|BorderSize=5000000|LeftOffset=0|BottomOffset=0|Orientation=2|PaperLength=1000|PaperWidth=1000|Scale=100|PaperSource=7|PrintQuality=-3|MediaType=1|DitherType=10|PrintScaleMode=1|PaperKind=A4|PaperIndex=9")
    }
}

/// `SerializeLayerHash` with every signal, plane, mechanical, overlay, paste, mask, keep-out and
/// drill layer (V7 layer ids). Altium plots only the layers the board actually uses.
fn layer_set(with_drill: bool) -> String {
    let mut ids: Vec<u32> = vec![];
    ids.extend(0x0100_0001..=0x0100_001f); // top + mid layers
    ids.push(0x0100_ffff); // bottom
    ids.extend(0x0101_0001..=0x0101_0010); // internal planes
    ids.extend(0x0102_0001..=0x0102_0020); // mechanical 1..32
    ids.extend(0x0103_0006..=0x0103_000d); // overlays, paste, solder mask, drill guide, keep-out
    if with_drill {
        ids.extend([0x0103_0018, 0x0103_0019]); // drill drawing / guide
    }
    let mut s = "SerializeLayerHash.Version~2,ClassName~TLayerToBoolean".to_owned();
    for id in ids {
        s.push_str(&format!(",{id}~1"));
    }
    s
}

/// One output of a new OutJob.
#[derive(Debug, Clone)]
pub struct NewOutput {
    pub kind: OutputKind,
    /// Source document file name (e.g. `Board.PcbDoc`); empty = the project's (only) one,
    /// `[Project Physical Documents]` for schematic prints.
    pub document: String,
}

/// Where the generated files go (Altium-side absolute folder) and which variant to use.
#[derive(Debug, Clone)]
pub struct NewJob {
    pub outputs: Vec<NewOutput>,
    /// Folder for generated files and PDFs (absolute, as Altium sees it).
    pub out_dir: String,
    /// PDF file name (in `out_dir`) for the printed outputs.
    pub pdf_name: String,
    /// Variant name, or empty for no variations.
    pub variant: String,
}

/// Names of the two mediums a generated OutJob has.
pub const FILES_MEDIUM: &str = "altium-cli-extended files";
pub const PDF_MEDIUM: &str = "altium-cli-extended pdf";

impl NewJob {
    /// The `.OutJob` text (CRLF line ends).
    pub fn to_ini_text(&self) -> String {
        let mut l: Vec<String> = vec![
            "[OutputJobFile]".into(),
            "Version=1.0".into(),
            "Caption=altium-cli-extended export".into(),
            "Description=Generated by altium-cli-extended; safe to delete".into(),
            String::new(),
            "[OutputGroup1]".into(),
            "Name=".into(),
            "Description=".into(),
            format!("TargetOutputMedium={FILES_MEDIUM}"),
            format!("VariantName={}", if self.variant.is_empty() { "[No Variations]" } else { &self.variant }),
            "VariantScope=0".into(),
            "CurrentConfigurationName=".into(),
            "TargetPrinter=".into(),
            "PrinterOptions=Record=PrinterOptions|Copies=1|Duplex=1|TrueTypeOptions=3|Collate=1|PrintJobKind=1|PrintWhat=1".into(),
            format!("OutputMedium1={FILES_MEDIUM}"),
            "OutputMedium1_Type=GeneratedFiles".into(),
            format!("OutputMedium2={PDF_MEDIUM}"),
            "OutputMedium2_Type=Publish".into(),
        ];
        let (mut files_n, mut pdf_n) = (0, 0);
        for (i, o) in self.outputs.iter().enumerate() {
            let k = i + 1;
            let (m1, m2) = if o.kind.is_pdf() {
                pdf_n += 1;
                (0, pdf_n)
            } else {
                files_n += 1;
                (files_n, 0)
            };
            let doc = if o.document.is_empty() && o.kind == OutputKind::SchPdf { "[Project Physical Documents]".to_owned() } else { o.document.clone() };
            l.push(format!("OutputType{k}={}", o.kind.output_type()));
            l.push(format!("OutputName{k}={}", o.kind.name()));
            l.push(format!("OutputCategory{k}={}", o.kind.category()));
            l.push(format!("OutputDocumentPath{k}={doc}"));
            l.push(format!("OutputVariantName{k}="));
            l.push(format!("OutputEnabled{k}=1"));
            l.push(format!("OutputEnabled{k}_OutputMedium1={m1}"));
            l.push(format!("OutputEnabled{k}_OutputMedium2={m2}"));
            l.push(format!("OutputDefault{k}=0"));
            if let Some(po) = o.kind.page_options() {
                l.push(format!("PageOptions{k}={po}"));
            }
            for (j, c) in o.kind.config().iter().enumerate() {
                l.push(format!("Configuration{k}_Name{}=OutputConfigurationParameter{}", j + 1, j + 1));
                l.push(format!("Configuration{k}_Item{}={c}", j + 1));
            }
        }
        let dir = self.out_dir.trim_end_matches('\\');
        l.extend([
            String::new(),
            "[PublishSettings]".into(),
            format!("OutputFilePath2={dir}\\{}", self.pdf_name),
            "ReleaseManaged2=0".into(),
            format!("OutputBasePath2={dir}"),
            "OutputPathMedia2=".into(),
            "OutputPathMediaValue2=".into(),
            "OutputPathOutputer2=".into(),
            "OutputPathOutputerPrefix2=".into(),
            "OutputPathOutputerValue2=".into(),
            format!("OutputFileName2={}", self.pdf_name),
            "OutputFileNameMulti2=".into(),
            "UseOutputNameForMulti2=0".into(),
            "OutputFileNameSpecial2=".into(),
            "OpenOutput2=0".into(),
            "PromptOverwrite2=0".into(),
            "PublishMethod2=0".into(),
            "ZoomLevel2=50".into(),
            "FitSCHPrintSizeToDoc2=1".into(),
            "FitPCBPrintSizeToDoc2=1".into(),
            "GenerateNetsInfo2=1".into(),
            "MarkPins2=1".into(),
            "MarkNetLabels2=1".into(),
            "MarkPortsId2=1".into(),
            "GenerateTOC2=1".into(),
            "ShowComponentParameters2=1".into(),
            "GlobalBookmarks2=0".into(),
            "PDFACompliance2=Disabled".into(),
            "PDFVersion2=Default".into(),
            String::new(),
            "[GeneratedFilesSettings]".into(),
            format!("RelativeOutputPath1={dir}\\"),
            "OpenOutputs1=0".into(),
            "AddToProject1=0".into(),
            "TimestampFolder1=0".into(),
            "UseOutputName1=1".into(),
            "OpenODBOutput1=0".into(),
            "OpenGerberOutput1=0".into(),
            "OpenNCDrillOutput1=0".into(),
            "OpenIPCOutput1=0".into(),
            "EnableReload1=0".into(),
            String::new(),
        ]);
        l.join("\r\n")
    }

    /// Which mediums have outputs (files first).
    pub fn mediums(&self) -> Vec<&'static str> {
        let mut v = vec![];
        if self.outputs.iter().any(|o| !o.kind.is_pdf()) {
            v.push(FILES_MEDIUM);
        }
        if self.outputs.iter().any(|o| o.kind.is_pdf()) {
            v.push(PDF_MEDIUM);
        }
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_parse() {
        assert_eq!(OutputKind::parse("Gerber").unwrap(), OutputKind::Gerber);
        assert_eq!(OutputKind::parse("odb++").unwrap(), OutputKind::Odb);
        assert_eq!(OutputKind::parse("pnp").unwrap(), OutputKind::PickPlace);
        assert_eq!(OutputKind::parse("3d-pdf").unwrap(), OutputKind::Pcb3dPdf);
        assert!(OutputKind::parse("hologram").is_err());
        for k in OUTPUT_KINDS {
            assert_eq!(OutputKind::parse(k.name()).unwrap(), *k);
        }
    }

    #[test]
    fn new_job_round_trips_through_the_reader() {
        let job = NewJob {
            outputs: vec![
                NewOutput { kind: OutputKind::Gerber, document: "Board.PcbDoc".into() },
                NewOutput { kind: OutputKind::Drill, document: "Board.PcbDoc".into() },
                NewOutput { kind: OutputKind::SchPdf, document: String::new() },
            ],
            out_dir: r"C:\w\Project Outputs\altium-cli-extended".into(),
            pdf_name: "docs.pdf".into(),
            variant: "Lite".into(),
        };
        let text = job.to_ini_text();
        let dir = std::env::temp_dir().join(format!("oj-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("t.OutJob");
        std::fs::write(&p, &text).unwrap();
        let j = OutJob::load(&p).unwrap();
        let outs = j.outputs();
        assert_eq!(outs.len(), 3);
        assert_eq!(outs[0].output_type, "Gerber");
        assert_eq!(outs[0].enabled_for, vec![FILES_MEDIUM.to_owned()]);
        assert_eq!(outs[2].enabled_for, vec![PDF_MEDIUM.to_owned()]);
        assert_eq!(outs[2].document, "[Project Physical Documents]");
        assert_eq!(j.group_info()["VariantName"], "Lite");
        assert!(j.output_config(1)[0].get("Plot.Set").is_some_and(|s| s.contains("16842751~1")));
        assert_eq!(job.mediums(), vec![FILES_MEDIUM, PDF_MEDIUM]);
        let _ = std::fs::remove_dir_all(dir);
    }
}
