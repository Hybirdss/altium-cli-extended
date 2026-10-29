//! One module per command group. Each exposes a clap `Subcommand` enum, `run`, and `META`
//! (one [`crate::meta::CmdMeta`] row per leaf command).

pub mod analysis;
pub mod designer;
pub mod meta;
pub mod outjob;
pub mod pcb;
pub mod pcblib;
pub mod project;
pub mod raw;
pub mod sch;
pub mod schlib;
pub mod skill;
