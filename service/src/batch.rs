//! Files sent in one body, both ways between the page and the service: each [u32 path length][path, UTF-8][f64 time of
//! change, seconds since 1970][u32 length][bytes], little-endian. The page sends KovaaK's files and the cut-off's labels
//! this way (library/browser.rs) and an export's recording to open (library/export.rs); the service answers an export
//! with one. The page's side is ui/src/app/modes/service/kovaak-batch.ts. In: a body. Out: its files, or a body.

use crate::library::{Answer, Failure};

/// A u32's bytes in a batch.
const U32_BYTES: usize = 4;
/// An f64's bytes in a batch.
const F64_BYTES: usize = 8;

/// One file of a batch.
pub struct BatchFile<'a> {
    /// Its path (stats/<name>, train/<name>.npz, reviews/<model>/tracks.json, ...).
    pub path: &'a str,
    /// Its time of change in seconds since 1970.
    pub modified: f64,
    /// Its bytes.
    pub bytes: &'a [u8],
}

/// A batch's files; a batch that ends early or names a path that is not UTF-8 is refused (400).
pub fn read(body: &[u8]) -> Answer<Vec<BatchFile<'_>>> {
    let mut rest = body;
    let mut files = Vec::new();
    while !rest.is_empty() {
        let path_len = take_u32(&mut rest)?;
        let path =
            std::str::from_utf8(take(&mut rest, path_len)?).map_err(|_| Failure::bad("a path that is not UTF-8"))?;
        let modified = f64::from_le_bytes(take(&mut rest, F64_BYTES)?.try_into().unwrap_or_default());
        let len = take_u32(&mut rest)?;
        files.push(BatchFile { path, modified, bytes: take(&mut rest, len)? });
    }
    Ok(files)
}

/// Adds one file to a batch's body.
pub fn push(out: &mut Vec<u8>, path: &str, modified: f64, bytes: &[u8]) {
    out.extend_from_slice(&u32::try_from(path.len()).unwrap_or(u32::MAX).to_le_bytes());
    out.extend_from_slice(path.as_bytes());
    out.extend_from_slice(&modified.to_le_bytes());
    out.extend_from_slice(&u32::try_from(bytes.len()).unwrap_or(u32::MAX).to_le_bytes());
    out.extend_from_slice(bytes);
}

/// The first `len` bytes of `rest`, which then starts after them; a batch that ends early is refused (400).
fn take<'a>(rest: &mut &'a [u8], len: usize) -> Answer<&'a [u8]> {
    if rest.len() < len {
        return Err(Failure::bad("the batch of files ends early"));
    }
    let (part, after) = rest.split_at(len);
    *rest = after;
    Ok(part)
}

/// A little-endian u32 from the start of `rest` (see `take`).
fn take_u32(rest: &mut &[u8]) -> Answer<usize> {
    Ok(u32::from_le_bytes(take(rest, U32_BYTES)?.try_into().unwrap_or_default()) as usize)
}

/// A batch read back.
#[cfg(test)]
mod tests {
    use super::*;

    /// Files pushed read back as they were; a body cut short is refused.
    #[test]
    fn files_read_back() {
        let mut body = Vec::new();
        push(&mut body, "a/b.json", 1.5, b"{}");
        push(&mut body, "\u{e9}.csv", 0.0, b"");
        let files = read(&body).unwrap();
        let got: Vec<(&str, f64, &[u8])> = files.iter().map(|file| (file.path, file.modified, file.bytes)).collect();
        assert_eq!(got, [("a/b.json", 1.5, b"{}".as_slice()), ("\u{e9}.csv", 0.0, b"".as_slice())]);
        assert!(read(&body[..body.len() - 1]).is_err());
    }
}
