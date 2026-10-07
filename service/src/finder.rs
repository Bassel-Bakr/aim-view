//! The area finder (src/areas.rs, python/areas.py) on a recording: its frames read, and what it found kept with the
//! recording as python/areas.py keeps it (store.rs; areas.json: the found areas; areas_maps.npz: the stand-out and
//! change maps). A review keeps what the finder found in the key frames it reads anyway (review.rs); a recording not
//! reviewed yet is read here when its areas are first asked for (in the browser build the page reads it, with the
//! core's finder in a worker, and sends what it found: library/browser.rs). In: a video, or a review's finds. Out:
//! those two items, which areas.rs reads for /api/find_areas.

#[cfg(feature = "native")]
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
use crate::store::{Item, Mark, Store};
#[cfg(feature = "native")]
use crate::video::{Frames, probe};

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

/// The found areas kept for the recording `id`, if any.
pub fn found(store: &dyn Store, id: &str) -> Option<Vec<Area>> {
    serde_json::from_value(pyjson::load(store, Item::Mark(id, Mark::FoundAreas))?).ok()
}

/// The maps kept for the recording `id`, if any.
pub fn maps(store: &dyn Store, id: &str) -> Option<Maps> {
    let bytes = store.read(Item::Mark(id, Mark::FoundMaps)).ok()??;
    Maps::new(npz::array(&bytes, "stand").ok()?.data, npz::array(&bytes, "change").ok()?.data)
}

/// Keeps what the finder found for the recording `id` where nothing is kept yet: with no found areas kept, both items;
/// else the maps when they are missing (python/areas.py: find, maps).
pub fn keep(store: &dyn Store, id: &str, found: &Found) -> Result<(), String> {
    let (areas, maps) = (Item::Mark(id, Mark::FoundAreas), Item::Mark(id, Mark::FoundMaps));
    let fresh = !store.has(areas);
    if fresh {
        pyjson::dump(store, areas, &found.areas, false)?;
    }
    if fresh || !store.has(maps) {
        let stand = Array::u8(&[DST_H, DST_W], found.maps.stand().to_vec());
        let change = Array::u8(&[DST_H, DST_W], found.maps.change().to_vec());
        let bytes = npz::to_bytes(&[("stand", &stand), ("change", &change)])?;
        store.write(maps, &bytes).map_err(|error| format!("{}: {error}", store.name(maps)))?;
    }
    Ok(())
}

/// Keeps what a review of the recording `id` found: a review is not failed for it.
pub fn keep_with_review(store: &dyn Store, id: &str, found: Option<&Found>) -> Result<(), String> {
    if let Some(found) = found
        && let Err(error) = keep(store, id, found)
    {
        eprintln!("the found areas could not be kept: {error}");
    }
    Ok(())
}
