//! The peripherals, over the MCP server: what is on the back, fitting it, and
//! the one button any of them has.

use zx_rustrum::mcp::json::Json;
use zx_rustrum::mcp::tools::Session;

fn call<const N: usize>(
    session: &mut Session,
    name: &str,
    args: [(&str, Json); N],
) -> Result<String, String> {
    let args = Json::obj(args);
    session.call(name, &args).map(|reply| match reply {
        zx_rustrum::mcp::tools::Reply::Text(text) => text,
        zx_rustrum::mcp::tools::Reply::Picture { text, .. } => text,
    })
}

/// The list says what is fitted and how far each thing is emulated, which is
/// the distinction the whole peripherals list exists to make.
#[test]
fn the_hardware_list_says_what_is_fitted_and_what_is_emulated() {
    let mut session = Session::new();
    let out = call(&mut session, "hardware", []).expect("a list");
    for name in [
        "Interface 1",
        "Currah µSpeech",
        "Multiface One",
        "Fuller Audio Box",
        "Cheetah SpecDrum",
    ] {
        assert!(out.contains(name), "{name} should be on the list:\n{out}");
    }
    assert!(out.contains("not fitted"), "and nothing is on yet:\n{out}");
    assert!(
        out.contains("not emulated"),
        "and the ones that do nothing say so:\n{out}"
    );
}

/// Fitting something puts it on the machine, and taking it off takes it away.
#[test]
fn fitting_an_interface_1_gives_the_machine_microdrives() {
    let mut session = Session::new();
    let out = call(
        &mut session,
        "fit",
        [("what", Json::str("if1")), ("microdrives", Json::num(4.0))],
    )
    .expect("fitted");
    assert!(out.contains("Interface 1 fitted"), "{out}");
    assert_eq!(
        session.spec.bus.if1.as_ref().map(|i| i.drive_count()),
        Some(4),
        "four drives on the chain"
    );

    let out = call(
        &mut session,
        "fit",
        [("what", Json::str("if1")), ("fitted", Json::Bool(false))],
    )
    .expect("unfitted");
    assert!(out.contains("taken off"), "{out}");
    assert!(session.spec.bus.if1.is_none(), "and the drives go with it");
}

/// A peripheral whose ROM is not there is still fitted, and says what is
/// missing: a machine that ignores a box looks the same as a broken one.
#[test]
fn a_box_with_no_rom_is_fitted_and_says_so() {
    let mut session = Session::new();
    // Nowhere to find a ROM, whatever is on this machine.
    session.rom_dirs = vec![std::path::PathBuf::from("/nonexistent")];
    let out = call(&mut session, "fit", [("what", Json::str("multiface1"))]).expect("fitted");
    assert!(
        out.contains("multiface1.rom"),
        "it should name the ROM it wanted:\n{out}"
    );
    assert_eq!(session.spec.bus.multifaces.len(), 1, "and it is still on");
}

/// An unknown name is answered with the names that would have worked.
#[test]
fn asking_for_something_that_does_not_exist_says_what_does() {
    let mut session = Session::new();
    let err = call(&mut session, "fit", [("what", Json::str("kempston"))])
        .expect_err("no such peripheral");
    assert!(err.contains("if1"), "{err}");
    assert!(err.contains("specdrum"), "{err}");
}

/// The red button says what to do when there is nothing to press.
#[test]
fn the_red_button_says_when_there_is_no_multiface() {
    let mut session = Session::new();
    let err = call(&mut session, "red_button", []).expect_err("nothing fitted");
    assert!(err.contains("fit"), "it should say how to get one: {err}");

    // Fitted but with no ROM, the button still does nothing — and says which
    // of the two it is.
    session.rom_dirs = vec![std::path::PathBuf::from("/nonexistent")];
    call(&mut session, "fit", [("what", Json::str("multiface128"))]).expect("fitted");
    let err = call(&mut session, "red_button", []).expect_err("no ROM");
    assert!(err.contains("ROM"), "{err}");
}

/// With a ROM in it, the button stops the machine: the NMI is pending and the
/// interface pages in when the machine reaches $0066.
#[test]
fn the_red_button_stops_the_machine() {
    let mut session = Session::new();
    if rom_missing("multiface1.rom") {
        eprintln!("need roms/multiface1.rom or mf1.rom; skipping");
        return;
    }
    call(&mut session, "fit", [("what", Json::str("multiface1"))]).expect("fitted");
    let out = call(&mut session, "red_button", []).expect("pressed");
    assert!(out.contains("$0066"), "{out}");
    assert!(
        session.spec.bus.nmi_pending || session.spec.bus.multifaces[0].pressed,
        "the machine should have an NMI waiting for it"
    );
}

