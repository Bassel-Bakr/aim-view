//! The area finder (src/areas.rs, python/areas.py) on a recording: its frames read, and what it found kept in the
//! recording's folder as python/areas.py keeps it (areas.json: the found areas; areas_maps.npz: the stand-out and
//! change maps). A review keeps what the finder found in the key frames it reads anyway (review.rs); a recording not
//! reviewed yet is read here when its areas are first asked for (in the browser build the page reads it, with the
//! core's finder in a worker, and sends what it found: library/browser.rs). In: a video, or a review's finds. Out:
//! those two files, which areas.rs reads for /api/find_areas.

use std::path::Path;

use aimview::areas::{Area, Found, Maps};
#[cfg(feature = "native")]
use aimview::areas::{AreaFinder, FRAME, sample_frames};
#[cfg(feature = "native")]
use aimview::convert::Converter;
use aimview::convert::{DST_H, DST_W};
#[cfg(feature = "native")]
use aimview::hud::HudWatch;

use crate::npz::{self, Array};
use crate::pyjson;
#[cfg(feature = "native")]
use crate::review::{fixed_map, frame_bytes};
#[cfg(feature = "native")]
use crate::video::{Frames, probe};

const FOUND: &str = "areas.json";
const MAPS: &str = "areas_maps.npz";

/// Finds the recording's areas (python/areas.py: analyse): from its key frames, or when it has too few, from the frames
/// `sample_frames` picks over the whole recording; KovaaK's session box from the key frames.
#[cfg(feature = "native")]
pub fn analyse(video: &Path) -> Result<Found, String> {
    crate::ffmpeg::ensure(|_, _| {})?;
    let info = probe(video)?;
    let picks = sample_frames(info.keys.len(), &info.times, info.duration);
    let mut finder = AreaFinder::new();
    let mut hud = HudWatch::new(info.width, info.height, info.full);
    fixed_map(video, &info, |small, luma| {
        hud.add_key(luma);
        if picks.is_none() {
            finder.add(small);
        }
    })?;
    if let Some(picks) = picks {
        let mut frames = Frames::open(video, None, None)?;
        let mut convert = Converter::new(info.width, info.height, info.matrix, info.full);
        let (mut yuv, mut small) = (vec![0u8; frame_bytes(&info)], vec![0u8; FRAME]);
        let mut next = picks.iter().peekable();
        let mut frame = 0;
        while next.peek().is_some() && frames.next_into(&mut yuv)? {
            if next.peek() == Some(&&frame) {
                convert.yuv420p(&yuv, &mut small);
            }
            // a frame picked more than once is given again
            while next.peek() == Some(&&frame) {
                finder.add(&small);
                next.next();
            }
            frame += 1;
        }
    }
    Ok(finder.finish(hud.session_box()))
}

/// The found areas kept for the recording in `dir`, if any.
pub fn found(dir: &Path) -> Option<Vec<Area>> {
    serde_json::from_value(pyjson::load(&dir.join(FOUND))?).ok()
}

/// The maps kept for the recording in `dir`, if any.
pub fn maps(dir: &Path) -> Option<Maps> {
    let file = dir.join(MAPS);
    Maps::new(npz::load(&file, "stand").ok()?.data, npz::load(&file, "change").ok()?.data)
}

/// Keeps what the finder found for the recording in `dir` where nothing is kept yet: with no found areas kept, both
/// files; else the maps when they are missing (python/areas.py: find, maps).
pub fn keep(dir: &Path, found: &Found) -> Result<(), String> {
    let fresh = !crate::disk::exists(dir.join(FOUND));
    if fresh {
        pyjson::dump(&dir.join(FOUND), &found.areas, false)?;
    }
    if fresh || !crate::disk::exists(dir.join(MAPS)) {
        let stand = Array::u8(&[DST_H, DST_W], found.maps.stand().to_vec());
        let change = Array::u8(&[DST_H, DST_W], found.maps.change().to_vec());
        npz::save(&dir.join(MAPS), &[("stand", &stand), ("change", &change)])?;
    }
    Ok(())
}

/// Keeps what a review's finder found (`models` is the review's folder, models/<model> in the recording's): a review
/// is not failed for it.
pub fn keep_with_review(models: &Path, found: Option<&Found>) -> Result<(), String> {
    if let (Some(dir), Some(found)) = (models.parent().and_then(Path::parent), found)
        && let Err(error) = keep(dir, found)
    {
        eprintln!("the found areas could not be kept: {error}");
    }
    Ok(())
}
