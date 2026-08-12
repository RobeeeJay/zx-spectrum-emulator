//! Just enough of the ZIP format to get a tape or a recording out of one.
//!
//! Games arrive zipped, and a zip holding one file is the usual shape of a
//! download. What is needed is to look inside, find the file worth loading and
//! unpack it — not to write archives, not to walk directories, not to handle
//! encryption or spanned volumes.
//!
//! Written here rather than taken as a dependency, for the same reason
//! [`crate::svg`] is: the build stays offline-reproducible against the crates
//! already in the lock file. The compression is deflate, which `flate2`
//! already provides for the recordings.

use std::io::Read;

/// A file inside the archive.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    /// 0 is stored, 8 is deflate. Anything else is somebody else's format.
    pub method: u16,
    pub compressed: usize,
    pub uncompressed: usize,
    /// Where the local header is, which is where the data is found from.
    pub at: usize,
}

impl Entry {
    /// The part after the last dot, lowercased: what decides how a file is
    /// read.
    pub fn extension(&self) -> String {
        self.name
            .rsplit_once('.')
            .map(|(_, ext)| ext.to_ascii_lowercase())
            .unwrap_or_default()
    }
}

const LOCAL_HEADER: u32 = 0x0403_4b50;
const CENTRAL_ENTRY: u32 = 0x0201_4b50;
const END_OF_DIRECTORY: u32 = 0x0605_4b50;

/// Whether these bytes look like an archive. A zip starts with a local header,
/// or with the end-of-directory record when it holds nothing at all.
pub fn is_zip(data: &[u8]) -> bool {
    data.len() >= 4 && matches!(u32at(data, 0), Some(LOCAL_HEADER) | Some(END_OF_DIRECTORY))
}

/// Everything in the archive, in the order the directory lists it.
///
/// The directory is at the end and points backwards, which is what lets a zip
/// be written in one pass — and means the file has to be read from the end to
/// know what is in it.
pub fn entries(data: &[u8]) -> Result<Vec<Entry>, String> {
    let end = end_of_directory(data).ok_or("not a zip file, or its directory is missing")?;
    let count = u16at(data, end + 10).ok_or("truncated zip directory")? as usize;
    let mut at = u32at(data, end + 16).ok_or("truncated zip directory")? as usize;

    let mut entries = Vec::with_capacity(count.min(4096));
    for _ in 0..count {
        if u32at(data, at) != Some(CENTRAL_ENTRY) {
            break;
        }
        let method = u16at(data, at + 10).ok_or("truncated zip directory")?;
        let compressed = u32at(data, at + 20).ok_or("truncated zip directory")? as usize;
        let uncompressed = u32at(data, at + 24).ok_or("truncated zip directory")? as usize;
        let name_len = u16at(data, at + 28).ok_or("truncated zip directory")? as usize;
        let extra_len = u16at(data, at + 30).ok_or("truncated zip directory")? as usize;
        let comment_len = u16at(data, at + 32).ok_or("truncated zip directory")? as usize;
        let offset = u32at(data, at + 42).ok_or("truncated zip directory")? as usize;
        let name_at = at + 46;
        let name = data
            .get(name_at..name_at + name_len)
            .ok_or("truncated zip directory")?;
        entries.push(Entry {
            // Names are bytes, and not always UTF-8: a name that cannot be
            // read is still a file that can be unpacked.
            name: String::from_utf8_lossy(name).to_string(),
            method,
            compressed,
            uncompressed,
            at: offset,
        });
        at = name_at + name_len + extra_len + comment_len;
    }
    Ok(entries)
}

/// Unpack one entry.
pub fn read(data: &[u8], entry: &Entry) -> Result<Vec<u8>, String> {
    if u32at(data, entry.at) != Some(LOCAL_HEADER) {
        return Err(format!(
            "{}: its header is not where the directory says",
            entry.name
        ));
    }
    // The local header repeats the name and carries its own extra field, which
    // need not be the same length as the directory's.
    let name_len = u16at(data, entry.at + 26).ok_or("truncated zip entry")? as usize;
    let extra_len = u16at(data, entry.at + 28).ok_or("truncated zip entry")? as usize;
    let from = entry.at + 30 + name_len + extra_len;
    let packed = data
        .get(from..from + entry.compressed)
        .ok_or_else(|| format!("{}: runs off the end of the file", entry.name))?;

    match entry.method {
        0 => Ok(packed.to_vec()),
        8 => {
            let mut out = Vec::with_capacity(entry.uncompressed.min(64 << 20));
            flate2::read::DeflateDecoder::new(packed)
                .read_to_end(&mut out)
                .map_err(|e| format!("{}: could not unpack it ({e})", entry.name))?;
            Ok(out)
        }
        other => Err(format!(
            "{}: packed with method {other}, which this does not read",
            entry.name
        )),
    }
}

/// The first file in the archive with one of these extensions, unpacked.
///
/// The first rather than the best: an archive holding a tape and a scan of the
/// inlay has one file worth loading, and picking between two tapes is a
/// question for whoever made the archive, not for this.
pub fn first_with_extension(data: &[u8], wanted: &[&str]) -> Option<(String, Vec<u8>)> {
    let entries = entries(data).ok()?;
    let entry = entries.iter().find(|entry| {
        // Directories are entries too, and end in a slash.
        !entry.name.ends_with('/') && wanted.contains(&entry.extension().as_str())
    })?;
    let bytes = read(data, entry).ok()?;
    Some((entry.name.clone(), bytes))
}

/// Find the end-of-directory record, which sits at the end behind a comment of
/// up to sixty-four kilobytes.
fn end_of_directory(data: &[u8]) -> Option<usize> {
    let earliest = data.len().saturating_sub(22 + 0xFFFF);
    (earliest..data.len().saturating_sub(21))
        .rev()
        .find(|at| u32at(data, *at) == Some(END_OF_DIRECTORY))
}

fn u16at(data: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(data.get(at..at + 2)?.try_into().ok()?))
}

fn u32at(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(data.get(at..at + 4)?.try_into().ok()?))
}
