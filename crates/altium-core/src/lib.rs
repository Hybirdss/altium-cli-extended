//! Foundation shared by every altium crate: the typed error every layer returns and the
//! length units every coordinate is stored in. Nothing here knows about files or the CLI.

pub mod error;
pub mod units;

pub use error::{Error, ErrorCode, Result};
pub use units::{Coord, Point, Unit};
