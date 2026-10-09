//! The shapes a target is drawn with on a crop, as their outline on screen (the targets are 3D). A pill (a sphere is a
//! pill with equal sides), an oval (a sphere stretched by the camera's perspective) and a box (a square or a cube), each
//! turned to any angle. Each can have a third face, the offset of its far end, for a target seen at an angle: a cube's
//! outline is then a hexagon, a deep pill's or oval's the shape swept back to its far end. Or a pill or a box can be
//! solid: a box or a capsule with a thickness, tipped and
//! swung out of the screen's plane, its outline what that solid shows the camera. A box's vertices can also be placed
//! one by one (`points`: a flat box's 4 corners, a 3D box's 8), for a target seen in perspective: its outline is then
//! what they span. Shapes are joined into targets (a bot's head and body), ordered
//! front to back by depth (a shape hides the parts of shapes behind it), and some only hide what is behind them
//! (occluders: the crosshair, a pillar, an overlay).
//!
//! In: a scene, as the Crops page saves it (service/src/crops.rs). Out: each target's visible pixels and box
//! (`visible`), for the page (src/wasm.rs `shapes_visible`) and the training set (aimview-tool crop-labels, read by
//! python/model/checked_data.py). Coordinates are crop pixels, pixel i at coordinate i (as checked_data.py's
//! `ellipse_mask` reads a box); an angle is in degrees, clockwise on screen.

use serde::{Deserialize, Serialize};

/// The target shapes: KovaaK's two, and the oval perspective stretches a sphere into.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum ShapeKind {
    /// Two half circles joined by straight sides; a circle when its sides are equal (a sphere).
    Pill,
    /// An ellipse ("ellipse" in the JSON): a sphere stretched by the camera's perspective, which a pill cannot fit. It
    /// is never solid and never has its points placed (`check` refuses both).
    Ellipse,
    /// A rectangle (a square or a cube's face), or the solid or hand-placed box its other fields give.
    Box,
}

/// The part of a bot a shape stands for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum ShapeRole {
    /// The bot's head.
    Head,
    /// The bot's body.
    Body,
}

/// One shape: its kind, its frame before turning ([center x, center y, width, height], pixels), its angle (degrees,
/// clockwise), a third face (the offset of the far end, pixels), its solid (a 3D shape's thickness and tumble), a
/// box's vertices placed by hand (crop pixels; they decide its outline when given), its depth (greater is nearer), its
/// role, and the model box it started from (an index into the crop's boxes), if any.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Shape {
    /// The shape's id in the scene, which `Scene::targets` and `Scene::occluders` name it by.
    pub id: String,
    /// Pill, oval or box.
    pub kind: ShapeKind,
    /// Its frame before turning: [center x, center y, width, height], crop pixels ("box" in the JSON).
    #[serde(rename = "box")]
    #[cfg_attr(feature = "ts", ts(rename = "box", as = "crate::typescript::CropBox"))]
    pub frame: [f64; 4],
    /// Its turn about its center, in degrees clockwise on screen.
    #[serde(default)]
    pub angle: f64,
    /// The offset of its far end from its near one, crop pixels [x, y]; None for a flat shape.
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(as = "Option<crate::typescript::FaceOffset>"))]
    pub face: Option<[f64; 2]>,
    /// Its thickness and turn out of the screen's plane, for a solid shape; when given it decides the outline
    /// instead of `face`.
    #[serde(default)]
    pub solid: Option<Solid>,
    /// A box's vertices placed by hand, crop pixels (4 for a flat box, 8 for a 3D one); when there are 3 or more they
    /// decide its outline.
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(as = "Option<Vec<crate::typescript::CropVertex>>"))]
    pub points: Option<Vec<[f64; 2]>>,
    /// Its place front to back: greater is nearer, and a nearer shape hides the parts of those behind it.
    #[serde(default)]
    pub depth: i32,
    /// The part of a bot it stands for, if it is one.
    #[serde(default)]
    pub role: Option<ShapeRole>,
    /// The model box it started from, an index into the crop's boxes; None for one drawn from nothing.
    #[serde(default)]
    pub model: Option<usize>,
}

/// A 3D shape's thickness and its turn out of the screen's plane, for a target seen at an angle. It is drawn as the
/// solid seen from the camera, straight on (a parallel projection): a box with each corner where the turn puts it; a
/// pill as a capsule, round (as thick as its short side), whose axis the turn tips toward or away from the camera.
/// Turns apply tip, then swing, then the shape's angle; x runs to the right, y down, z away from the camera.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Solid {
    /// A box's size front to back, pixels.
    pub thickness: f64,
    /// Degrees its top leans toward the camera, so its top face shows (about the screen's x axis).
    pub tip: f64,
    /// Degrees its right side turns toward the camera, so its right face shows (about the screen's y axis).
    pub swing: f64,
}

/// A crop's shapes: which are joined into one target (a shape in no group, and no occluder, is a target of its own),
/// and which only hide what is behind them.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Scene {
    /// Every shape on the crop, targets' and occluders'.
    pub shapes: Vec<Shape>,
    /// The groups of shapes joined into one target, each by its shapes' ids.
    #[serde(default)]
    pub targets: Vec<Vec<String>>,
    /// The ids of the shapes that only hide what is behind them.
    #[serde(default)]
    pub occluders: Vec<String>,
}

/// One target as the scene shows it: its shapes, the box round its visible pixels ([center x, center y, width,
/// height]; None when none shows), the box round all its shapes (hidden parts too), whether something hides all of
/// it, and its visible pixels as run lengths (`runs`).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct TargetView {
    /// The ids of the target's shapes.
    pub shapes: Vec<String>,
    /// The box round its visible pixels, [center x, center y, width, height] in crop pixels ("box" in the JSON); None
    /// when none shows.
    #[serde(rename = "box")]
    #[cfg_attr(feature = "ts", ts(rename = "box", as = "Option<crate::typescript::CropBox>"))]
    pub frame: Option<[f64; 4]>,
    /// The box round all its shapes, hidden parts too, in the same form.
    #[cfg_attr(feature = "ts", ts(as = "crate::typescript::CropBox"))]
    pub whole: [f64; 4],
    /// Whether nearer shapes hide all of it.
    pub hidden: bool,
    /// Its visible pixels as run lengths over the crop (`runs`).
    pub runs: Vec<u32>,
}

