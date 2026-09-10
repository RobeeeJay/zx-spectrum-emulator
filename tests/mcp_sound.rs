//! What `sound_state` says, once there is more than a beeper and an AY to say
//! it about.

use zx_rustrum::mcp::json::Json;
use zx_rustrum::mcp::tools::{Reply, Session};

fn call<const N: usize>(
    session: &mut Session,
    name: &str,
    args: [(&str, Json); N],
) -> Result<String, String> {
    session
        .call(name, &Json::obj(args))
        .map(|reply| match reply {
            Reply::Text(text) => text,
            Reply::Picture { text, .. } => text,
        })
}

/// A 48K with a Fuller Box has a sound chip, and its registers are worth
/// reading: that is what the box was bought for.
///
/// `sound_state` used to stop at "this machine has no sound chip" on anything
/// without one of its own, which is exactly the machine somebody would fit a
/// Fuller to.
#[test]
fn a_fuller_box_gives_a_48k_registers_to_read() {
    let mut session = Session::new();
    let bare = call(&mut session, "sound_state", []).expect("state");
    assert!(
        bare.contains("no sound chip of its own"),
        "a bare 48K has none:\n{bare}"
    );

    call(&mut session, "fit", [("what", Json::str("fuller"))]).expect("fitted");
    let out = call(&mut session, "sound_state", []).expect("state");
    assert!(
        out.contains("Fuller Box's"),
        "and now there is a chip to read:\n{out}"
    );
    assert!(out.contains("$3F selects"), "on its own ports:\n{out}");
    assert!(out.contains("channel A"), "with its channels:\n{out}");
}

/// The add-ons that are neither beeper nor chip say what they are doing.
#[test]
fn the_specdrum_and_the_speech_chip_are_reported() {
    let mut session = Session::new();
    call(&mut session, "fit", [("what", Json::str("specdrum"))]).expect("fitted");
    let out = call(&mut session, "sound_state", []).expect("state");
    assert!(out.contains("SpecDrum"), "{out}");
    assert!(out.contains("$DF"), "with the port to watch:\n{out}");

    call(&mut session, "fit", [("what", Json::str("uspeech"))]).expect("fitted");
    let out = call(&mut session, "sound_state", []).expect("state");
    assert!(out.contains("µSpeech"), "{out}");
    assert!(
        out.contains("allophones said"),
        "and what it has been saying:\n{out}"
    );
}

/// Each part of the sound can be silenced on its own, and the state says so.
#[test]
fn each_part_of_the_sound_can_be_silenced() {
    let mut session = Session::new();
    let out = call(&mut session, "set_sound", [("beeper", Json::Bool(false))]).expect("muted");
    assert!(out.contains("beeper muted"), "{out}");
    assert!(!session.spec.bus.audio.beeper_on);
    assert!(
        session.spec.bus.audio.ay_on && session.spec.bus.audio.hardware_on,
        "and the others are left alone"
    );

    let state = call(&mut session, "sound_state", []).expect("state");
    assert!(state.contains("beeper MUTED"), "{state}");

    call(
        &mut session,
        "set_sound",
        [
            ("beeper", Json::Bool(true)),
            ("hardware", Json::Bool(false)),
        ],
    )
    .expect("changed");
    assert!(session.spec.bus.audio.beeper_on);
    assert!(!session.spec.bus.audio.hardware_on);
}

/// Asking for nothing is an error that says what could be asked for.
#[test]
fn set_sound_with_nothing_to_set_says_what_it_takes() {
    let mut session = Session::new();
    let err = call(&mut session, "set_sound", []).expect_err("nothing asked");
    assert!(err.contains("beeper"), "{err}");
    assert!(err.contains("hardware"), "{err}");
}
