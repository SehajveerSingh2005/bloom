//! Linux Mint Cinnamon/X11 implementation.
//!
//! Keep desktop-specific integrations in focused modules so a future Wayland
//! backend can share the command contract without inheriting X11 assumptions.

pub mod apps;
pub mod audio;
pub mod bluetooth;
pub mod icons;
pub mod media;
pub mod network;
pub mod power;
pub mod system;
pub mod theme;
pub mod thumbnails;
pub mod visibility;
pub mod windows_manager;
pub mod x11;

use std::{env, path::PathBuf};

/// Milliseconds since the Unix epoch, used for ordering and cache stamps.
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

/// Locate a program on `PATH` without spawning anything.
pub fn find_in_path(program: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    env::split_paths(&path)
        .map(|directory| directory.join(program))
        .find(|candidate| candidate.is_file())
}

/// Launch the first available tool from a list of candidates, without a shell.
/// Returns whether anything was started.
pub fn launch_first(candidates: &[(&str, &[&str])]) -> bool {
    for (program, args) in candidates {
        let Some(path) = find_in_path(program) else {
            continue;
        };
        if std::process::Command::new(path).args(*args).spawn().is_ok() {
            return true;
        }
    }
    false
}
