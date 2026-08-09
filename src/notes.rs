//! Labels and comments the user writes against addresses while disassembling.
//!
//! They are kept in a plain text file beside whatever is being disassembled —
//! the tape if one is in the deck, otherwise the ROM — so a listing built up
//! over an evening is still there next time, and can be read, edited or diffed
//! without the emulator. One line per address:
//!
//! ```text
//! # ZX-Rustrum notes
//! 8000 start      ; wait for the frame to finish
//! 8003            ; the border is set here
//! 800A loop
//! ```

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// What is written against one address. Either half may be empty; an entry
/// with both empty is dropped rather than written out.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Note {
    pub label: String,
    pub comment: String,
}

impl Note {
    fn is_empty(&self) -> bool {
        self.label.is_empty() && self.comment.is_empty()
    }
}

/// The notes for one file, and where they are saved.
#[derive(Clone, Debug, Default)]
pub struct Notes {
    entries: BTreeMap<u16, Note>,
    /// Where these are written. None when nothing is loaded to write beside,
    /// in which case the notes are still usable, just not kept.
    path: Option<PathBuf>,
    /// Whether there is anything to write that has not been written.
    dirty: bool,
}

impl Notes {
    /// The extension the sidecar file carries.
    pub const EXTENSION: &'static str = "zxrs.txt";

    /// Where the notes for a file live: beside it, under its own name.
    /// `games/manic.tap` keeps its notes in `games/manic.zxrs.txt`.
    pub fn sidecar(source: &Path) -> PathBuf {
        source.with_extension(Self::EXTENSION)
    }

    /// Read the notes for a file. A file that is not there yet is not an
    /// error: it is simply a listing nobody has annotated.
    pub fn for_file(source: &Path) -> Notes {
        let path = Self::sidecar(source);
        let entries = std::fs::read_to_string(&path)
            .map(|text| parse(&text))
            .unwrap_or_default();
        Notes {
            entries,
            path: Some(path),
            dirty: false,
        }
    }

    /// Notes with nowhere to go: used until something is loaded.
    pub fn unattached() -> Notes {
        Notes::default()
    }

    pub fn file(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn label(&self, addr: u16) -> &str {
        self.entries.get(&addr).map_or("", |n| n.label.as_str())
    }

    pub fn comment(&self, addr: u16) -> &str {
        self.entries.get(&addr).map_or("", |n| n.comment.as_str())
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn set_label(&mut self, addr: u16, label: &str) {
        self.edit(addr, |note| note.label = label.trim().to_string());
    }

    pub fn set_comment(&mut self, addr: u16, comment: &str) {
        self.edit(addr, |note| note.comment = comment.trim().to_string());
    }

    fn edit(&mut self, addr: u16, change: impl FnOnce(&mut Note)) {
        let note = self.entries.entry(addr).or_default();
        let before = note.clone();
        change(note);
        if *note == before {
            return;
        }
        if note.is_empty() {
            self.entries.remove(&addr);
        }
        self.dirty = true;
    }

    /// Whether there are changes waiting to be written.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Write the notes out, if they have changed and there is somewhere to
    /// write them. Returns what went wrong, if anything: a listing is not
    /// worth stopping the emulator over, but the user should be told.
    pub fn save_if_dirty(&mut self) -> Result<bool, String> {
        if !self.dirty {
            return Ok(false);
        }
        let Some(path) = self.path.clone() else {
            return Ok(false);
        };
        // An empty set of notes leaves no litter behind: the file is removed
        // rather than left as a header with nothing under it.
        let result = if self.entries.is_empty() {
            match std::fs::remove_file(&path) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                other => other,
            }
        } else {
            std::fs::write(&path, self.to_text())
        };
        match result {
            Ok(()) => {
                self.dirty = false;
                Ok(true)
            }
            Err(e) => Err(format!("{}: {e}", path.display())),
        }
    }

    /// The file's contents: a header saying what it is, then the notes in
    /// address order so the file reads like the listing it describes.
    pub fn to_text(&self) -> String {
        let mut out = String::from(
            "# ZX-Rustrum notes. One line per address:\n\
             #   ADDR [label] [; comment]\n",
        );
        for (addr, note) in &self.entries {
            out.push_str(&format!("{addr:04X}"));
            if !note.label.is_empty() {
                out.push_str(&format!(" {}", note.label));
            }
            if !note.comment.is_empty() {
                out.push_str(&format!(" ; {}", note.comment));
            }
            out.push('\n');
        }
        out
    }
}

/// Read the file back. Anything that cannot be understood is skipped rather
/// than throwing the rest away: these files are meant to be edited by hand.
pub fn parse(text: &str) -> BTreeMap<u16, Note> {
    let mut entries = BTreeMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (addr, rest) = match line.split_once(char::is_whitespace) {
            Some((addr, rest)) => (addr, rest.trim()),
            None => (line, ""),
        };
        let Ok(addr) = u16::from_str_radix(addr.trim_start_matches('$'), 16) else {
            continue;
        };
        let (label, comment) = match rest.split_once(';') {
            Some((label, comment)) => (label.trim(), comment.trim()),
            None => (rest, ""),
        };
        let note = Note {
            label: label.to_string(),
            comment: comment.to_string(),
        };
        if !note.is_empty() {
            entries.insert(addr, note);
        }
    }
    entries
}
