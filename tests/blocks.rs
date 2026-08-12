//! Working out where the routines and the data are.

use zx_rustrum::blocks::{self, Block, Kind};
use zx_rustrum::machine::Spectrum;
use zx_rustrum::notes::{parse_blocks, Notes};

/// A block is written to the notes file beside the labels and read back the
/// same, so what a long run of a recording worked out is there next time.
#[test]
fn blocks_go_in_the_notes_file_and_come_back() {
    let block = Block {
        from: 0x8000,
        to: 0x80FF,
        kind: Kind::Code,
    };
    let line = blocks::to_line(&block);
    assert_eq!(line, "block CODE 8000-80FF");
    assert_eq!(blocks::from_line(&line), Some(block));

    // Beside real notes, and not confused with them.
    let text = "# ZX-Rustrum notes\n\
                8000 start ; wait for the frame\n\
                block CODE 8000-80FF\n\
                block DATA B000-B7FF\n";
    let found = parse_blocks(text);
    assert_eq!(found.len(), 2, "both block lines: {found:?}");
    assert_eq!(found[0].kind, Kind::Code);
    assert_eq!(found[1].from, 0xB000);
    assert_eq!(
        zx_rustrum::notes::parse(text).len(),
        1,
        "and the block lines are not read as addresses"
    );
}

/// A line that cannot be understood is skipped rather than throwing the file
/// away: these files are meant to be edited by hand, and a typo should cost
/// one line, not an evening's work.
#[test]
fn a_bad_block_line_is_skipped_not_fatal() {
    for bad in [
        "block CODE",
        "block SOMETHING 8000-80FF",
        "block CODE 80FF-8000",
        "block CODE zzzz-80FF",
        "blocked CODE 8000-80FF",
    ] {
        assert_eq!(blocks::from_line(bad), None, "{bad:?} should not parse");
    }
    let text = "block CODE 8000-80FF\nblock CODE oops\nblock DATA 9000-90FF\n";
    assert_eq!(
        parse_blocks(text).len(),
        2,
        "the two good lines should survive the bad one"
    );
}

/// A second run of a program sees more of it — the next level, the other menu
/// — so what is found is added to what was known rather than replacing it, and
/// the boundaries between routines survive the merge.
#[test]
fn what_is_found_adds_to_what_was_known() {
    let known = [Block {
        from: 0x8000,
        to: 0x80FF,
        kind: Kind::Code,
    }];
    let found = [
        // Touching the known block: one block, not two.
        Block {
            from: 0x8100,
            to: 0x81FF,
            kind: Kind::Code,
        },
        Block {
            from: 0x9000,
            to: 0x90FF,
            kind: Kind::Data,
        },
    ];

    let merged = blocks::merge(&known, &found);
    assert_eq!(
        merged,
        vec![
            // Two routines side by side stay two blocks: where one starts is a
            // fact about the program, and joining them would draw them in one
            // colour and say they were one thing.
            Block {
                from: 0x8000,
                to: 0x80FF,
                kind: Kind::Code
            },
            Block {
                from: 0x8100,
                to: 0x81FF,
                kind: Kind::Code
            },
            Block {
                from: 0x9000,
                to: 0x90FF,
                kind: Kind::Data
            },
        ],
        "the new block should be added without losing the boundary"
    );
}

/// An address that was executed is code, whatever else was done to it. A
/// routine copied into place is read as data by the copier and run as code
/// afterwards, and calling it data would be describing the copy rather than
/// the program.
#[test]
fn code_wins_where_code_and_data_disagree() {
    let known = [Block {
        from: 0x8000,
        to: 0x80FF,
        kind: Kind::Data,
    }];
    let found = [Block {
        from: 0x8080,
        to: 0x817F,
        kind: Kind::Code,
    }];

    let merged = blocks::merge(&known, &found);
    let data: Vec<&Block> = merged
        .iter()
        .filter(|block| block.kind == Kind::Data)
        .collect();
    assert_eq!(
        data.len(),
        1,
        "the data before the code should still be there: {merged:?}"
    );
    assert_eq!(
        (data[0].from, data[0].to),
        (0x8000, 0x807F),
        "up to where the code starts, and no further"
    );
    assert!(
        merged
            .iter()
            .any(|block| block.kind == Kind::Code && block.from == 0x8080 && block.to == 0x817F),
        "and the executed bytes are code: {merged:?}"
    );
}