/// A scene seen on a crop: its targets, and the pixels of all of them (the training `tmask`) as run lengths.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct SceneView {
    /// Each target, in the order `targets` gives them.
    pub targets: Vec<TargetView>,
    /// Every target's visible pixels as run lengths over the crop (`runs`).
    pub mask: Vec<u32>,
}

/// A pill's ends and a box's outline are drawn with this many points per half circle.
const ARC_POINTS: usize = 16;
/// An oval's outline is drawn with this many points, a multiple of 4 so the tips of its axes are among them. A chord
/// strays at most a * (1 - cos(pi / ELLIPSE_POINTS)) from the curve, a its longer half axis: 0.14 px for one as long as a
/// crop's side (256 px).
const ELLIPSE_POINTS: usize = 96;
/// The largest frame side, angle or face (pixels or degrees) a shape may have: anything past it is a slip.
const MAX_VALUE: f64 = 1e5;

/// A point of a shape's own frame (along its width, across it) on the crop.
fn to_crop(shape: &Shape, along: f64, across: f64) -> [f64; 2] {
    let (sin, cos) = shape.angle.to_radians().sin_cos();
    [shape.frame[0] + along * cos - across * sin, shape.frame[1] + along * sin + across * cos]
}

/// A crop point in a shape's own frame: (along its width, across it).
fn to_own(shape: &Shape, x: f64, y: f64) -> (f64, f64) {
    let (sin, cos) = shape.angle.to_radians().sin_cos();
    let (dx, dy) = (x - shape.frame[0], y - shape.frame[1]);
    (dx * cos + dy * sin, -dx * sin + dy * cos)
}

/// A box's four corners on the crop, in order round it.
fn corners(shape: &Shape) -> [[f64; 2]; 4] {
    let (half_w, half_h) = (shape.frame[2] / 2.0, shape.frame[3] / 2.0);
    [
        to_crop(shape, -half_w, -half_h),
        to_crop(shape, half_w, -half_h),
        to_crop(shape, half_w, half_h),
        to_crop(shape, -half_w, half_h),
    ]
}

/// The convex hull of points, counterclockwise in math's terms (Andrew's monotone chain).
fn convex_hull(mut points: Vec<[f64; 2]>) -> Vec<[f64; 2]> {
    points.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
    points.dedup();
    if points.len() < 3 {
        return points;
    }
    let turn = |from: &[f64; 2], a: &[f64; 2], b: &[f64; 2]| {
        (a[0] - from[0]) * (b[1] - from[1]) - (a[1] - from[1]) * (b[0] - from[0])
    };
    let mut hull: Vec<[f64; 2]> = Vec::with_capacity(2 * points.len());
    for pass in [points.clone(), points.iter().rev().copied().collect()] {
        let start = hull.len();
        for point in pass {
            while hull.len() >= start + 2 && turn(&hull[hull.len() - 2], &hull[hull.len() - 1], &point) <= 0.0 {
                hull.pop();
            }
            hull.push(point);
        }
        hull.pop();
    }
    hull
}

/// A box's outline: its turned rectangle, or with a third face the hull of the rectangle and the rectangle moved by
/// the face.
fn box_outline(shape: &Shape) -> Vec<[f64; 2]> {
    let near = corners(shape);
    match shape.face {
        None => near.to_vec(),
        Some([dx, dy]) => {
            convex_hull(near.iter().flat_map(|corner| [*corner, [corner[0] + dx, corner[1] + dy]]).collect())
        }
    }
}

/// A pill's radius (half its short side) and the half length of its middle segment, along its long side.
fn pill_parts(shape: &Shape) -> (f64, f64) {
    let radius = shape.frame[2].min(shape.frame[3]) / 2.0;
    (radius, shape.frame[2].max(shape.frame[3]) / 2.0 - radius)
}

/// A pill's outline: two half circles joined, ARC_POINTS a half circle; with a third face, the hull of the pill and
/// the pill moved by the face.
fn pill_outline(shape: &Shape) -> Vec<[f64; 2]> {
    let near = flat_pill_outline(shape);
    match shape.face {
        None => near,
        Some([dx, dy]) => convex_hull(near.iter().flat_map(|point| [*point, [point[0] + dx, point[1] + dy]]).collect()),
    }
}

/// A pill's outline without its third face.
fn flat_pill_outline(shape: &Shape) -> Vec<[f64; 2]> {
    let (radius, half) = pill_parts(shape);
    let wide = shape.frame[2] >= shape.frame[3];
    let mut points = Vec::with_capacity(2 * (ARC_POINTS + 1));
    for (end, start) in [(half, -90.0_f64), (-half, 90.0)] {
        for step in 0..=ARC_POINTS {
            let turn = (start + 180.0 * step as f64 / ARC_POINTS as f64).to_radians();
            let (along, across) = (end + radius * turn.cos(), radius * turn.sin());
            points.push(if wide { to_crop(shape, along, across) } else { to_crop(shape, across, along) });
        }
    }
    points
}

/// An oval's outline: ELLIPSE_POINTS round it; with a third face, the hull of the oval and the oval moved by the face.
fn ellipse_outline(shape: &Shape) -> Vec<[f64; 2]> {
    let (half_w, half_h) = (shape.frame[2] / 2.0, shape.frame[3] / 2.0);
    let near: Vec<[f64; 2]> = (0..ELLIPSE_POINTS)
        .map(|step| {
            let turn = (360.0 * step as f64 / ELLIPSE_POINTS as f64).to_radians();
            to_crop(shape, half_w * turn.cos(), half_h * turn.sin())
        })
        .collect();
    match shape.face {
        None => near,
        Some([dx, dy]) => convex_hull(near.iter().flat_map(|point| [*point, [point[0] + dx, point[1] + dy]]).collect()),
    }
}

