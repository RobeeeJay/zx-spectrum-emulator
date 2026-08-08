//! Finding ROMs when the working directory is not the project directory —
//! which is the normal case for a shipped app.

use std::path::{Path, PathBuf};
use zx_rustrum::prefs::Platform;
use zx_rustrum::resources::{find_file, resolve, search_dirs_from};

fn dirs(platform: Platform) -> Vec<PathBuf> {
    search_dirs_from(
        platform,
        Some(PathBuf::from("/work")),
        Some(PathBuf::from("/apps/Emulator.app/Contents/MacOS/emu")),
        Some(PathBuf::from("/home/u/.config/zx")),
    )
}

fn has(list: &[PathBuf], path: &str) -> bool {
    list.iter().any(|d| d == Path::new(path))
}

#[test]
fn the_working_directory_comes_first() {
    let d = dirs(Platform::MacOs);
    assert_eq!(d[0], Path::new("/work"));
    assert_eq!(d[1], Path::new("/work/roms"));
}

#[test]
fn roms_are_looked_for_beside_the_executable() {
    let d = dirs(Platform::MacOs);
    assert!(has(&d, "/apps/Emulator.app/Contents/MacOS"));
    assert!(has(&d, "/apps/Emulator.app/Contents/MacOS/roms"));
}

#[test]
fn a_mac_bundle_looks_in_its_resources_directory() {
    let d = dirs(Platform::MacOs);
    assert!(has(&d, "/apps/Emulator.app/Contents/Resources"));
    assert!(has(&d, "/apps/Emulator.app/Contents/Resources/roms"));
}

#[test]
fn other_platforms_do_not_invent_a_resources_directory() {
    for platform in [Platform::Windows, Platform::Unix] {
        let d = dirs(platform);
        assert!(
            !has(&d, "/apps/Emulator.app/Contents/Resources"),
            "{platform:?} should not look for a mac bundle layout"
        );
    }
}

#[test]
fn a_unix_install_looks_in_share() {
    let d = search_dirs_from(
        Platform::Unix,
        None,
        Some(PathBuf::from("/usr/local/bin/emu")),
        None,
    );
    assert!(has(&d, "/usr/local/share/zx-rustrum"));
    assert!(has(&d, "/usr/local/share/zx-rustrum/roms"));
}

#[test]
fn the_config_directory_is_searched_last() {
    let d = dirs(Platform::Unix);
    let config = d
        .iter()
        .position(|p| p == Path::new("/home/u/.config/zx"))
        .expect("config directory searched");
    let exe = d
        .iter()
        .position(|p| p == Path::new("/apps/Emulator.app/Contents/MacOS"))
        .expect("executable directory searched");
    assert!(config > exe, "ROMs beside the app beat ROMs in the config");
}

#[test]
fn a_missing_environment_does_not_produce_empty_paths() {
    let d = search_dirs_from(Platform::Windows, None, None, None);
    assert!(d.is_empty(), "nothing known means nothing to search");
}

#[test]
fn duplicate_directories_are_searched_once() {
    // Launched from the directory the binary is in, as an unzipped build is.
    let d = search_dirs_from(
        Platform::Unix,
        Some(PathBuf::from("/opt/emu")),
        Some(PathBuf::from("/opt/emu/emu")),
        None,
    );
    assert_eq!(d.iter().filter(|p| *p == Path::new("/opt/emu")).count(), 1);
}

/// The real point of all this: a ROM found without the working directory
/// having anything to do with it.
#[test]
fn a_rom_is_found_next_to_the_executable() {
    let tmp = tmp_dir("beside-exe");
    let bin = tmp.join("Emulator.app/Contents/MacOS");
    let res = tmp.join("Emulator.app/Contents/Resources/roms");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::create_dir_all(&res).unwrap();
    std::fs::write(res.join("48.rom"), vec![0xC9; 0x4000]).unwrap();

    let dirs = search_dirs_from(
        Platform::MacOs,
        Some(PathBuf::from("/")), // double-clicked: cwd is the root
        Some(bin.join("emu")),
        None,
    );
    let (path, data) = find_file(&dirs, &["48.rom", "48k.rom"], 0x4000).expect("ROM found");
    assert_eq!(path, res.join("48.rom"));
    assert_eq!(data.len(), 0x4000);
}

#[test]
fn a_short_file_is_not_mistaken_for_a_rom() {
    let tmp = tmp_dir("short-rom");
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(tmp.join("48.rom"), b"not a rom").unwrap();
    let dirs = vec![tmp.clone()];
    assert!(find_file(&dirs, &["48.rom"], 0x4000).is_none());
    // The ZX81's 8K image would fail a 16K floor, which is why it has its own.
    std::fs::write(tmp.join("zx81.rom"), vec![0; 0x2000]).unwrap();
    assert!(find_file(&dirs, &["zx81.rom"], 0x4000).is_none());
    assert!(find_file(&dirs, &["zx81.rom"], 0x2000).is_some());
}

#[test]
fn the_preferred_name_wins_over_the_nearer_directory() {
    let tmp = tmp_dir("name-order");
    let (near, far) = (tmp.join("near"), tmp.join("far"));
    std::fs::create_dir_all(&near).unwrap();
    std::fs::create_dir_all(&far).unwrap();
    std::fs::write(near.join("128k.rom"), vec![1; 0x8000]).unwrap();
    std::fs::write(far.join("128.rom"), vec![2; 0x8000]).unwrap();

    let dirs = vec![near, far.clone()];
    let (path, data) = find_file(&dirs, &["128.rom", "128k.rom"], 0x4000).unwrap();
    assert_eq!(path, far.join("128.rom"));
    assert_eq!(data[0], 2);
}

#[test]
fn a_relative_file_argument_resolves_against_the_search_path() {
    let tmp = tmp_dir("resolve");
    let tapes = tmp.join("tapes");
    std::fs::create_dir_all(&tapes).unwrap();
    std::fs::write(tapes.join("game.tap"), b"x").unwrap();

    let dirs = vec![PathBuf::from("/nowhere"), tmp.clone()];
    assert_eq!(
        resolve(&dirs, Path::new("tapes/game.tap")),
        tapes.join("game.tap")
    );
    // An absolute path, and a name that matches nothing, are left alone so the
    // error message names what the user actually typed.
    let abs = tmp.join("absolute.tap");
    assert_eq!(resolve(&dirs, &abs), abs);
    assert_eq!(
        resolve(&dirs, Path::new("missing.tap")),
        Path::new("missing.tap")
    );
}

fn tmp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("zx-resources-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
