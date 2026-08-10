//! The labels and comments kept beside a tape or a ROM.

use std::path::Path;
use zx_rustrum::notes::{parse, Notes};

/// The notes live beside the file they describe, under its own name, so a
/// directory of tapes has an obvious file per tape rather than a pile of
/// identically named ones.
#[test]
fn notes_are_named_after_the_file_they_belong_to() {
    assert_eq!(
        Notes::sidecar(Path::new("/games/manic.tap")),
        Path::new("/games/manic.zxrs.txt")
    );
    assert_eq!(
        Notes::sidecar(Path::new("roms/48.rom")),
        Path::new("roms/48.zxrs.txt")
    );
}

/// A line is an address, then a label, then a comment after a semicolon —
/// each of the three parts optional bar the address.
#[test]
fn a_line_can_carry_a_label_a_comment_or_both() {
    let notes = parse(
        "# ZX-Rustrum notes\n\
         8000 start ; wait for the frame\n\
         8003 ; the border is set here\n\
         800A loop\n",
    );

    assert_eq!(notes[&0x8000].label, "start");
    assert_eq!(notes[&0x8000].comment, "wait for the frame");
    assert_eq!(notes[&0x8003].label, "", "there is no label on this line");
    assert_eq!(notes[&0x8003].comment, "the border is set here");
    assert_eq!(notes[&0x800A].label, "loop");
    assert_eq!(notes[&0x800A].comment, "");
}

/// The file is there to be edited by hand, so it survives being edited badly:
/// a line nobody can make sense of is skipped and the rest is kept.
#[test]
fn a_line_that_makes_no_sense_does_not_lose_the_rest() {
    let notes = parse(
        "\n\
         # a comment about the file itself\n\
         $4000 screen ; the display file\n\
         not an address at all\n\
         ZZZZ nope\n\
         4001 ; second line\n",
    );

    assert_eq!(
        notes.len(),
        2,
        "kept {:?}",
        notes.keys().collect::<Vec<_>>()
    );
    assert_eq!(notes[&0x4000].label, "screen", "a leading $ is allowed");
    assert_eq!(notes[&0x4001].comment, "second line");
}

/// What is written out reads back the same, so an evening's work is not
/// quietly changed by being saved.
#[test]
fn what_is_written_reads_back_unchanged() {
    let dir = tempdir("round-trip");
    let source = dir.join("manic.tap");
    let mut notes = Notes::for_file(&source);
    notes.set_label(0x8000, "start");
    notes.set_comment(0x8000, "wait for the frame");
    notes.set_comment(0x8003, "border");
    notes.set_label(0xFFFF, "last");
    assert!(notes.save_if_dirty().unwrap(), "nothing was written");

    let written = std::fs::read_to_string(dir.join("manic.zxrs.txt")).unwrap();
    assert!(
        written.starts_with("# ZX-Rustrum notes"),
        "the file should say what it is:\n{written}"
    );

    let read_back = Notes::for_file(&source);
    for addr in [0x8000, 0x8003, 0xFFFFu16] {
        assert_eq!(
            (read_back.label(addr), read_back.comment(addr)),
            (notes.label(addr), notes.comment(addr)),
            "${addr:04X} came back different:\n{written}"
        );
    }
}

/// Saving happens only when there is something to save. Writing the file on
/// every frame of the debugger would be a write per frame to somebody's disk.
#[test]
fn nothing_is_written_until_something_changes() {
    let dir = tempdir("only-when-dirty");
    let source = dir.join("game.tap");
    let mut notes = Notes::for_file(&source);
    assert!(!notes.is_dirty(), "fresh notes have nothing to save");
    assert!(
        !notes.save_if_dirty().unwrap(),
        "wrote a file for no reason"
    );
    assert!(!dir.join("game.zxrs.txt").exists());

    notes.set_label(0x1234, "here");
    assert!(notes.is_dirty());
    assert!(notes.save_if_dirty().unwrap());
    assert!(!notes.is_dirty(), "saving should clear it");
    assert!(!notes.save_if_dirty().unwrap(), "saved twice over");
}

