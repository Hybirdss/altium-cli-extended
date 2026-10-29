//! Running programs inside the installation's Wine environment, and finding its processes.

use std::path::Path;
use std::process::{Command, Stdio};

use altium_core::{Error, ErrorCode, Result};

use crate::install::{BOTTLES_APP, Installation, Runtime};

/// A `wine <args>` command with the same environment Bottles itself uses for the bottle, so
/// the altium-on-linux fixes (LD_PRELOAD shims, Vulkan layer, env vars) apply.
pub fn wine(inst: &Installation, args: &[String]) -> Command {
    match &inst.runtime {
        Runtime::Bottles { bottle, runner_dir, env, sync, .. } => {
            let rd = runner_dir.display();
            let mut vars: Vec<String> = vec![
                format!("WINEPREFIX={}", inst.prefix.display()),
                "WINEARCH=win64".into(),
                format!("BOTTLE={bottle}"),
                "WINE_LARGE_ADDRESS_AWARE=1".into(),
                "STAGING_SHARED_MEMORY=1".into(),
                "WINEDLLOVERRIDES=winemenubuilder=".into(),
                format!("LD_LIBRARY_PATH={rd}/lib:{rd}/lib/wine/x86_64-unix:{rd}/lib/wine/i386-unix"),
                "WINEDEBUG=-all".into(),
                "DXVK_LOG_LEVEL=none".into(),
            ];
            match sync.as_str() {
                "esync" => vars.push("WINEESYNC=1".into()),
                "fsync" => vars.push("WINEFSYNC=1".into()),
                "ntsync" | "wine" if Path::new("/dev/ntsync").exists() => vars.push("WINENTSYNC=1".into()),
                "ntsync" => vars.push("WINEFSYNC=1".into()),
                _ => {}
            }
            for (k, v) in env {
                vars.push(format!("{k}={v}"));
            }
            let mut c = Command::new("flatpak");
            c.args(["run", "--command=bash", BOTTLES_APP, "-c", "cd \"$1\" && shift && exec env \"$@\"", "_"]);
            c.arg(&inst.altium_dir);
            c.args(vars);
            c.arg(runner_dir.join("bin/wine"));
            c.args(args);
            c
        }
        Runtime::Wine { wine } => {
            let mut c = Command::new(wine);
            c.current_dir(&inst.altium_dir);
            c.env("WINEPREFIX", &inst.prefix).env("WINEDEBUG", "-all");
            c.args(args);
            c
        }
    }
}

/// Start X2.EXE with `args` detached (stdin closed, output appended to `log`). Returns
/// immediately; Altium keeps running after this process tree's launcher exits.
pub fn spawn_x2(inst: &Installation, args: &[String], log: &Path) -> Result<()> {
    let mut a = vec![
        "start".to_owned(),
        "/d".to_owned(),
        inst.altium_dir_windows(),
        inst.x2_windows(),
    ];
    a.extend(args.iter().cloned());
    let out = std::fs::OpenOptions::new().create(true).append(true).open(log)?;
    let err = out.try_clone()?;
    wine(inst, &a)
        .stdin(Stdio::null())
        .stdout(Stdio::from(out))
        .stderr(Stdio::from(err))
        .spawn()
        .map_err(|e| {
            Error::new(ErrorCode::DesignerUnavailable, format!("cannot launch Altium: {e}"))
                .hint("check `ace doctor`")
        })?;
    Ok(())
}

/// PIDs of host processes whose WINEPREFIX is this installation's prefix.
pub fn prefix_pids(inst: &Installation) -> Vec<u32> {
    let want = format!("WINEPREFIX={}", inst.prefix.display());
    let mut out = vec![];
    let Ok(rd) = std::fs::read_dir("/proc") else { return out };
    for e in rd.flatten() {
        let Some(pid) = e.file_name().to_str().and_then(|s| s.parse::<u32>().ok()) else { continue };
        let Ok(env) = std::fs::read(e.path().join("environ")) else { continue };
        if env.split(|&b| b == 0).any(|v| v == want.as_bytes()) {
            out.push(pid);
        }
    }
    out
}

/// PID of the running X2.EXE (argv[0] ends with `\X2.EXE`), if any.
pub fn altium_pid(inst: &Installation) -> Option<u32> {
    prefix_pids(inst).into_iter().find(|pid| {
        std::fs::read(format!("/proc/{pid}/cmdline"))
            .ok()
            .and_then(|c| c.split(|&b| b == 0).next().map(|a| String::from_utf8_lossy(a).to_ascii_lowercase()))
            .is_some_and(|argv0| argv0.ends_with("\\x2.exe") || argv0.ends_with("/x2.exe"))
    })
}

/// Terminate every process of the prefix (Altium, wineserver, helpers). Last resort; unsaved
/// work is lost.
pub fn kill_prefix(inst: &Installation) -> usize {
    let pids = prefix_pids(inst);
    let me = std::process::id();
    let mut n = 0;
    for pid in pids.into_iter().filter(|p| *p != me) {
        if Command::new("kill").arg("-TERM").arg(pid.to_string()).status().is_ok_and(|s| s.success()) {
            n += 1;
        }
    }
    n
}