/// A solid shape's turn as a matrix, Rz(angle) Ry(swing) Rx(tip): a point of its own frame (along its width, across
/// it, front to back) times the matrix is where the turn puts it.
fn rotation(shape: &Shape, solid: &Solid) -> [[f64; 3]; 3] {
    let (sin_z, cos_z) = shape.angle.to_radians().sin_cos();
    let (sin_y, cos_y) = solid.swing.to_radians().sin_cos();
    let (sin_x, cos_x) = solid.tip.to_radians().sin_cos();
    [
        [cos_z * cos_y, cos_z * sin_y * sin_x - sin_z * cos_x, cos_z * sin_y * cos_x + sin_z * sin_x],
        [sin_z * cos_y, sin_z * sin_y * sin_x + cos_z * cos_x, sin_z * sin_y * cos_x - cos_z * sin_x],
        [-sin_y, cos_y * sin_x, cos_y * cos_x],
    ]
}

/// A point of a solid shape's own frame (along its width, across it, front to back; pixels) on the crop.
fn solid_point(shape: &Shape, turn: &[[f64; 3]; 3], own: [f64; 3]) -> [f64; 2] {
    let moved = |row: &[f64; 3]| row[0] * own[0] + row[1] * own[1] + row[2] * own[2];
    [shape.frame[0] + moved(&turn[0]), shape.frame[1] + moved(&turn[1])]
}

/// A solid box's eight corners on the crop.
fn solid_corners(shape: &Shape, solid: &Solid) -> Vec<[f64; 2]> {
    let turn = rotation(shape, solid);
    let half = [shape.frame[2] / 2.0, shape.frame[3] / 2.0, solid.thickness / 2.0];
    (0..8_usize)
        .map(|corner| {
            let side = |axis: usize| if corner >> axis & 1 == 0 { -half[axis] } else { half[axis] };
            solid_point(shape, &turn, [side(0), side(1), side(2)])
        })
        .collect()
}

/// A solid pill's middle segment on the crop: its axis, tipped toward or away from the camera.
fn solid_pill_segment(shape: &Shape, solid: &Solid) -> [[f64; 2]; 2] {
    let (_, half) = pill_parts(shape);
    let turn = rotation(shape, solid);
    let wide = shape.frame[2] >= shape.frame[3];
    let end = |sign: f64| {
        let own = if wide { [sign * half, 0.0, 0.0] } else { [0.0, sign * half, 0.0] };
        solid_point(shape, &turn, own)
    };
    [end(-1.0), end(1.0)]
}

/// The outline of the points within a radius of a segment: its ends' circles and the lines joining them.
fn stadium(ends: [[f64; 2]; 2], radius: f64) -> Vec<[f64; 2]> {
    let steps = 2 * ARC_POINTS;
    let points = ends.iter().flat_map(|center| {
        (0..steps).map(move |step| {
            let turn = (360.0 * step as f64 / steps as f64).to_radians();
            [center[0] + radius * turn.cos(), center[1] + radius * turn.sin()]
        })
    });
    convex_hull(points.collect())
}

/// A box's vertices placed by hand, when there are enough to span an area.
fn free_points(shape: &Shape) -> Option<&Vec<[f64; 2]>> {
    shape.points.as_ref().filter(|points| shape.kind == ShapeKind::Box && points.len() >= 3)
}

/// A shape's outline on the crop, in order round it.
pub fn outline(shape: &Shape) -> Vec<[f64; 2]> {
    if let Some(points) = free_points(shape) {
        return convex_hull(points.clone());
    }
    match (shape.kind, &shape.solid) {
        (ShapeKind::Pill, Some(solid)) => stadium(solid_pill_segment(shape, solid), pill_parts(shape).0),
        (ShapeKind::Box, Some(solid)) => convex_hull(solid_corners(shape, solid)),
        (ShapeKind::Pill, None) => pill_outline(shape),
        (ShapeKind::Box, None) => box_outline(shape),
        (ShapeKind::Ellipse, _) => ellipse_outline(shape),
    }
}

/// Whether a point is inside a convex polygon (its points in order round it, either way).
fn in_convex(polygon: &[[f64; 2]], x: f64, y: f64) -> bool {
    let mut sign = 0.0_f64;
    for (i, a) in polygon.iter().enumerate() {
        let b = polygon[(i + 1) % polygon.len()];
        let side = (b[0] - a[0]) * (y - a[1]) - (b[1] - a[1]) * (x - a[0]);
        if side != 0.0 {
            if sign != 0.0 && side.signum() != sign {
                return false;
            }
            sign = side.signum();
        }
    }
    polygon.len() >= 3
}

/// A pill's middle segment on the crop, its two ends.
fn pill_segment(shape: &Shape) -> [[f64; 2]; 2] {
    let (_, half) = pill_parts(shape);
    let end = |sign: f64| {
        if shape.frame[2] >= shape.frame[3] {
            to_crop(shape, sign * half, 0.0)
        } else {
            to_crop(shape, 0.0, sign * half)
        }
    };
    [end(-1.0), end(1.0)]
}

/// The distance from a point to a segment, in crop pixels.
fn segment_distance([x, y]: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let (along_x, along_y) = (b[0] - a[0], b[1] - a[1]);
    let length_sq = along_x * along_x + along_y * along_y;
    let share =
        if length_sq > 0.0 { (((x - a[0]) * along_x + (y - a[1]) * along_y) / length_sq).clamp(0.0, 1.0) } else { 0.0 };
    (x - a[0] - share * along_x).hypot(y - a[1] - share * along_y)
}

/// Whether a crop point is in a pill with a third face: within its radius of the parallelogram its middle segment
/// sweeps on the way to its far end.
fn in_deep_pill(shape: &Shape, [dx, dy]: [f64; 2], x: f64, y: f64) -> bool {
    let (radius, _) = pill_parts(shape);
    let [a, b] = pill_segment(shape);
    let sweep = [a, b, [b[0] + dx, b[1] + dy], [a[0] + dx, a[1] + dy]];
    let area = (b[0] - a[0]) * dy - (b[1] - a[1]) * dx;
    if area.abs() > f64::EPSILON && in_convex(&sweep, x, y) {
        return true;
    }
    (0..sweep.len()).any(|i| segment_distance([x, y], sweep[i], sweep[(i + 1) % sweep.len()]) <= radius)
}