/// Clearing the last note takes the file with it, rather than leaving a
/// header behind that says nothing.
#[test]
fn emptying_the_notes_removes_the_file() {
    let dir = tempdir("emptied");
    let source = dir.join("game.tap");
    let sidecar = dir.join("game.zxrs.txt");
    let mut notes = Notes::for_file(&source);
    notes.set_comment(0x4000, "something");
    notes.save_if_dirty().unwrap();
    assert!(sidecar.exists());

    notes.set_comment(0x4000, "");
    assert!(notes.is_empty(), "the note should be gone, not left blank");
    notes.save_if_dirty().unwrap();
    assert!(!sidecar.exists(), "the empty file was left behind");
}

/// Notes with nowhere to go are still usable — a machine can be disassembled
/// before anything is loaded into it — they are simply not kept.
#[test]
fn notes_with_no_file_behind_them_do_not_fail() {
    let mut notes = Notes::unattached();
    notes.set_label(0x0000, "reset");
    assert_eq!(notes.label(0x0000), "reset");
    assert!(notes.file().is_none());
    assert!(!notes.save_if_dirty().unwrap(), "there is nowhere to write");
}

/// A scratch directory of our own, so the tests do not tread on each other.
fn tempdir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("zxrs-notes-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A guess is written with an `@` in front of it, so the file says which lines
/// are the emulator's opinion and which are the user's.
#[test]
fn guesses_are_marked_in_the_file() {
    let dir = tempdir("marked");
    let source = dir.join("game.tap");
    let mut notes = Notes::for_file(&source);
    notes.set_label(0x8000, "mine");
    notes.suggest(0x9000, "clear_screen_9000", "Fills the display file");
    notes.save_if_dirty().unwrap();

    let written = std::fs::read_to_string(dir.join("game.zxrs.txt")).unwrap();
    assert!(
        written.contains("8000 mine"),
        "the user's line should be plain:\n{written}"
    );
    assert!(
        written.contains("9000 @clear_screen_9000 ; @Fills the display file"),
        "the guess should be marked:\n{written}"
    );

    // And it comes back knowing which was which.
    let read_back = Notes::for_file(&source);
    assert!(read_back.label_is_auto(0x9000), "the mark was lost");
    assert!(
        !read_back.label_is_auto(0x8000),
        "the user's line was marked"
    );
    assert_eq!(read_back.label(0x9000), "clear_screen_9000", "@ kept in");
}

/// A later run replaces an earlier guess: the machine may have unpacked the
/// code by then, and the second look is the better one.
#[test]
fn a_later_guess_replaces_an_earlier_one() {
    let mut notes = Notes::unattached();
    notes.suggest(0x9000, "routine_9000", "");
    notes.suggest(0x9000, "draw_sprite_9000", "Merges bytes into the screen");

    assert_eq!(notes.label(0x9000), "draw_sprite_9000");
    assert_eq!(notes.comment(0x9000), "Merges bytes into the screen");
    assert!(notes.label_is_auto(0x9000));
}

/// But a guess never replaces what somebody typed, in either half of the note.
#[test]
fn a_guess_leaves_the_users_own_words_alone() {
    let mut notes = Notes::unattached();
    notes.set_label(0x9000, "sprite_masker");
    notes.suggest(0x9000, "draw_sprite_9000", "Merges bytes into the screen");

    assert_eq!(
        notes.label(0x9000),
        "sprite_masker",
        "the name was taken over"
    );
    assert!(!notes.label_is_auto(0x9000));
    assert_eq!(
        notes.comment(0x9000),
        "Merges bytes into the screen",
        "the empty half is fair game, though"
    );
    assert!(notes.comment_is_auto(0x9000));
}

/// Typing over a guess makes it the user's own, and it stops being replaced.
#[test]
fn typing_over_a_guess_makes_it_yours() {
    let mut notes = Notes::unattached();
    notes.suggest(0x9000, "routine_9000", "");
    notes.set_label(0x9000, "the_loader");
    assert!(!notes.label_is_auto(0x9000), "it should be the user's now");

    notes.suggest(0x9000, "decompress_9000", "");
    assert_eq!(
        notes.label(0x9000),
        "the_loader",
        "a later guess took it back"
    );
}

/// An empty guess does not wipe out a better one from a previous run.
#[test]
fn an_empty_guess_does_not_erase_a_previous_one() {
    let mut notes = Notes::unattached();
    notes.suggest(0x9000, "decompress_9000", "Unpacks compressed data");
    notes.suggest(0x9000, "", "");

    assert_eq!(notes.label(0x9000), "decompress_9000");
    assert_eq!(notes.comment(0x9000), "Unpacks compressed data");
}
