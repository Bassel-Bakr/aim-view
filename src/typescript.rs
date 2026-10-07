//! Names for the tuples the reports write, for the TypeScript types only (ts-rs, feature `ts`): the UI names every
//! tuple type, and ts-rs writes a field's tuple in place. The fields that hold them say so with `#[ts(as = ...)]`.
//!
//! In: nothing at run time; no build uses these structs. Out: their TypeScript types, which `bun run types` writes
//! into ui/src/app/generated/.

use ts_rs::TS;

/// A target's place in one frame: frame, x, y (degrees from the crosshair).
#[derive(TS)]
#[ts(export)]
pub struct PathPoint(pub i64, pub f64, pub f64);

/// A target in a frame: its track id, x, y (degrees from the crosshair).
#[derive(TS)]
#[ts(export)]
pub struct TrackPoint(pub u32, pub f64, pub f64);

/// A target's size: width, height (degrees).
#[derive(TS)]
#[ts(export)]
pub struct TargetSize(pub f64, pub f64);

/// How far the view moved since the frame before: x, y (degrees).
#[derive(TS)]
#[ts(export)]
pub struct ViewShift(pub f64, pub f64);

/// A fixed screen spot where the detector marks the crosshair: x, y (degrees).
#[derive(TS)]
#[ts(export)]
pub struct CrosshairSpot(pub f64, pub f64);

/// A target's place from the crosshair at the click: x, y (degrees).
#[derive(TS)]
#[ts(export)]
pub struct TargetOffset(pub f64, pub f64);

/// A kill's time in its five steps, in seconds: react, main flick, onto the target, settle, still on the target.
#[derive(TS)]
#[ts(export)]
pub struct KillParts(pub f64, pub f64, pub f64, pub f64, pub f64);

/// A frame's offset from the bot's center line along its motion (positive: ahead) and across it, and its radius.
#[derive(TS)]
#[ts(export)]
pub struct AroundPoint(pub f64, pub f64, pub f64);

/// A second of a tracking run: the share of it on the bot, and the share switching between bots.
#[derive(TS)]
#[ts(export)]
pub struct SecondShares(pub f64, pub f64);

/// An area a review leaves out: x0, y0, x1, y1 (shares of the frame), and its kind's id.
#[derive(TS)]
#[ts(export)]
pub struct AreaBox(pub f64, pub f64, pub f64, pub f64, pub String);

/// A bot's death: its frame, the frame the crosshair is on a bot again, and the first frame a bot shows.
#[derive(TS)]
#[ts(export)]
pub struct Switch(pub usize, pub usize, pub usize);

/// A box on a crop: center x, center y, width, height (crop pixels; a shape's before it is turned).
#[derive(TS)]
#[ts(export)]
pub struct CropBox(pub f64, pub f64, pub f64, pub f64);

/// A box's vertex placed by hand: x, y (crop pixels).
#[derive(TS)]
#[ts(export)]
pub struct CropVertex(pub f64, pub f64);

/// Where a box's far face sits from its near one: x, y (crop pixels).
#[derive(TS)]
#[ts(export)]
pub struct FaceOffset(pub f64, pub f64);