/// Whether a crop point is in an oval, or with a third face in the band it sweeps to its far end. Stretched along its
/// width until it is a circle of radius half its height (the stretch maps the oval moved along its face to that circle
/// moved along the stretched face), the point is within that radius of the segment from the center to the stretched
/// face (no segment for a flat oval).
fn in_ellipse(shape: &Shape, x: f64, y: f64) -> bool {
    let radius = shape.frame[3] / 2.0;
    let stretch = shape.frame[3] / shape.frame[2];
    let (along, across) = to_own(shape, x, y);
    let [dx, dy] = shape.face.unwrap_or([0.0, 0.0]);
    let (face_along, face_across) = to_own(shape, shape.frame[0] + dx, shape.frame[1] + dy);
    segment_distance([along * stretch, across], [0.0, 0.0], [face_along * stretch, face_across]) <= radius
}

/// Whether a crop point is inside a shape: an oval by `in_ellipse`; a pill holds the points within its radius of its middle segment (of the
/// band it sweeps to its far end, with a third face; of its tipped axis, solid); a box those in its turned rectangle,
/// or in its outline when it has a third face, is solid or has its vertices placed.
pub fn contains(shape: &Shape, x: f64, y: f64) -> bool {
    if free_points(shape).is_some() {
        return in_convex(&outline(shape), x, y);
    }
    if shape.kind == ShapeKind::Ellipse {
        return in_ellipse(shape, x, y);
    }
    if let Some(solid) = &shape.solid {
        return match shape.kind {
            ShapeKind::Pill => {
                let [a, b] = solid_pill_segment(shape, solid);
                segment_distance([x, y], a, b) <= pill_parts(shape).0
            }
            ShapeKind::Box | ShapeKind::Ellipse => in_convex(&outline(shape), x, y),
        };
    }
    match (shape.kind, shape.face) {
        (ShapeKind::Pill, Some(face)) => in_deep_pill(shape, face, x, y),
        (ShapeKind::Pill, None) => {
            let (radius, half) = pill_parts(shape);
            let (along, across) = to_own(shape, x, y);
            let (long, short) = if shape.frame[2] >= shape.frame[3] { (along, across) } else { (across, along) };
            (long.abs() - half).max(0.0).hypot(short) <= radius
        }
        (ShapeKind::Box, None) => {
            let (along, across) = to_own(shape, x, y);
            along.abs() <= shape.frame[2] / 2.0 && across.abs() <= shape.frame[3] / 2.0
        }
        (ShapeKind::Box, Some(_)) => in_convex(&box_outline(shape), x, y),
        (ShapeKind::Ellipse, _) => in_ellipse(shape, x, y),
    }
}

/// The box round a shape's outline: [x0, y0, x1, y1].
fn bounds(shape: &Shape) -> [f64; 4] {
    outline(shape).iter().fold([f64::MAX, f64::MAX, f64::MIN, f64::MIN], |b, point| {
        [b[0].min(point[0]), b[1].min(point[1]), b[2].max(point[0]), b[3].max(point[1])]
    })
}

/// Why a scene cannot be saved, or Ok: every id once, sizes positive and finite, no oval solid or with its points
/// placed, every joined or hiding id a shape's, no shape in two targets, and no occluder in a target.
pub fn check(scene: &Scene) -> Result<(), String> {
    let mut ids = std::collections::HashSet::new();
    for shape in &scene.shapes {
        if !ids.insert(shape.id.as_str()) {
            return Err(format!("the shape id {} is used twice", shape.id));
        }
        let solid = shape.solid.iter().flat_map(|solid| [solid.thickness, solid.tip, solid.swing]);
        let points = shape.points.iter().flatten().flatten().copied();
        let mut numbers = shape
            .frame
            .into_iter()
            .chain([shape.angle])
            .chain(shape.face.into_iter().flatten())
            .chain(solid)
            .chain(points);
        if numbers.any(|value| !value.is_finite() || value.abs() > MAX_VALUE) {
            return Err(format!("the shape {} has a number out of range", shape.id));
        }
        let flat = shape.solid.is_some_and(|solid| solid.thickness <= 0.0);
        if shape.frame[2] <= 0.0 || shape.frame[3] <= 0.0 || flat {
            return Err(format!("the shape {} has no size", shape.id));
        }
        if shape.kind == ShapeKind::Ellipse && (shape.solid.is_some() || shape.points.is_some()) {
            return Err(format!("the oval {} cannot be solid or have its points placed", shape.id));
        }
    }
    let mut joined = std::collections::HashSet::new();
    for id in scene.targets.iter().flatten().chain(&scene.occluders) {
        if !ids.contains(id.as_str()) {
            return Err(format!("no shape has the id {id}"));
        }
        if !joined.insert(id.as_str()) {
            return Err(format!("the shape {id} is in two targets, or a target and the occluders"));
        }
    }
    Ok(())
}

/// The scene's targets, as indexes into its shapes: the joined ones in order, then each shape in no group that is no
/// occluder.
fn target_groups(scene: &Scene) -> Vec<Vec<usize>> {
    let index = |id: &String| scene.shapes.iter().position(|shape| &shape.id == id);
    let mut groups: Vec<Vec<usize>> = scene
        .targets
        .iter()
        .map(|group| group.iter().filter_map(index).collect::<Vec<usize>>())
        .filter(|group| !group.is_empty())
        .collect();
    let occluders: Vec<usize> = scene.occluders.iter().filter_map(index).collect();
    let alone: Vec<usize> = (0..scene.shapes.len())
        .filter(|i| !occluders.contains(i) && !groups.iter().flatten().any(|j| j == i))
        .collect();
    groups.extend(alone.into_iter().map(|i| vec![i]));
    groups
}

/// A shape's pixels on a crop of width x height (1 inside), tested only within the box round its outline.
fn shape_mask(shape: &Shape, width: usize, height: usize) -> Vec<u8> {
    let mut mask = vec![0u8; width * height];
    let [x0, y0, x1, y1] = bounds(shape);
    let clamp = |value: f64, end: usize| (value.max(0.0) as usize).min(end);
    for y in clamp(y0.floor(), height)..clamp(y1.ceil() + 1.0, height) {
        for x in clamp(x0.floor(), width)..clamp(x1.ceil() + 1.0, width) {
            if contains(shape, x as f64, y as f64) {
                mask[y * width + x] = 1;
            }
        }
    }
    mask
}