/// Where a routine ends is where it returned from, which the observer knows
/// because it watched it happen. Decoding forwards until a RET turns up reads
/// a table of graphics as instructions and never stops in the right place.
#[test]
fn a_routines_block_runs_from_its_entry_to_where_it_returned() {
    let mut spec = Spectrum::new();
    // A caller that calls a routine which runs on for a few instructions and
    // returns, then halts.
    let program: [(u16, &[u8]); 2] = [
        // $8000: CALL $9000 : JR $8000
        (0x8000, &[0xCD, 0x00, 0x90, 0x18, 0xFB]),
        // $9000: NOP NOP NOP NOP RET
        (0x9000, &[0x00, 0x00, 0x00, 0x00, 0xC9]),
    ];
    for (at, bytes) in program {
        for (offset, byte) in bytes.iter().enumerate() {
            spec.bus.poke(at + offset as u16, *byte);
        }
    }
    spec.cpu.pc = 0x8000;
    spec.cpu.sp = 0xFF00;
    spec.bus.observer.enabled = true;
    for _ in 0..200 {
        spec.step_instruction();
    }

    let seen = spec
        .bus
        .observer
        .routines
        .get(&0x9000)
        .expect("it was called");
    assert_eq!(
        seen.exits,
        vec![0x9004],
        "the RET is at $9004 and that is where the routine ends"
    );
    assert_eq!(
        seen.spans,
        Some((0x9000, 0x9004)),
        "and it reaches from its entry to there"
    );

    let found = blocks::work_out(&spec.bus.observer);
    assert!(
        found.contains(&Block {
            from: 0x9000,
            to: 0x9004,
            kind: Kind::Code
        }),
        "the routine should be one block: {found:?}"
    );
}

/// Clearing the notes clears the blocks with them: they are part of the same
/// file, and leaving the shape of the program behind after being asked to
/// delete everything would be keeping what was said to be thrown away.
#[test]
fn clearing_the_notes_clears_the_blocks() {
    let mut notes = Notes::unattached();
    notes.set_blocks(vec![Block {
        from: 0x8000,
        to: 0x80FF,
        kind: Kind::Code,
    }]);
    assert_eq!(notes.blocks().len(), 1);
    notes.clear();
    assert!(notes.blocks().is_empty(), "the blocks should go too");
}

/// What a recording of a real game gives: the shape of the program, and how
/// much of it a run of a few seconds accounts for.
#[test]
fn a_recording_gives_the_shape_of_the_program() {
    use zx_rustrum::ui::{App, Roms};

    let path = std::path::PathBuf::from("recordings/manic.rzx");
    if !path.exists() {
        return;
    }
    let roms = Roms {
        rom48: std::fs::read("roms/48.rom").ok(),
        ..Default::default()
    };
    let mut app = App::with_roms(Spectrum::new(), String::new(), roms, None);
    app.show_ram_map = false;
    app.show_debugger = false;
    app.show_back_buffer = false;
    app.show_tape = false;
    app.load_path(&path);
    if app.rzx.is_none() {
        return;
    }
    app.spec.bus.observer.enabled = true;
    if let Some(rzx) = app.rzx.as_mut() {
        rzx.max_speed = true;
    }
    // Twelve seconds of play. The whole recording sees more of the game —
    // that is the point of having one — but a test that plays a minute of it
    // costs the suite twenty seconds to say the same thing.
    for _ in 0..600 {
        app.advance(1.0 / 50.0);
    }

    let found = blocks::work_out(&app.spec.bus.observer);
    let code: u32 = found
        .iter()
        .filter(|block| block.kind == Kind::Code)
        .map(|block| block.length())
        .sum();
    let data: u32 = found
        .iter()
        .filter(|block| block.kind == Kind::Data)
        .map(|block| block.length())
        .sum();
    println!(
        "MANIC {} blocks, {code} bytes of code, {data} of data",
        found.len()
    );
    assert!(
        code > 1000,
        "only {code} bytes of code in twelve seconds of play"
    );
    assert!(
        data > 1000,
        "only {data} bytes of data in twelve seconds of play"
    );
    assert!(
        found.len() > 10,
        "a game running for twelve seconds shows more than {} blocks",
        found.len()
    );
    // No address in two blocks: the listing bands each row once, and a row in
    // two blocks would be drawn in whichever colour won the race.
    for pair in found.windows(2) {
        assert!(
            pair[0].to < pair[1].from,
            "{:04X}-{:04X} overlaps {:04X}-{:04X}",
            pair[0].from,
            pair[0].to,
            pair[1].from,
            pair[1].to
        );
    }
}

/// Neighbouring blocks are drawn in different colours, and code and data in
/// different pairs of colours. A palette that repeated every block would put
/// two routines side by side in the same shade, which is the one thing the
/// banding exists to prevent.
#[test]
fn neighbouring_blocks_are_drawn_in_different_colours() {
    use zx_rustrum::ui::theme;

    for index in 0..8 {
        assert_ne!(
            theme::band(index, Kind::Code),
            theme::band(index + 1, Kind::Code),
            "blocks {index} and {} came out the same colour",
            index + 1
        );
        assert_ne!(
            theme::band(index, Kind::Code),
            theme::band(index, Kind::Data),
            "code and data should not share a shade"
        );
    }
}
