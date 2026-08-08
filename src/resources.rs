//! Finding ROM images regardless of how the emulator was started.
//!
//! Run from a shell during development the working directory is the project,
//! so `roms/48.rom` just works. Run from a double-clicked macOS `.app` the
//! working directory is `/`, and from a Windows shortcut it is whatever the
//! shortcut says — so a shipped build has to look next to itself and in the
//! places a user would reasonably put ROMs instead.

use crate::prefs::Platform;
use std::path::{Path, PathBuf};

/// Directories to search for ROMs, in preference order, given the pieces of
/// the environment that vary. Each is tried both directly and with a `roms`
/// subdirectory, so `<exe>/48.rom` and `<exe>/roms/48.rom` both work.
///
/// * the working directory — a `cargo run` or a shell launch
/// * beside the executable — a portable unzip-and-run build
/// * `../Resources` from the executable — inside a macOS `.app` bundle, where
///   the binary sits in `Contents/MacOS`
/// * `../share/zx-rustrum` — a Unix `bin`/`share` install
/// * the config directory — where a user can drop ROMs once and forget
pub fn search_dirs_from(
    platform: Platform,
    cwd: Option<PathBuf>,
    exe: Option<PathBuf>,
    config: Option<PathBuf>,
) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let mut push = |d: PathBuf| {
        if !dirs.contains(&d) {
            dirs.push(d);
        }
    };

    if let Some(cwd) = cwd {
        push(cwd);
    }
    // `current_exe` follows symlinks, which is what we want: the ROMs live
    // beside the real binary, not beside a link on someone's PATH.
    if let Some(exe) = exe {
        if let Some(bin) = exe.parent() {
            push(bin.to_path_buf());
            if let Some(up) = bin.parent() {
                if platform == Platform::MacOs {
                    push(up.join("Resources"));
                }
                push(up.join("share").join("zx-rustrum"));
            }
        }
    }
    if let Some(config) = config {
        push(config);
    }

    // Expand each base into itself plus its `roms` subdirectory.
    dirs.iter()
        .flat_map(|d| [d.clone(), d.join("roms")])
        .collect()
}

pub fn search_dirs() -> Vec<PathBuf> {
    search_dirs_from(
        Platform::current(),
        std::env::current_dir().ok(),
        std::env::current_exe().ok(),
        crate::prefs::config_dir(),
    )
}

/// The first readable file matching one of `names` in one of `dirs` that is at
/// least `min` bytes. The size floor rejects truncated or placeholder files,
/// which would otherwise boot into a machine that hangs.
pub fn find_file(dirs: &[PathBuf], names: &[&str], min: usize) -> Option<(PathBuf, Vec<u8>)> {
    // Names outrank directories: a `128.rom` in the working directory should
    // not lose to a `128k.rom` sitting beside the executable.
    for name in names {
        for dir in dirs {
            let path = dir.join(name);
            if let Ok(data) = std::fs::read(&path) {
                if data.len() >= min {
                    return Some((path, data));
                }
            }
        }
    }
    None
}

/// Absolute paths are used as given; anything else is resolved against the
/// search directories, so `--zx81 zx81.rom` works from a bundle too.
pub fn resolve(dirs: &[PathBuf], path: &Path) -> PathBuf {
    if path.is_absolute() || path.exists() {
        return path.to_path_buf();
    }
    for dir in dirs {
        let candidate = dir.join(path);
        if candidate.exists() {
            return candidate;
        }
    }
    path.to_path_buf()
}