/// The box round a mask's set pixels, [center x, center y, width, height] with pixel i spanning one pixel round
/// coordinate i; None when no pixel is set.
fn mask_box(mask: &[u8], width: usize) -> Option<[f64; 4]> {
    let mut edges: Option<[usize; 4]> = None;
    for at in (0..mask.len()).filter(|&at| mask[at] != 0) {
        let (x, y) = (at % width, at / width);
        edges = Some(edges.map_or([x, y, x, y], |old| [old[0].min(x), old[1].min(y), old[2].max(x), old[3].max(y)]));
    }
    edges.map(|[x0, y0, x1, y1]| {
        [(x0 + x1) as f64 / 2.0, (y0 + y1) as f64 / 2.0, (x1 - x0 + 1) as f64, (y1 - y0 + 1) as f64]
    })
}

/// The box round shapes' outlines, [center x, center y, width, height].
fn shapes_box(shapes: &[&Shape]) -> [f64; 4] {
    let [x0, y0, x1, y1] =
        shapes.iter().map(|shape| bounds(shape)).fold([f64::MAX, f64::MAX, f64::MIN, f64::MIN], |b, one| {
            [b[0].min(one[0]), b[1].min(one[1]), b[2].max(one[2]), b[3].max(one[3])]
        });
    [(x0 + x1) / 2.0, (y0 + y1) / 2.0, x1 - x0, y1 - y0]
}

/// A mask as run lengths, row by row: the number of unset pixels, then of set ones, and so on (the first run may be
/// 0).
pub fn runs(mask: &[u8]) -> Vec<u32> {
    let mut out = Vec::new();
    let (mut current, mut length) = (0u8, 0u32);
    for &pixel in mask {
        let set = u8::from(pixel != 0);
        if set != current {
            out.push(length);
            (current, length) = (set, 0);
        }
        length += 1;
    }
    out.push(length);
    out
}

/// A mask of `len` pixels from its run lengths (`runs`).
pub fn from_runs(lengths: &[u32], len: usize) -> Vec<u8> {
    let mut mask = Vec::with_capacity(len);
    for (i, &length) in lengths.iter().enumerate() {
        mask.extend(std::iter::repeat_n(u8::from(i % 2 == 1), length as usize));
    }
    mask.resize(len, 0);
    mask
}

/// What a scene shows on a crop of width x height: each target's visible pixels (its shapes minus every pixel a
/// shape of greater depth covers, of another target or an occluder; a target's own shapes never hide each other), the
/// box round them (the shapes' own box when nothing hides any of them), whether something hides all of it, and the
/// pixels of all targets.
pub fn visible(scene: &Scene, width: usize, height: usize) -> SceneView {
    let masks: Vec<Vec<u8>> = scene.shapes.iter().map(|shape| shape_mask(shape, width, height)).collect();
    let mut all = vec![0u8; width * height];
    let mut targets = Vec::new();
    for group in target_groups(scene) {
        let (union, seen) = group_pixels(scene, &masks, &group, width * height);
        let (covered, shown) = (count(&union), count(&seen));
        let members: Vec<&Shape> = group.iter().map(|&i| &scene.shapes[i]).collect();
        let whole = shapes_box(&members);
        let frame = match shown {
            0 => None,
            _ if shown == covered => Some(whole),
            _ => mask_box(&seen, width),
        };
        all.iter_mut().zip(&seen).for_each(|(pixel, &set)| *pixel |= set);
        targets.push(TargetView {
            shapes: members.iter().map(|shape| shape.id.clone()).collect(),
            frame,
            whole,
            hidden: covered > 0 && shown == 0,
            runs: runs(&seen),
        });
    }
    SceneView { targets, mask: runs(&all) }
}

/// A target's pixels (its shapes' union) and the visible ones among them: those no shape of another target, or no
/// occluder, of greater depth than the shape covers.
fn group_pixels(scene: &Scene, masks: &[Vec<u8>], group: &[usize], len: usize) -> (Vec<u8>, Vec<u8>) {
    let (mut union, mut seen) = (vec![0u8; len], vec![0u8; len]);
    for &member in group {
        let depth = scene.shapes[member].depth;
        let front: Vec<&Vec<u8>> = (0..scene.shapes.len())
            .filter(|other| !group.contains(other) && scene.shapes[*other].depth > depth)
            .map(|other| &masks[other])
            .collect();
        for at in (0..len).filter(|&at| masks[member][at] != 0) {
            union[at] = 1;
            if front.iter().all(|mask| mask[at] == 0) {
                seen[at] = 1;
            }
        }
    }
    (union, seen)
}

/// How many pixels of a mask are set.
fn count(mask: &[u8]) -> usize {
    mask.iter().filter(|&&set| set != 0).count()
}

/// The request of `visible_json`: a scene on a crop of width x height.
#[derive(Deserialize)]
struct VisibleRequest {
    /// The crop's shapes, targets and occluders.
    scene: Scene,
    /// The crop's width, in pixels.
    width: usize,
    /// The crop's height, in pixels.
    height: usize,
}

/// `visible` over JSON, for the page (src/wasm.rs): {scene, width, height} in, a `SceneView` (or {error}) out.
pub fn visible_json(request: &[u8]) -> Vec<u8> {
    let answer = serde_json::from_slice::<VisibleRequest>(request)
        .map_err(|error| format!("The scene could not be read: {error}"))
        .and_then(|request| check(&request.scene).map(|()| request))
        .map(|request| visible(&request.scene, request.width, request.height));
    match answer {
        Ok(view) => serde_json::to_vec(&view),
        Err(message) => serde_json::to_vec(&serde_json::json!({ "error": message })),
    }
    .unwrap_or_default()
}

/// Checks the shapes' outlines, what hides what, the scene's checks and the run lengths.
#[cfg(test)]
mod tests {
    use super::*;

    /// The test crops' side, in pixels.
    const SIDE: usize = 128;

    /// A flat, unturned shape with this id, kind, frame and depth, and no role or model box.
    fn shape(id: &str, kind: ShapeKind, frame: [f64; 4], depth: i32) -> Shape {
        Shape {
            id: id.into(),
            kind,
            frame,
            angle: 0.0,
            face: None,
            solid: None,
            points: None,
            depth,
            role: None,
            model: None,
        }
    }

