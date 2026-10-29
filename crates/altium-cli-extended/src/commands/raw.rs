//! `ace raw ...` — low-level access to any compound document: streams and records.
//! The escape hatch when no dedicated command decodes what an agent needs.

use std::path::PathBuf;

use clap::Subcommand;
use serde_json::{Value, json};

use altium_core::error::Result;
use altium_format::cfbx::{Compound, split_records};

use crate::ctx::Ctx;
use crate::meta::{Backend::*, CmdMeta, Effect::*, m};
use crate::output::{Out, list, list_with};

#[derive(Subcommand, Debug)]
pub enum RawCmd {
    /// List every stream in a compound document (.SchDoc/.SchLib/.PcbDoc/.PcbLib/.IntLib).
    Streams { file: PathBuf },
    /// Decode a stream as length-prefixed records (text records shown as property maps).
    Records {
        file: PathBuf,
        /// Stream path, e.g. `FileHeader`, `Board6/Data`, `Nets6/Data`.
        stream: String,
        /// Show binary records as hex (otherwise just their size).
        #[arg(long)]
        hex: bool,
    },
    /// Copy one stream's bytes to a file (or hex dump it when no --out is given).
    Dump {
        file: PathBuf,
        stream: String,
        /// Write bytes here instead of printing hex.
        #[arg(long, short)]
        out: Option<PathBuf>,
        /// Hex dump at most this many bytes.
        #[arg(long, default_value_t = 512)]
        max: usize,
    },
}


pub const META: &[CmdMeta] = &[
    m("raw streams", Native, Read, "{count, items:[{path, size}]}", "ace raw streams Board.PcbDoc",
      "Escape hatch: see every stream even if no dedicated command decodes it."),
    m("raw records", Native, Read,
      "{stream, count, leftover_bytes, items:[{index, kind, size, props?|hex?}]}",
      "ace raw records Sheet1.SchDoc FileHeader --where props.RECORD=1", ""),
    m("raw dump", Native, Export, "{stream, size, out?|hex?}", "ace raw dump Board.PcbDoc Board6/Data --out board6.bin", ""),
];

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect::<Vec<_>>().join(" ")
}

pub fn run(c: RawCmd, ctx: &Ctx) -> Result<Out> {
    match c {
        RawCmd::Streams { file } => {
            let cf = Compound::open(&ctx.input(&file)?)?;
            Out::new(list(&cf.streams())?)
        }
        RawCmd::Records { file, stream, hex: show_hex } => {
            let mut cf = Compound::open(&ctx.input(&file)?)?;
            let buf = cf.read(&stream)?;
            let (recs, left) = split_records(&buf);
            let items: Vec<Value> = recs
                .iter()
                .enumerate()
                .map(|(i, r)| {
                    let mut o = json!({"index": i, "kind": if r.is_text() {"text"} else {"binary"}, "size": r.data.len()});
                    if r.is_text() && r.data.contains(&b'=') {
                        o["props"] = r.props().to_json();
                    } else if show_hex {
                        o["hex"] = json!(hex(&r.data));
                    }
                    o
                })
                .collect();
            let mut out = Out::new(list_with(&items, json!({"stream": stream, "leftover_bytes": left}))?)?;
            if left > 0 {
                out = out.warn(format!("{left} trailing bytes do not form a complete record; this stream may not be record-framed (try `raw dump`)"));
            }
            Ok(out)
        }
        RawCmd::Dump { file, stream, out, max } => {
            let mut cf = Compound::open(&ctx.input(&file)?)?;
            let buf = cf.read(&stream)?;
            if let Some(o) = out {
                ctx.export_file(&o, &buf)?;
                Out::new(json!({"stream": stream, "size": buf.len(), "out": o}))
            } else {
                Out::new(json!({"stream": stream, "size": buf.len(), "hex": hex(&buf[..buf.len().min(max)]), "truncated": buf.len() > max}))
            }
        }
    }
}

