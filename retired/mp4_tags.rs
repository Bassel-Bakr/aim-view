//! An MP4's or a MOV's metadata tags as ffmpeg writes them with `-movflags use_metadata_tags`: the `mdta` keys of a
//! `meta` box in moov/udta, or in moov.
//!
//! In: the video, read a box header at a time through disk.rs's `RangeReader` (a few small reads, then the boxes that
//! can hold the tags), never its frames. Out: one tag's value, as its bytes, for video_tags.rs.

use std::io;

use crate::disk::RangeReader;

/// A box header's bytes: its 32-bit size and its type.
const HEADER_BYTES: u64 = 8;
/// A box header's bytes with its 64-bit size after the type.
const LARGE_HEADER_BYTES: u64 = 16;
/// The 32-bit size that says the 64-bit size follows the type (a box over 4 GiB, such as a long recording's mdat).
const LARGE_SIZE: u64 = 1;
/// The 32-bit size that says the box runs to the end of the file, or of the box it is in.
const TO_END: u64 = 0;
/// The bytes of a full box's version and flags, before the rest of its body (`keys`, an ISO `meta`).
const VERSION_FLAGS_BYTES: usize = 4;
/// The bytes of a `keys` entry's size, or of its entry count.
const COUNT_BYTES: usize = 4;
/// The bytes of a `data` box's type indicator and locale, before its value.
const DATA_HEAD_BYTES: usize = 8;
/// The most boxes walked at one level: a real file has a handful, and a broken one never keeps the walk reading.
const MAX_BOXES: usize = 4096;
/// The most bytes of a box read whole to look for the tags in (moov/udta, moov/meta): a stats file takes tens of kB.
const MAX_TAGS_BYTES: u64 = 64 << 20;

/// A box: its type, and where its body starts and where the box ends, in bytes from the start of what it is in.
struct Mp4Box {
    /// Its four-byte type (`moov`, `udta`, `meta`...).
    kind: [u8; 4],
    /// Where its body starts: after its header.
    body: u64,
    /// Where it ends: its start plus its size.
    end: u64,
}

/// The box whose header starts `head` (read at `at`, in a span that ends at `end`); None when its size does not fit.
fn parse_header(head: &[u8], at: u64, end: u64) -> Option<Mp4Box> {
    let size_32 = u64::from(u32::from_be_bytes(head.get(0..4)?.try_into().ok()?));
    let kind: [u8; 4] = head.get(4..8)?.try_into().ok()?;
    let (size, body) = match size_32 {
        LARGE_SIZE => (u64::from_be_bytes(head.get(8..16)?.try_into().ok()?), at + LARGE_HEADER_BYTES),
        TO_END => (end - at, at + HEADER_BYTES),
        size => (size, at + HEADER_BYTES),
    };
    let box_end = at.checked_add(size)?;
    (box_end >= body && box_end <= end).then_some(Mp4Box { kind, body, end: box_end })
}

/// The boxes from `start` to `end`, one after another, each from its header (`head_at` gives up to a large header's
/// bytes at a place); `stop` ends the walk after the first box of that type. A broken header ends the walk.
fn walk(
    start: u64,
    end: u64,
    stop: Option<&[u8; 4]>,
    mut head_at: impl FnMut(u64, u64) -> io::Result<Vec<u8>>,
) -> io::Result<Vec<Mp4Box>> {
    let mut boxes = Vec::new();
    let mut at = start;
    while at + HEADER_BYTES <= end && boxes.len() < MAX_BOXES {
        let head = head_at(at, LARGE_HEADER_BYTES.min(end - at))?;
        let Some(found) = parse_header(&head, at, end) else { break };
        at = found.end;
        let stopped = stop == Some(&found.kind);
        boxes.push(found);
        if stopped {
            break;
        }
    }
    Ok(boxes)
}

/// The boxes in `bytes` (a box's body read whole).
fn boxes_in(bytes: &[u8]) -> Vec<Mp4Box> {
    let read = |at: u64, len: u64| Ok(bytes[at as usize..(at + len) as usize].to_vec());
    walk(0, bytes.len() as u64, None, read).unwrap_or_default()
}

/// A box's body in `bytes`, the bytes its place is counted in.
fn body_of<'a>(bytes: &'a [u8], found: &Mp4Box) -> Option<&'a [u8]> {
    bytes.get(usize::try_from(found.body).ok()?..usize::try_from(found.end).ok()?)
}

/// A big-endian u32 at byte `at`.
fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(bytes.get(at..at + COUNT_BYTES)?.try_into().ok()?))
}