    /// Whether the pixel (x, y) of a SIDE x SIDE crop is set in these run lengths.
    fn pixel(view: &[u32], x: usize, y: usize) -> bool {
        from_runs(view, SIDE * SIDE)[y * SIDE + x] != 0
    }

    /// A head and a body joined are one target, with one box round both.
    #[test]
    fn joined_shapes_make_one_target_with_one_box() {
        let head = Shape { role: Some(ShapeRole::Head), ..shape("head", ShapeKind::Pill, [50.0, 40.0, 10.0, 10.0], 0) };
        let body = Shape { role: Some(ShapeRole::Body), ..shape("body", ShapeKind::Pill, [50.0, 52.0, 10.0, 14.0], 0) };
        let joined = vec![vec!["head".into(), "body".into()]];
        let scene = Scene { shapes: vec![head, body], targets: joined, ..Scene::default() };
        let view = visible(&scene, SIDE, SIDE);
        assert_eq!(view.targets.len(), 1);
        let target = &view.targets[0];
        assert_eq!(target.shapes, ["head", "body"]);
        assert!(!target.hidden);
        let [cx, cy, width, height] = target.frame.unwrap();
        assert!((cx - 50.0).abs() < 1e-9 && (cy - 47.0).abs() < 1e-9, "{cx} {cy}");
        assert!((width - 10.0).abs() < 1e-9 && (height - 24.0).abs() < 1e-9, "{width} {height}");
        assert_eq!(view.mask, target.runs);
    }

    /// A pill turned upright, a round pill, a cube with a third face and a turned box hold the points inside their
    /// outlines and no others.
    #[test]
    fn a_turned_pill_and_a_box_with_a_face_cover_their_outlines() {
        let upright = Shape { angle: 90.0, ..shape("p", ShapeKind::Pill, [64.0, 64.0, 40.0, 10.0], 0) };
        assert!(contains(&upright, 64.0, 82.0) && !contains(&upright, 82.0, 64.0));
        let flat = shape("p", ShapeKind::Pill, [64.0, 64.0, 40.0, 10.0], 0);
        assert!(contains(&flat, 82.0, 64.0) && !contains(&flat, 64.0, 82.0));
        let round = shape("p", ShapeKind::Pill, [64.0, 64.0, 20.0, 20.0], 0);
        assert!(contains(&round, 64.0 + 9.9, 64.0) && !contains(&round, 64.0 + 7.5, 64.0 + 7.5));
        let square = shape("b", ShapeKind::Box, [50.0, 50.0, 20.0, 20.0], 0);
        let cube = Shape { face: Some([6.0, -6.0]), ..square.clone() };
        assert!(contains(&cube, 64.0, 38.0) && !contains(&square, 64.0, 38.0), "the far face");
        assert!(!contains(&cube, 38.0, 64.0), "the corner the hull leaves out");
        assert_eq!(outline(&cube).len(), 6, "a cube seen at an angle is a hexagon");
        let turned = Shape { angle: 45.0, ..square };
        assert!(contains(&turned, 50.0, 50.0 - 13.0) && !contains(&turned, 41.0, 41.0));
    }

    /// A pill with a third face holds the band it sweeps to its far end, round ends included, and its bounds span it.
    #[test]
    fn a_pill_with_a_third_face_covers_the_band_to_its_far_end() {
        let pill = shape("p", ShapeKind::Pill, [40.0, 64.0, 10.0, 30.0], 0);
        let deep = Shape { face: Some([20.0, -10.0]), ..pill.clone() };
        assert!(contains(&deep, 50.0, 59.0) && !contains(&pill, 50.0, 59.0), "the band between the ends");
        assert!(contains(&deep, 60.0, 54.0 - 14.0), "the far end's round top");
        assert!(!contains(&deep, 60.0, 54.0 - 16.0) && !contains(&deep, 40.0, 64.0 + 16.0), "past either end");
        assert!(!contains(&deep, 36.0, 44.0), "the corner the sweep leaves out");
        let ball = Shape { face: Some([10.0, 0.0]), ..shape("s", ShapeKind::Pill, [40.0, 40.0, 10.0, 10.0], 0) };
        assert!(contains(&ball, 54.9, 40.0) && !contains(&ball, 40.0, 45.5), "a sphere swept is a pill");
        let [x0, y0, x1, y1] = bounds(&deep);
        assert!((x0 - 35.0).abs() < 1e-9 && (x1 - 65.0).abs() < 1e-9, "{x0} {x1}");
        assert!((y0 - 39.0).abs() < 1e-9 && (y1 - 79.0).abs() < 1e-9, "{y0} {y1}");
    }

    /// An oval with equal sides covers a round pill's pixels, turned or not, and has its box.
    #[test]
    fn a_round_oval_covers_a_round_pills_pixels() {
        for angle in [0.0, 30.0] {
            let pill = Shape { angle, ..shape("p", ShapeKind::Pill, [64.0, 64.0, 20.0, 20.0], 0) };
            let oval = Shape { kind: ShapeKind::Ellipse, ..pill.clone() };
            let (round, stretched) = (
                visible(&Scene { shapes: vec![pill], ..Scene::default() }, SIDE, SIDE),
                visible(&Scene { shapes: vec![oval], ..Scene::default() }, SIDE, SIDE),
            );
            assert_eq!(round.mask, stretched.mask, "turned {angle} degrees");
        }
        let oval = shape("e", ShapeKind::Ellipse, [64.0, 64.0, 20.0, 20.0], 0);
        let [x0, y0, x1, y1] = bounds(&oval);
        assert!([x0 - 54.0, y0 - 54.0, x1 - 74.0, y1 - 74.0].iter().all(|gap| gap.abs() < 1e-9), "{x0} {y0} {x1} {y1}");
    }

