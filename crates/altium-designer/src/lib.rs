//! Bridge to a real Altium Designer, wherever it runs.
//!
//! - [`host`]: the [`host::Host`] interface everything else uses.
//!   - [`local`]: Altium on this Windows machine (default on Windows).
//!   - [`ssh`]: Altium on another Windows machine over SSH.
//!   - [`wine`] (+ [`install`], [`runtime`]): a Wine prefix on Linux (Bottles or plain Wine).
//! - [`bridge`]: the file-based request/response protocol with `assets/Bridge.pas`.
//! - [`dialogs`]: modal dialogs Altium is waiting on (detected, reported, dismissed).
//! - [`ops`]: [`ops::Designer`], the typed operations the CLI uses.
//! - [`setup`]: installation guide per host (`ace designer setup`).
//! - [`paths`], [`config`]: helpers.

pub mod bridge;
pub mod config;
pub mod dialogs;
pub mod host;
pub mod install;
pub mod local;
pub mod ops;
pub mod paths;
pub mod runtime;
mod security;
pub mod setup;
pub mod ssh;
pub mod wine;

use serde_json::{Value, json};

/// Readiness report for `ace doctor`: the host choice and its setup steps.
pub fn doctor() -> Value {
    let cfg = match config::Config::load() {
        Ok(c) => c,
        Err(e) => return json!({"ready": false, "config_error": e}),
    };
    let host = ops::host_choice(&cfg);
    let steps = setup::steps(&cfg);
    let ready = steps.iter().filter(|s| s.id != "license").all(|s| s.done || s.id == "bridge");
    let next = steps.iter().find(|s| !s.done && s.id != "license").map(|s| s.id);
    json!({
        "config": config::path(),
        "host": host,
        "ready": ready,
        "next": next,
        "steps": steps,
    })
}
