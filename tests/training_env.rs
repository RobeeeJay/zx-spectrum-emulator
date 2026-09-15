//! The training environment: what the network may press, what it may see, the
//! reward, and games played one at a time and many at once.

use std::sync::Arc;
use zx_rustrum::joystick::Kind;
use zx_rustrum::machine::Spectrum;
use zx_rustrum::training::env::{Config, Env, Pool};
use zx_rustrum::training::inputs::InputSet;
use zx_rustrum::training::judge::{Judge, Number};
use zx_rustrum::training::sight::{changes, look, Area, Sight};
use zx_rustrum::z80::Bus;

/// A Kempston action is exactly what port $1F reads, and choosing another
/// action lets go of the first: a direction left held from the last step
/// would be a move the network never chose.
#[test]
fn a_kempston_action_is_what_the_port_reads() {
    let set = InputSet::joystick(Kind::Kempston, false, true);
    let find = |name: &str| set.actions.iter().position(|a| a.name == name).unwrap();
    let mut spec = Spectrum::new();
    spec.bus.set_joystick(Kind::Kempston);
    set.apply(&mut spec, find("left+fire"));
    assert_eq!(spec.bus.io_read(0x001F), 0x12, "left is bit 1, fire bit 4");
    set.apply(&mut spec, find("nothing"));
    assert_eq!(spec.bus.io_read(0x001F), 0x00, "and let go");
    assert!(
        set.actions.iter().any(|a| a.name == "up+fire")
            && !set.actions.iter().any(|a| a.name == "up+left"),
        "fire while moving, but no diagonals unless asked: {:?}",
        set.actions.iter().map(|a| &a.name).collect::<Vec<_>>()
    );
}

/// Keys chosen by their legends press those keys and nothing else, and the
/// set is saved as text and read back.
#[test]
fn a_key_set_presses_its_keys_only() {
    let set = InputSet::keys(&["Q", "A", "SPACE"]).unwrap();
    assert_eq!(set.actions.len(), 4, "the three and doing nothing");
    let mut spec = Spectrum::new();
    set.apply(&mut spec, 3);
    assert_eq!(spec.bus.keys[7], 0xFE, "SPACE is row 7 bit 0");
    assert_eq!(spec.bus.keys[2], 0xFF, "and Q is not down");
    set.apply(&mut spec, 1);
    assert_eq!(
        (spec.bus.keys[2], spec.bus.keys[7]),
        (0xFE, 0xFF),
        "Q, and SPACE let go"
    );
    assert!(InputSet::keys(&["NOT A KEY"]).is_err());

    let stick = InputSet::joystick(Kind::Sinclair1, true, false);
    for original in [set, stick] {
        let text = original.to_text();
        assert_eq!(InputSet::from_text(&text).unwrap(), original, "{text}");
    }
}

/// The network sees the screen and nothing else: two machines with the same
/// picture but different memory and registers look the same to it, and a
/// change on the screen is a change in what it sees.
#[test]
fn the_network_sees_only_the_screen() {
    let sight = Sight {
        area: Area::Display,
        shrink: 2,
        colour: false,
        frames: 1,
    };
    assert_eq!(
        (sight.width(), sight.height(), sight.channels()),
        (128, 96, 1)
    );

    let mut a = Spectrum::new();
    let mut b = Spectrum::new();
    for addr in 0x8000..0x9000u16 {
        b.bus.poke(addr, (addr as u8).wrapping_mul(7));
    }
    b.cpu.pc = 0x1234;
    b.cpu.a = 0x99;
    a.run(zx_rustrum::machine::FRAME_T);
    b.run(zx_rustrum::machine::FRAME_T);
    // Whatever the CPU did, put the same picture in front of both: a clear
    // screen, black ink on white paper — ink and paper the same colour would
    // make every picture alike, and the test mean nothing.
    for m in [&mut a, &mut b] {
        for addr in 0x4000..0x5B00u16 {
            m.bus.poke(addr, if addr >= 0x5800 { 0x38 } else { 0 });
        }
        m.cpu.pc = 0x8000;
        m.bus.poke(0x8000, 0x18); // JR $ at $8000, in both
        m.bus.poke(0x8001, 0xFE);
    }
    for addr in 0x8002..0x9000u16 {
        b.bus.poke(addr, (addr as u8).wrapping_mul(7));
    }
    a.run(zx_rustrum::machine::FRAME_T);
    b.run(zx_rustrum::machine::FRAME_T);
    let seen = look(&a, &sight);
    assert!(
        seen.iter().any(|v| *v > 0),
        "the picture is white paper, not all black — or the test would prove nothing"
    );
    assert_eq!(seen, look(&b, &sight), "different memory, same picture");

    a.bus.poke(0x4000, 0xFF);
    a.run(zx_rustrum::machine::FRAME_T);
    assert_ne!(
        look(&a, &sight),
        look(&b, &sight),
        "a change on the screen is seen"
    );
}