/// The 1-based index the body of a `keys` box gives the `mdta` key `key` (any case): after its version, flags and
/// entry count, each entry is its size, its namespace and its name.
fn key_index(keys: &[u8], key: &str) -> Option<u32> {
    let count = u32_at(keys, VERSION_FLAGS_BYTES)?;
    let mut at = VERSION_FLAGS_BYTES + COUNT_BYTES;
    for index in 1..=count.min(MAX_BOXES as u32) {
        let size = u32_at(keys, at)? as usize;
        let entry = keys.get(at..at.checked_add(size)?)?;
        let named = entry.get(8..).is_some_and(|name| name.eq_ignore_ascii_case(key.as_bytes()));
        if entry.get(4..8) == Some(b"mdta") && named {
            return Some(index);
        }
        at += size.max(HEADER_BYTES as usize);
    }
    None
}

/// The value of the `mdta` tag `key` in a `meta` box's body: its `keys` box gives each key's index, its `ilst` box
/// holds a box per index with the value in its `data` box. An ISO `meta` (an MP4's) starts with its version and
/// flags, a QuickTime one (a MOV's) with its first box, its handler.
fn tag_in_meta(meta: &[u8], key: &str) -> Option<Vec<u8>> {
    let skip = if meta.get(4..8) == Some(b"hdlr") { 0 } else { VERSION_FLAGS_BYTES };
    let children = meta.get(skip..)?;
    let child = |kind: &[u8; 4]| {
        let found = boxes_in(children).into_iter().find(|found| &found.kind == kind)?;
        body_of(children, &found)
    };
    let index = key_index(child(b"keys")?, key)?;
    let items = child(b"ilst")?;
    let item = boxes_in(items).into_iter().find(|found| found.kind == index.to_be_bytes())?;
    let item_body = body_of(items, &item)?;
    let data = boxes_in(item_body).into_iter().find(|found| &found.kind == b"data")?;
    Some(body_of(item_body, &data)?.get(DATA_HEAD_BYTES..)?.to_vec())
}

/// The value of the `mdta` tag `key` (any case) in the MP4 or MOV `reader` reads; None when it has no such tag or its
/// boxes do not read. Reads the top level's box headers up to moov, moov's box headers, then its udta and meta boxes
/// whole.
pub(super) fn mdta_tag(reader: &mut RangeReader, key: &str) -> io::Result<Option<Vec<u8>>> {
    let size_bytes = reader.size_bytes()?;
    let top = walk(0, size_bytes, Some(b"moov"), |at, len| reader.read_range(at, len))?;
    let Some(moov) = top.into_iter().find(|found| &found.kind == b"moov") else { return Ok(None) };
    for child in walk(moov.body, moov.end, None, |at, len| reader.read_range(at, len))? {
        let holds_tags = &child.kind == b"udta" || &child.kind == b"meta";
        if !holds_tags || child.end - child.body > MAX_TAGS_BYTES {
            continue;
        }
        let body = reader.read_range(child.body, child.end - child.body)?;
        let metas: Vec<&[u8]> = if &child.kind == b"meta" {
            vec![&body]
        } else {
            boxes_in(&body)
                .iter()
                .filter(|found| &found.kind == b"meta")
                .filter_map(|found| body_of(&body, found))
                .collect()
        };
        if let Some(value) = metas.into_iter().find_map(|meta| tag_in_meta(meta, key)) {
            return Ok(Some(value));
        }
    }
    Ok(None)
}

/// A box of `kind` around `body`, for the tests.
#[cfg(test)]
fn mp4_box(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    [&(body.len() as u32 + 8).to_be_bytes()[..], kind, body].concat()
}