fn rom_missing(name: &str) -> bool {
    let alternatives = [name, "mf1.rom"];
    !alternatives
        .iter()
        .any(|n| std::path::Path::new("roms").join(n).exists())
}

/// machine_info says what is on the back, because it changes what the machine
/// can do: a microdrive command on a machine with no Interface 1 is an error
/// rather than a mystery.
#[test]
fn machine_info_says_what_is_plugged_in() {
    let mut session = Session::new();
    let bare = call(&mut session, "machine_info", []).expect("info");
    assert!(bare.contains("nothing plugged into the back"), "{bare}");

    call(&mut session, "fit", [("what", Json::str("specdrum"))]).expect("fitted");
    let after = call(&mut session, "machine_info", []).expect("info");
    assert!(
        after.contains("Cheetah SpecDrum"),
        "and now it should say so:\n{after}"
    );
}

// ---- the microdrives -------------------------------------------------------

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join("zx-rustrum-mcp-tests");
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    dir.join(name)
}

/// Without an Interface 1 there are no microdrives, and the error says how to
/// get one rather than reporting an empty drive.
#[test]
fn the_microdrive_tools_say_when_there_is_no_interface() {
    let mut session = Session::new();
    for tool in [
        "microdrive_info",
        "cartridge_catalogue",
        "eject_cartridge",
        "new_cartridge",
    ] {
        let err = call(&mut session, tool, []).expect_err("no interface");
        assert!(err.contains("fit") && err.contains("if1"), "{tool}: {err}");
    }
}

/// A blank cartridge goes in, comes back in the catalogue, and can be written
/// out and mounted again.
#[test]
fn a_cartridge_is_made_written_out_and_put_back() {
    let mut session = Session::new();
    call(
        &mut session,
        "fit",
        [("what", Json::str("if1")), ("microdrives", Json::num(2.0))],
    )
    .expect("fitted");

    let path = scratch("made.mdr");
    let out = call(
        &mut session,
        "new_cartridge",
        [
            ("drive", Json::num(2.0)),
            ("name", Json::str("TESTCART")),
            ("sectors", Json::num(120.0)),
            ("path", Json::str(path.display().to_string())),
        ],
    )
    .expect("a blank cartridge");
    assert!(out.contains("Drive 2"), "{out}");
    assert!(path.exists(), "and it was written out");

    let cat = call(
        &mut session,
        "cartridge_catalogue",
        [("drive", Json::num(2.0))],
    )
    .expect("a catalogue");
    assert!(cat.contains("TESTCART"), "{cat}");
    assert!(
        cat.contains("Nothing on it"),
        "a blank one is empty:\n{cat}"
    );
    assert!(cat.contains("Every sector adds up"), "{cat}");

    let info = call(&mut session, "microdrive_info", []).expect("the chain");
    assert!(info.contains("drive 1: empty"), "{info}");
    assert!(info.contains("TESTCART"), "{info}");

    // Out and back in again, read-only this time.
    call(&mut session, "eject_cartridge", [("drive", Json::num(2.0))]).expect("ejected");
    let err = call(
        &mut session,
        "cartridge_catalogue",
        [("drive", Json::num(2.0))],
    )
    .expect_err("empty now");
    assert!(err.contains("empty"), "{err}");

    let out = call(
        &mut session,
        "mount_cartridge",
        [
            ("path", Json::str(path.display().to_string())),
            ("drive", Json::num(1.0)),
        ],
    )
    .expect("mounted");
    assert!(out.contains("Read-only"), "read-only unless asked: {out}");
    assert!(out.contains("TESTCART"), "{out}");
}

/// A drive that is not on the chain is an error that says how long the chain
/// is, rather than a panic on an index.
#[test]
fn asking_for_a_drive_that_is_not_there_says_how_many_there_are() {
    let mut session = Session::new();
    call(&mut session, "fit", [("what", Json::str("if1"))]).expect("fitted");
    let err = call(&mut session, "microdrive_info", []).expect("info");
    assert!(err.contains("1 microdrive"), "{err}");

    let err = call(
        &mut session,
        "cartridge_catalogue",
        [("drive", Json::num(5.0))],
    )
    .expect_err("no drive 5");
    assert!(err.contains("chain has 1"), "{err}");
}