/// The judge's numbers, and its rewards: points up are rewarded, a life lost
/// costs, running out of lives ends the game, and a score put back to nought
/// is not a punishment.
#[test]
fn the_judge_reads_the_score_and_the_lives() {
    let mut memory = [0u8; 0x10000];
    memory[0x9000] = 0x01;
    memory[0x9001] = 0x23; // BCD 0123
    memory[0x9100] = 3; // lives
    memory[0x9200] = b'4';
    memory[0x9201] = b'2';
    let peek = |m: &[u8; 0x10000]| {
        let m = *m;
        move |a: u16| m[a as usize]
    };
    assert_eq!(
        Number::Bcd {
            at: 0x9000,
            bytes: 2
        }
        .read(&peek(&memory)),
        123
    );
    assert_eq!(Number::Word(0x9000).read(&peek(&memory)), 0x2301);
    assert_eq!(
        Number::Digits {
            at: 0x9200,
            count: 2,
            zero: b'0'
        }
        .read(&peek(&memory)),
        42
    );

    let judge = Judge {
        score: Some(Number::Bcd {
            at: 0x9000,
            bytes: 2,
        }),
        score_scale: 0.1,
        lives: Some(Number::Byte(0x9100)),
        life_penalty: 5.0,
        ..Judge::default()
    };
    let mut tally = judge.start(&peek(&memory));
    memory[0x9001] = 0x50; // 150
    let (reward, over) = judge.judge(&mut tally, &peek(&memory));
    assert!(
        (reward - 2.7).abs() < 1e-4 && !over,
        "27 points at 0.1: {reward}"
    );
    memory[0x9100] = 2;
    let (reward, over) = judge.judge(&mut tally, &peek(&memory));
    assert!(
        (reward + 5.0).abs() < 1e-4 && !over,
        "a life lost: {reward}"
    );
    memory[0x9000] = 0;
    memory[0x9001] = 0;
    memory[0x9100] = 0;
    let (reward, over) = judge.judge(&mut tally, &peek(&memory));
    assert!(
        (reward + 10.0).abs() < 1e-4,
        "two lives, and no penalty for the score going back: {reward}"
    );
    assert!(over, "no lives left is game over");
}

/// A machine running a tiny game: each press of fire on the Kempston port
/// scores a point at $9000 and draws a byte on the top line of the screen.
fn tiny_game() -> Spectrum {
    let mut spec = Spectrum::new();
    let code: [u8; 26] = [
        0xF3, // DI
        0xDB, 0x1F, // loop: IN A,($1F)
        0xE6, 0x10, // AND $10
        0x28, 0xFA, // JR Z,loop
        0x21, 0x00, 0x90, // LD HL,$9000
        0x34, // INC (HL)
        0x7E, // LD A,(HL)
        0x5F, // LD E,A
        0x16, 0x40, // LD D,$40
        0x3E, 0xFF, // LD A,$FF
        0x12, // LD (DE),A
        0xDB, 0x1F, // wait: IN A,($1F)
        0xE6, 0x10, // AND $10
        0x20, 0xFA, // JR NZ,wait
        0x18, 0xE7, // JR loop
    ];
    for (i, b) in code.iter().enumerate() {
        spec.bus.poke(0x8000 + i as u16, *b);
    }
    for addr in 0x4000..0x5B00u16 {
        spec.bus.poke(addr, if addr >= 0x5800 { 0x38 } else { 0 });
    }
    spec.bus.poke(0x9000, 0);
    spec.cpu.pc = 0x8000;
    spec.cpu.sp = 0x7F00;
    spec
}

fn tiny_config(random_wait: u32) -> Config {
    Config {
        inputs: InputSet::joystick(Kind::Kempston, false, false),
        sight: Sight {
            area: Area::Display,
            shrink: 2,
            colour: false,
            frames: 2,
        },
        judge: Judge {
            score: Some(Number::Byte(0x9000)),
            max_steps: 6,
            ..Judge::default()
        },
        frames_per_step: 2,
        random_wait,
    }
}

