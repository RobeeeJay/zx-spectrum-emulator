//! Finding a word on the keys, and the shifts it takes to type it.
//!
//! A Spectrum key carries up to five legends and each is reached a different
//! way, which is the whole difficulty of the keyboard: PRINT is a key on its
//! own at the start of a statement, NOT is SYMBOL SHIFT with S, and BEEP is
//! extended mode and then SYMBOL SHIFT with Z. The search says which keys, and
//! which shifts, and in what order.

use zx_rustrum::keyboard::{self, Reach};

fn found(zx81: bool, text: &str) -> Vec<(&'static str, &'static str, Reach)> {
    keyboard::search(zx81, text)
        .into_iter()
        .map(|f| (keyboard::layout(zx81)[f.key].main, f.legend, f.reach))
        .collect()
}

fn shifts(zx81: bool, text: &str) -> Vec<&'static str> {
    let f = keyboard::search(zx81, text)[0];
    f.shifts(zx81)
        .into_iter()
        .map(|i| keyboard::layout(zx81)[i].main)
        .collect()
}

/// The examples: each is on the key it should be, reached the way the case
/// says, and needs the shifts that way takes.
#[test]
fn the_words_are_found_with_the_shifts_they_take() {
    // PRINT is P's keyword; LPRINT, above C, is found too, since it prints.
    assert_eq!(
        found(false, "print"),
        vec![
            ("P", "PRINT", Reach::Plain),
            ("C", "LPRINT", Reach::Extended)
        ]
    );
    assert!(shifts(false, "print").is_empty(), "PRINT needs no shift");

    assert_eq!(found(false, "not"), vec![("S", "NOT", Reach::Symbol)]);
    assert_eq!(shifts(false, "not"), vec!["SYMBOL SHIFT"]);

    for (word, key) in [("merge", "T"), ("cat", "9"), ("beep", "Z")] {
        let f = found(false, word);
        assert_eq!(f.len(), 1, "{word}: {f:?}");
        assert_eq!((f[0].0, f[0].2), (key, Reach::ExtendedSymbol), "{word}");
        assert_eq!(
            shifts(false, word),
            vec!["CAPS SHIFT", "SYMBOL SHIFT"],
            "{word} is under its key: extended mode, then SYMBOL SHIFT"
        );
    }
}

/// The jobs over the digits, and BREAK, are CAPS SHIFT alone — not extended
/// mode, which is what the word above a letter is.
#[test]
fn the_words_over_the_digits_are_caps_shift() {
    assert_eq!(found(false, "delete"), vec![("0", "DELETE", Reach::Caps)]);
    assert_eq!(shifts(false, "delete"), vec!["CAPS SHIFT"]);
    assert_eq!(found(false, "break"), vec![("SPACE", "BREAK", Reach::Caps)]);
    assert_eq!(found(false, "sin"), vec![("Q", "SIN", Reach::Extended)]);
}

/// On a ZX81 SHIFT is the one shift, and the word above a key is function
/// mode: SHIFT with NEWLINE, then the key.
#[test]
fn a_zx81_has_its_own_ways() {
    assert_eq!(found(true, "lprint"), vec![("S", "LPRINT", Reach::Symbol)]);
    assert_eq!(shifts(true, "lprint"), vec!["SHIFT"]);
    assert_eq!(found(true, "not"), vec![("N", "NOT", Reach::Function)]);
    assert_eq!(shifts(true, "not"), vec!["SHIFT", "NEWLINE"]);
}

/// Case does not matter; commas look for several; a phrase on no key is tried
/// a word at a time, while one that is on a key is kept whole.
#[test]
fn several_things_can_be_looked_for() {
    assert_eq!(found(false, "BeEp"), found(false, "beep"));
    let both = found(false, "cat, beep");
    assert!(both.iter().any(|f| f.1 == "CAT") && both.iter().any(|f| f.1 == "BEEP"));
    let words = found(false, "cat beep");
    assert_eq!(words, both, "a phrase on no key is its words");
    assert_eq!(
        found(false, "def fn"),
        vec![("1", "DEF FN", Reach::ExtendedSymbol)]
    );
    assert!(found(false, "").is_empty() && found(false, "nothing like this").is_empty());
}

/// How to type each one, in words, in the order the shifts go.
#[test]
fn it_says_how_to_type_them() {
    let how = |text| keyboard::search(false, text)[0].how(false);
    assert_eq!(how("beep"), "BEEP: extended mode, then SYMBOL SHIFT with Z");
    assert_eq!(how("not"), "NOT: SYMBOL SHIFT with S");
    assert_eq!(how("print"), "PRINT: P, at the start of a statement");
}