    /// An oval 40 wide and 10 high, turned 90 degrees, covers the pixels of one 10 wide and 40 high: its tips, not the
    /// corners of its frame; turned 30 degrees its tips turn with it and its box stays within a quarter pixel.
    #[test]
    fn a_turned_oval_covers_its_ellipse() {
        let flat = shape("e", ShapeKind::Ellipse, [64.5, 64.5, 40.0, 10.0], 0);
        let upright = Shape { angle: 90.0, ..flat.clone() };
        let tall = shape("e", ShapeKind::Ellipse, [64.5, 64.5, 10.0, 40.0], 0);
        let view = |one: &Shape| visible(&Scene { shapes: vec![one.clone()], ..Scene::default() }, SIDE, SIDE).mask;
        assert_eq!(view(&upright), view(&tall));
        let seen = view(&upright);
        assert!(pixel(&seen, 64, 84) && pixel(&seen, 64, 45) && pixel(&seen, 69, 64), "its tips and sides");
        assert!(!pixel(&seen, 64, 85) && !pixel(&seen, 70, 64) && !pixel(&seen, 84, 64), "past them");
        assert!(!pixel(&seen, 68, 80) && !pixel(&seen, 61, 49), "the corners of its frame");
        assert!(contains(&flat, 64.5 + 19.9, 64.5) && !contains(&flat, 64.5 + 20.1, 64.5), "the tip along its width");
        assert!(!contains(&flat, 64.5 + 19.0, 64.5 + 4.0), "the corner of its frame");
        let turned = Shape { angle: 30.0, ..flat };
        let (sin, cos) = 30.0_f64.to_radians().sin_cos();
        assert!(contains(&turned, 64.5 + 19.9 * cos, 64.5 + 19.9 * sin), "its tip, turned clockwise");
        assert!(!contains(&turned, 64.5 + 20.1 * cos, 64.5 + 20.1 * sin) && !contains(&turned, 64.5 + 19.9, 64.5));
        let [x0, y0, x1, y1] = bounds(&turned);
        let (half_w, half_h) =
            ((400.0 * cos * cos + 25.0 * sin * sin).sqrt(), (400.0 * sin * sin + 25.0 * cos * cos).sqrt());
        for (side, exact) in [(x1 - x0, 2.0 * half_w), (y1 - y0, 2.0 * half_h)] {
            assert!(side <= exact + 1e-9 && exact - side < 0.5, "{side} against {exact}");
        }
    }

    /// An oval with a third face covers the band it sweeps to its far end, and its bounds span it; a solid oval or one
    /// with its points placed is refused.
    #[test]
    fn an_oval_with_a_third_face_covers_the_band_to_its_far_end() {
        let oval = shape("e", ShapeKind::Ellipse, [40.0, 64.0, 10.0, 30.0], 0);
        let deep = Shape { face: Some([20.0, -10.0]), ..oval.clone() };
        assert!(contains(&deep, 50.0, 59.0) && !contains(&oval, 50.0, 59.0), "the band between the ends");
        assert!(contains(&deep, 60.0, 54.0 - 14.9) && !contains(&deep, 60.0, 54.0 - 15.1), "the far end's top");
        assert!(!contains(&deep, 40.0, 64.0 + 15.1) && !contains(&deep, 66.0, 54.0), "past either end");
        assert!(!contains(&deep, 36.0, 44.0) && !contains(&deep, 64.0, 74.0), "the corners the sweep leaves out");
        let [x0, y0, x1, y1] = bounds(&deep);
        assert!((x0 - 35.0).abs() < 1e-9 && (x1 - 65.0).abs() < 1e-9, "{x0} {x1}");
        assert!((y0 - 39.0).abs() < 1e-9 && (y1 - 79.0).abs() < 1e-9, "{y0} {y1}");
        let solid = Shape { solid: Some(Solid { thickness: 5.0, tip: 0.0, swing: 0.0 }), ..oval.clone() };
        let placed = Shape { points: Some(vec![[35.0, 49.0], [45.0, 49.0], [45.0, 79.0]]), ..oval };
        for refused in [solid, placed] {
            let scene = Scene { shapes: vec![refused], ..Scene::default() };
            assert!(check(&scene).unwrap_err().contains("oval"));
        }
    }

    /// A solid box tipped or swung shows its thickness, a capsule end on is a disc, and a solid of no thickness is
    /// refused.
    #[test]
    fn a_solid_shows_the_camera_what_its_turn_puts_in_front() {
        let solid = |tip: f64, swing: f64| Some(Solid { thickness: 30.0, tip, swing });
        let flat = shape("b", ShapeKind::Box, [64.0, 64.0, 20.0, 10.0], 0);
        let facing = Shape { solid: solid(0.0, 0.0), ..flat.clone() };
        for (x, y) in [(73.9, 68.9), (75.0, 64.0), (64.0, 70.0)] {
            assert_eq!(contains(&facing, x, y), contains(&flat, x, y), "facing the camera, a box is its rectangle");
        }
        let tipped = Shape { solid: solid(90.0, 0.0), ..flat.clone() };
        assert!(
            contains(&tipped, 64.0, 64.0 + 14.9) && !contains(&tipped, 64.0, 64.0 + 15.1),
            "its top: the thickness"
        );
        let swung = Shape { solid: solid(0.0, 90.0), ..flat.clone() };
        assert!(contains(&swung, 64.0 + 14.9, 64.0) && !contains(&swung, 64.0 + 15.1, 64.0), "its side: the thickness");
        let [x0, y0, x1, y1] = bounds(&Shape { solid: solid(30.0, 30.0), ..flat });
        assert!(x1 - x0 > 20.0 && y1 - y0 > 10.0, "a cube seen from above and the right shows more than its front");
        let pill = shape("p", ShapeKind::Pill, [64.0, 64.0, 10.0, 40.0], 0);
        let end_on = Shape { solid: solid(90.0, 0.0), ..pill.clone() };
        assert!(
            contains(&end_on, 64.0 + 4.9, 64.0) && !contains(&end_on, 64.0, 64.0 + 6.0),
            "a capsule end on: a disc"
        );
        let leaning = Shape { solid: solid(60.0, 0.0), ..pill };
        assert!(
            contains(&leaning, 64.0, 64.0 + 12.4) && !contains(&leaning, 64.0, 64.0 + 13.0),
            "foreshortened by half"
        );
        assert!(
            check(&Scene {
                shapes: vec![Shape {
                    solid: Some(Solid { thickness: 0.0, tip: 0.0, swing: 0.0 }),
                    ..shape("z", ShapeKind::Box, [1.0, 1.0, 1.0, 1.0], 0)
                }],
                ..Scene::default()
            })
            .is_err()
        );
    }