/// Stepping the tiny game: pressing fire scores and is seen on the screen,
/// doing nothing scores nothing, the game ends after its steps and starts
/// again, and all of it is the same every time.
#[test]
fn a_game_is_stepped_rewarded_and_repeatable() {
    let config = tiny_config(0);
    let fire = config
        .inputs
        .actions
        .iter()
        .position(|a| a.name == "fire")
        .unwrap();
    let start = Arc::new(tiny_game());
    let mut env = Env::new(start.clone(), Arc::new(config.clone()), 1);
    let first = env.observation();
    let plan = [fire, 0, fire, 0, 0, fire];
    let steps: Vec<_> = plan.iter().map(|a| env.step(*a)).collect();
    let rewards: Vec<f32> = steps.iter().map(|s| s.reward).collect();
    assert_eq!(
        rewards,
        vec![1.0, 0.0, 1.0, 0.0, 0.0, 1.0],
        "a point a press"
    );
    assert_ne!(steps[0].observation, first, "the press is on the screen");
    assert!(steps[5].done, "six steps and the game is over");
    assert_eq!(steps[5].game_reward, Some(3.0));
    assert_eq!(
        steps[5].observation, first,
        "and the next game starts as the first did"
    );

    let mut again = Env::new(start, Arc::new(config), 1);
    let replay: Vec<_> = plan.iter().map(|a| again.step(*a)).collect();
    for (a, b) in steps.iter().zip(&replay) {
        assert_eq!(
            (&a.observation, a.reward, a.done),
            (&b.observation, b.reward, b.done)
        );
    }
}

/// Many games stepped across cores come out as they would one at a time.
#[test]
fn a_pool_of_games_agrees_with_one_at_a_time() {
    let config = tiny_config(3);
    let fire = config
        .inputs
        .actions
        .iter()
        .position(|a| a.name == "fire")
        .unwrap();
    let start = tiny_game();
    let games = 8;
    let mut pool = Pool::new(&start, config.clone(), games, 7);
    let plans: Vec<Vec<usize>> = (0..games)
        .map(|g| {
            (0..5)
                .map(|s| if (g + s) % 2 == 0 { fire } else { 0 })
                .collect()
        })
        .collect();
    let mut pooled = Vec::new();
    for s in 0..5 {
        let actions: Vec<usize> = plans.iter().map(|p| p[s]).collect();
        pooled.push(pool.step(&actions));
    }
    let start = Arc::new(start);
    let config = Arc::new(config);
    for (g, plan) in plans.iter().enumerate() {
        let mut env = Env::new(start.clone(), config.clone(), 7 + g as u64);
        for (s, action) in plan.iter().enumerate() {
            let alone = env.step(*action);
            let together = &pooled[s][g];
            assert_eq!(
                (&alone.observation, alone.reward, alone.done),
                (&together.observation, together.reward, together.done),
                "game {g}, step {s}"
            );
        }
    }
}

/// How much each frame of an observation moved since the one before, which
/// is what says whether stacking frames is telling the network anything: a
/// game drawing nothing new between steps stacks the same picture over and
/// over.
#[test]
fn the_frames_of_an_observation_say_how_much_moved() {
    let sight = Sight {
        area: Area::Display,
        shrink: 4,
        colour: false,
        frames: 4,
    };
    let len = sight.frame_len();
    let still: Vec<u8> = std::iter::repeat_n(7u8, len * 4).collect();
    assert_eq!(
        changes(&still, &sight),
        vec![0.0, 0.0, 0.0],
        "four of the same picture moved nothing"
    );

    let mut moving = still.clone();
    // A tenth of the last frame is redrawn, and a byte of the one before it
    // by too little to count as movement.
    for i in 0..len / 10 {
        moving[3 * len + i] = 200;
    }
    moving[2 * len] = 7 + 8;
    let moved = changes(&moving, &sight);
    assert_eq!(&moved[..2], &[0.0, 0.0], "nothing moved in the first three");
    assert!(
        (moved[2] - 0.1).abs() < 0.01,
        "a tenth of the last frame moved: {}",
        moved[2]
    );

    let one = Sight { frames: 1, ..sight };
    assert!(
        changes(&still[..len], &one).is_empty(),
        "one frame has nothing to differ from"
    );
}
