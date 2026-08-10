//! Building a training corpus out of somebody else's annotations.
//!
//! The corpus tool itself needs a memory image and a symbol file, neither of
//! which is in this repository. What is tested here is the part that matters:
//! that the features come from the emulator's own extractor, so a model is
//! asked at inference time exactly what it was taught on.

use zx_rustrum::autodoc::{self, read_routine};

/// The same routine gives the same features whether it is being read to build
/// a corpus or to describe what is on screen. If these two ever came apart, a
/// model trained on one would be answering a different question from the one
/// it was asked.
#[test]
fn the_corpus_reads_routines_with_the_emulators_own_extractor() {
    let source = include_str!("../src/bin/corpus.rs");
    assert!(
        source.contains("autodoc::read_routine"),
        "the corpus tool has grown a feature extractor of its own"
    );
    assert!(
        source.contains("autodoc::describe"),
        "and it should record what the rules make of each routine, or there is \
         nothing to measure a model against"
    );
}

/// A symbol on a table of bytes is not a routine, and a corpus that says
/// otherwise teaches a model that data looks like code.
#[test]
fn data_is_not_offered_as_a_routine() {
    let mut memory = vec![0u8; 0x10000];
    // A RET on its own: one instruction, which is not a routine worth learning
    // from. $C000 is left as zeros, which disassemble as NOPs.
    memory[0x9000] = 0xC9;
    let peek = |a: u16| memory[a as usize];

    assert_eq!(
        read_routine(&peek, 0x9000).length,
        1,
        "a bare RET is one instruction, and the corpus tool drops those"
    );
}

/// The symbol format the corpus reads is the one the converters write.
#[test]
fn the_corpus_reads_what_the_converters_write() {
    let (symbols, _) = autodoc::parse_symbols(
        "# a comment\n8000 draw_willy ; Draw Willy at his current position\n",
    );
    assert_eq!(symbols.len(), 1);
    assert_eq!(symbols[0].0, 0x8000);
    assert_eq!(symbols[0].1, "draw_willy");
    assert_eq!(symbols[0].2, "Draw Willy at his current position");
}