    /// A box's vertices placed by hand decide its outline: 4 moved corners, or a cube's 8 spanning a hexagon.
    #[test]
    fn a_box_with_its_vertices_placed_covers_what_they_span() {
        let flat = shape("b", ShapeKind::Box, [64.0, 64.0, 20.0, 20.0], 0);
        let leaning =
            Shape { points: Some(vec![[54.0, 54.0], [74.0, 58.0], [74.0, 70.0], [54.0, 74.0]]), ..flat.clone() };
        assert!(contains(&leaning, 73.0, 60.0) && !contains(&leaning, 73.0, 56.0), "the corner moved down on its own");
        assert!(contains(&flat, 73.0, 56.0), "the box it came from covered it");
        let cube: Vec<[f64; 2]> = (0..8_usize)
            .map(|i| {
                [
                    54.0 + 20.0 * (i & 1) as f64 + 6.0 * (i >> 2) as f64,
                    54.0 + 20.0 * (i >> 1 & 1) as f64 - 6.0 * (i >> 2) as f64,
                ]
            })
            .collect();
        let placed = Shape { points: Some(cube), ..flat };
        assert_eq!(outline(&placed).len(), 6, "eight vertices of a cube seen at an angle span a hexagon");
        assert!(contains(&placed, 78.0, 50.0) && !contains(&placed, 55.0, 50.0));
    }

    /// An occluder in front takes its pixels from a target and is no target itself; a nearer target hides a farther
    /// one and keeps its own box.
    #[test]
    fn what_is_in_front_hides_what_is_behind() {
        let target = shape("t", ShapeKind::Pill, [50.0, 50.0, 20.0, 20.0], 0);
        let pillar = shape("o", ShapeKind::Box, [50.0, 50.0, 6.0, 40.0], 1);
        let scene = Scene { shapes: vec![target.clone(), pillar], occluders: vec!["o".into()], ..Scene::default() };
        let view = visible(&scene, SIDE, SIDE);
        assert_eq!(view.targets.len(), 1, "an occluder is no target");
        let seen = &view.targets[0].runs;
        assert!(!pixel(seen, 50, 50) && pixel(seen, 42, 50) && pixel(seen, 58, 50));
        assert_eq!(view.targets[0].frame, Some([50.0, 50.0, 21.0, 19.0]), "the pillar takes the top and bottom too");

        let near = shape("n", ShapeKind::Pill, [58.0, 50.0, 20.0, 20.0], 1);
        let view = visible(&Scene { shapes: vec![target, near], ..Scene::default() }, SIDE, SIDE);
        let (far, front) = (&view.targets[0], &view.targets[1]);
        assert!(!pixel(&far.runs, 56, 50) && pixel(&far.runs, 42, 50), "the far target loses what the near one covers");
        assert!(pixel(&front.runs, 56, 50));
        assert_eq!(front.frame, Some([58.0, 50.0, 20.0, 20.0]), "nothing hides the near one: its own box");
    }

    /// A target an occluder covers whole is marked hidden, has no visible box, and keeps its shapes' box.
    #[test]
    fn a_target_hidden_entirely_is_flagged() {
        let small = shape("t", ShapeKind::Pill, [50.0, 50.0, 6.0, 6.0], 0);
        let wall = shape("o", ShapeKind::Box, [50.0, 50.0, 30.0, 30.0], 2);
        let scene = Scene { shapes: vec![small, wall], occluders: vec!["o".into()], ..Scene::default() };
        let view = visible(&scene, SIDE, SIDE);
        assert!(view.targets[0].hidden && view.targets[0].frame.is_none());
        assert_eq!(view.targets[0].whole, [50.0, 50.0, 6.0, 6.0], "its shapes' box, for an ignore box");
        assert!(from_runs(&view.mask, SIDE * SIDE).iter().all(|&set| set == 0));
    }

    /// `check` refuses an id used twice, a shape both joined and hiding, an unknown id, and a shape of no size.
    #[test]
    fn a_scene_with_a_slip_is_refused() {
        let one = shape("a", ShapeKind::Pill, [10.0, 10.0, 4.0, 4.0], 0);
        let twice = Scene { shapes: vec![one.clone(), one.clone()], ..Scene::default() };
        assert!(check(&twice).unwrap_err().contains("twice"));
        let both = Scene { shapes: vec![one.clone()], targets: vec![vec!["a".into()]], occluders: vec!["a".into()] };
        assert!(check(&both).is_err());
        let missing = Scene { shapes: vec![one.clone()], targets: vec![vec!["b".into()]], ..Scene::default() };
        assert!(check(&missing).unwrap_err().contains("no shape"));
        let flat = Scene { shapes: vec![Shape { frame: [10.0, 10.0, 0.0, 4.0], ..one }], ..Scene::default() };
        assert!(check(&flat).unwrap_err().contains("no size"));
    }

    /// `runs` starts with the unset count (0 when the first pixel is set), and `from_runs` gives the mask back.
    #[test]
    fn run_lengths_give_the_mask_back() {
        let mask = [1u8, 1, 0, 0, 0, 1, 0, 1, 1, 1];
        assert_eq!(runs(&mask), [0, 2, 3, 1, 1, 3]);
        assert_eq!(from_runs(&runs(&mask), mask.len()), mask);
        assert_eq!(runs(&[0, 0]), [2]);
    }

    /// `visible_json` gives the view for a scene it can read, and an error for one it cannot.
    #[test]
    fn the_page_gets_the_view_or_why_not() {
        let shapes = r#"[{"id": "a", "kind": "pill", "box": [8, 8, 6, 6]}]"#;
        let request = format!(r#"{{"scene": {{"shapes": {shapes}}}, "width": 16, "height": 16}}"#).into_bytes();
        let view: serde_json::Value = serde_json::from_slice(&visible_json(&request)).unwrap();
        assert_eq!(view["targets"][0]["box"], serde_json::json!([8.0, 8.0, 6.0, 6.0]));
        let refused: serde_json::Value = serde_json::from_slice(&visible_json(b"{}")).unwrap();
        assert!(refused["error"].as_str().unwrap().contains("could not be read"));
    }
}