/// An MP4's bytes with `tags` (key, value) in moov/udta/meta after its frames, as ffmpeg writes them, for the tests.
#[cfg(test)]
pub(crate) fn tagged_mp4(tags: &[(&str, &[u8])]) -> Vec<u8> {
    let udta = mp4_box(b"udta", &mp4_box(b"meta", &tests::meta_body(tags, true)));
    [mp4_box(b"ftyp", b"isom"), mp4_box(b"mdat", &[7; 100]), mp4_box(b"moov", &udta)].concat()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::video_tags::tag as mdta_tag;
    use std::path::Path;

    /// A `meta` box's body with ffmpeg's `mdta` tags: the handler, the keys and the values; `iso`: with a version and
    /// flags first, as an MP4's.
    pub(super) fn meta_body(tags: &[(&str, &[u8])], iso: bool) -> Vec<u8> {
        let handler = mp4_box(b"hdlr", &[&[0; 8][..], b"mdta", &[0; 13]].concat());
        let entries: Vec<u8> = tags.iter().flat_map(|(key, _)| mp4_box(b"mdta", key.as_bytes())).collect();
        let keys = mp4_box(b"keys", &[&[0; 4][..], &(tags.len() as u32).to_be_bytes(), &entries].concat());
        let items: Vec<u8> = (1..)
            .zip(tags)
            .flat_map(|(index, (_, value))| {
                let data = mp4_box(b"data", &[&[0, 0, 0, 1, 0, 0, 0, 0][..], value].concat());
                mp4_box(&(index as u32).to_be_bytes(), &data)
            })
            .collect();
        let version_flags: &[u8] = if iso { &[0; 4] } else { &[] };
        [version_flags, &handler, &keys, &mp4_box(b"ilst", &items)].concat()
    }

    /// A test's own temporary folder, made empty.
    fn test_dir(test: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("aimview-mp4-tags-{test}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A file of `top` boxes, written in `dir`.
    fn video(dir: &Path, name: &str, top: &[Vec<u8>]) -> std::path::PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, top.concat()).unwrap();
        path
    }

    /// The tag is read from moov/udta/meta after the frames (an MP4 as ffmpeg writes it), from moov/meta before them
    /// (a MOV's, without a version), and past a 64-bit mdat size; a key not there, or a file with no moov, gives None.
    #[test]
    fn a_tag_is_read_where_ffmpeg_puts_it() {
        let dir = test_dir("where");
        let video = |name: &str, top: &[Vec<u8>]| video(&dir, name, top);
        let csv: &[u8] = b"Kill #,Timestamp\r\n1,17:43:05.387\r\nScore:,60120\r\n";
        let tags: &[(&str, &[u8])] = &[("kovobs_kovaaks_stats", csv), ("encoder", b"Lavf")];
        let ftyp = mp4_box(b"ftyp", b"isom");
        let mdat = mp4_box(b"mdat", &[7; 100]);
        let trak = mp4_box(b"trak", &[0; 40]);
        let udta = mp4_box(b"udta", &mp4_box(b"meta", &meta_body(tags, true)));
        let mp4 = video("a.mp4", &[ftyp.clone(), mdat.clone(), mp4_box(b"moov", &[trak.clone(), udta].concat())]);
        assert_eq!(mdta_tag(&mp4, "kovobs_kovaaks_stats").unwrap().as_deref(), Some(csv));
        assert_eq!(mdta_tag(&mp4, "encoder").unwrap().as_deref(), Some(&b"Lavf"[..]));
        assert_eq!(mdta_tag(&mp4, "KOVOBS_KOVAAKS_STATS").unwrap().as_deref(), Some(csv), "any case");
        assert_eq!(mdta_tag(&mp4, "other").unwrap(), None);

        let meta = mp4_box(b"meta", &meta_body(tags, false));
        let mov = video("a.mov", &[ftyp.clone(), mp4_box(b"moov", &[trak.clone(), meta].concat()), mdat]);
        assert_eq!(mdta_tag(&mov, "kovobs_kovaaks_stats").unwrap().as_deref(), Some(csv));

        let large_mdat = [&1u32.to_be_bytes()[..], b"mdat", &(16u64 + 4).to_be_bytes(), &[7; 4]].concat();
        let udta = mp4_box(b"udta", &mp4_box(b"meta", &meta_body(tags, true)));
        let large = video("large.mp4", &[ftyp.clone(), large_mdat, mp4_box(b"moov", &udta)]);
        assert_eq!(mdta_tag(&large, "kovobs_kovaaks_stats").unwrap().as_deref(), Some(csv));

        let no_moov = video("none.mp4", &[ftyp, mp4_box(b"mdat", &[7; 10])]);
        assert_eq!(mdta_tag(&no_moov, "kovobs_kovaaks_stats").unwrap(), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Broken boxes (a size past the end, a size under a header's, a file cut short, not an MP4 at all) give None,
    /// never a panic or an endless walk.
    #[test]
    fn broken_boxes_give_none() {
        let dir = test_dir("broken");
        let video = |name: &str, top: &[Vec<u8>]| video(&dir, name, top);
        let tags: &[(&str, &[u8])] = &[("kovobs_kovaaks_stats", b"x")];
        let udta = mp4_box(b"udta", &mp4_box(b"meta", &meta_body(tags, true)));
        let whole = [mp4_box(b"ftyp", b"isom"), mp4_box(b"moov", &udta)].concat();
        for cut in [whole.len() - 3, whole.len() / 2, 9, 3, 0] {
            let path = video("cut.mp4", &[whole[..cut].to_vec()]);
            assert_eq!(mdta_tag(&path, "kovobs_kovaaks_stats").unwrap(), None, "cut at {cut}");
        }
        let tiny = [&4u32.to_be_bytes()[..], b"ftyp", &[0; 32]].concat();
        assert_eq!(mdta_tag(&video("tiny.mp4", &[tiny]), "kovobs_kovaaks_stats").unwrap(), None);
        let text = video("text.mp4", &[b"Kill #,Timestamp\r\n1,17:43:05.387\r\n".to_vec()]);
        assert_eq!(mdta_tag(&text, "kovobs_kovaaks_stats").unwrap(), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
