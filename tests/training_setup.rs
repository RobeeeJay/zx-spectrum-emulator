//! A training set-up as a file: what is written is what is read back, a file
//! that only says what differs is filled in from the defaults, and a line
//! that cannot be read is named rather than skipped.

use zx_rustrum::joystick::Kind;
use zx_rustrum::training::inputs::InputSet;
use zx_rustrum::training::judge::Number;
use zx_rustrum::training::setup::Setup;
use zx_rustrum::training::sight::Area;

/// A set-up with nothing left at its default comes back unchanged: every
/// field is in the file, so a network saved with it can be rebuilt exactly.
#[test]
fn a_set_up_is_read_back_as_it_was_written() {
    let mut setup = Setup::default();
    let env = &mut setup.env;
    env.inputs = InputSet::keys(&["Q", "A", "SPACE"]).unwrap();
    env.sight.area = Area::Television;
    env.sight.shrink = 4;
    env.sight.colour = true;
    env.sight.frames = 3;
    env.frames_per_step = 6;
    env.random_wait = 12;
    env.judge.score = Some(Number::Bcd {
        at: 0x9C4E,
        bytes: 3,
    });
    env.judge.score_scale = 0.05;
    env.judge.lives = Some(Number::Digits {
        at: 0x5A00,
        count: 1,
        zero: b'0',
    });
    env.judge.life_penalty = 2.5;
    env.judge.over_at_lives = None;
    env.judge.over_when = Some((0x8123, 0xFF));
    env.judge.per_step = 0.001;
    env.judge.max_steps = 4500;
    let ppo = &mut setup.ppo;
    ppo.games = 32;
    ppo.steps = 64;
    ppo.learning_rate = 3e-4;
    ppo.gamma = 0.97;
    ppo.lambda = 0.9;
    ppo.clip = 0.2;
    ppo.epochs = 3;
    ppo.minibatch = 512;
    ppo.value_weight = 0.25;
    ppo.entropy_weight = 0.02;
    ppo.seed = 99;

    let text = setup.to_text();
    assert_eq!(Setup::from_text(&text), Ok(setup.clone()), "{text}");

    let stick = Setup::default();
    assert_eq!(Setup::from_text(&stick.to_text()), Ok(stick));
}

/// Each way a number can be kept, written and read back.
#[test]
fn every_kind_of_number_is_written_and_read() {
    for n in [
        Number::Byte(0x9000),
        Number::Word(0xFFFE),
        Number::Bcd {
            at: 0x4000,
            bytes: 2,
        },
        Number::Digits {
            at: 0x9200,
            count: 6,
            zero: 0x30,
        },
    ] {
        assert_eq!(Number::from_text(&n.to_text()), Ok(n), "{}", n.to_text());
    }
    assert_eq!(Number::from_text("byte $9C4E"), Ok(Number::Byte(0x9C4E)));
}

/// A file written by hand need only say what differs.
#[test]
fn a_short_file_takes_the_defaults_for_the_rest() {
    let setup = Setup::from_text(
        "# Manic Miner\n\
         inputs = none; nothing; key:O; key:P; key:SPACE; key:O+key:SPACE; key:P+key:SPACE\n\
         score = digits 8429 6 30\n\
         lives = byte 8457\n",
    )
    .unwrap();
    let defaults = Setup::default();
    assert_eq!(setup.env.inputs.interface, Kind::None);
    assert_eq!(
        setup
            .env
            .inputs
            .actions
            .iter()
            .map(|a| a.name.as_str())
            .collect::<Vec<_>>(),
        ["nothing", "O", "P", "SPACE", "O+SPACE", "P+SPACE"]
    );
    assert_eq!(
        setup.env.judge.score,
        Some(Number::Digits {
            at: 0x8429,
            count: 6,
            zero: 0x30
        })
    );
    assert_eq!(setup.env.judge.lives, Some(Number::Byte(0x8457)));
    assert_eq!(setup.env.sight, defaults.env.sight);
    assert_eq!(setup.ppo, defaults.ppo);
}

/// A line that cannot be read stops the file being read, and says which
/// line and why: a score address mistyped and skipped would leave a network
/// training with nothing to reward it.
#[test]
fn a_bad_line_is_named_not_skipped() {
    for (text, says) in [
        ("score = byte 9G00", "line 1: \"9G00\" is not an address"),
        (
            "\n# fine\nscroe = byte 9000",
            "line 3: \"scroe\" is not something",
        ),
        ("shrink = 3", "shrink: 3 is not 1, 2 or 4"),
        ("lives = nibble 9000", "\"nibble\" is not a kind of number"),
        ("gamma 0.9", "is not key = value"),
        ("inputs = joyboard; left", "starts with an interface"),
    ] {
        let err = Setup::from_text(text).expect_err(text);
        assert!(err.contains(says), "{text:?} gave {err:?}");
    }
}
