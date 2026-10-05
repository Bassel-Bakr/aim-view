//! The shapes a target is drawn with on a crop: KovaaK's two, as their outline on screen (the targets are 3D). A pill
//! (a sphere is a pill with equal sides) and a box (a square or a cube; a cube seen at an angle gets a third face, so
//! its outline is a hexagon), each turned to any angle. Shapes are joined into targets (a bot's head and body), ordered
//! front to back by depth (a shape hides the parts of shapes behind it), and some only hide what is behind them
//! (occluders: the crosshair, a pillar, an overlay).
//!
//! In: a scene, as the Crops page saves it (service/src/library/crops.rs). Out: each target's visible pixels and box
//! (`visible`), for the page (src/wasm.rs `shapes_visible`) and the training set (aimview-tool crop-labels, read by
//! python/model/checked_data.py). Coordinates are crop pixels, pixel i at coordinate i (as checked_data.py's
//! `ellipse_mask` reads a box); an angle is in degrees, clockwise on screen.

use serde::{Deserialize, Serialize};

/// KovaaK's target shapes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum ShapeKind {
    Pill,
    Box,
}

/// The part of a bot a shape stands for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum ShapeRole {
    Head,
    Body,
}

/// One shape: its kind, its frame before turning ([center x, center y, width, height], pixels), its angle (degrees,
/// clockwise), a box's third face (the offset of the far face, pixels), its depth (greater is nearer), its role, and
/// the model box it started from (an index into the crop's boxes), if any.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Shape {
    pub id: String,
    pub kind: ShapeKind,
    #[serde(rename = "box")]
    #[cfg_attr(feature = "ts", ts(rename = "box", as = "crate::typescript::CropBox"))]
    pub frame: [f64; 4],
    #[serde(default)]
    pub angle: f64,
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(as = "Option<crate::typescript::FaceOffset>"))]
    pub face: Option<[f64; 2]>,
    #[serde(default)]
    pub depth: i32,
    #[serde(default)]
    pub role: Option<ShapeRole>,
    #[serde(default)]
    pub model: Option<usize>,
}

/// A crop's shapes: which are joined into one target (a shape in no group, and no occluder, is a target of its own),
/// and which only hide what is behind them.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Scene {
    pub shapes: Vec<Shape>,
    #[serde(default)]
    pub targets: Vec<Vec<String>>,
    #[serde(default)]
    pub occluders: Vec<String>,
}

/// One target as the scene shows it: its shapes, the box round its visible pixels ([center x, center y, width,
/// height]; None when none shows), the box round all its shapes (hidden parts too), whether something hides all of
/// it, and its visible pixels as run lengths (`runs`).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct TargetView {
    pub shapes: Vec<String>,
    #[serde(rename = "box")]
    #[cfg_attr(feature = "ts", ts(rename = "box", as = "Option<crate::typescript::CropBox>"))]
    pub frame: Option<[f64; 4]>,
    #[cfg_attr(feature = "ts", ts(as = "crate::typescript::CropBox"))]
    pub whole: [f64; 4],
    pub hidden: bool,
    pub runs: Vec<u32>,
}

/// A scene seen on a crop: its targets, and the pixels of all of them (the training `tmask`) as run lengths.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct SceneView {
    pub targets: Vec<TargetView>,
    pub mask: Vec<u32>,
}

/// A pill's ends and a box's outline are drawn with this many points per half circle.
const ARC_POINTS: usize = 16;
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

/// The convex hull of points, counterclockwise in maths' terms (Andrew's monotone chain).
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

/// A pill's outline: two half circles joined, ARC_POINTS a half circle.
fn pill_outline(shape: &Shape) -> Vec<[f64; 2]> {
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

/// A shape's outline on the crop, in order round it.
pub fn outline(shape: &Shape) -> Vec<[f64; 2]> {
    match shape.kind {
        ShapeKind::Pill => pill_outline(shape),
        ShapeKind::Box => box_outline(shape),
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

/// Whether a crop point is inside a shape: a pill holds the points within its radius of its middle segment; a box
/// those in its turned rectangle, or in its outline when it has a third face.
pub fn contains(shape: &Shape, x: f64, y: f64) -> bool {
    match (shape.kind, shape.face) {
        (ShapeKind::Pill, _) => {
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
    }
}

/// The box round a shape's outline: [x0, y0, x1, y1].
fn bounds(shape: &Shape) -> [f64; 4] {
    outline(shape).iter().fold([f64::MAX, f64::MAX, f64::MIN, f64::MIN], |b, point| {
        [b[0].min(point[0]), b[1].min(point[1]), b[2].max(point[0]), b[3].max(point[1])]
    })
}

/// Why a scene cannot be saved, or Ok: every id once, sizes positive and finite, every joined or hiding id a shape's,
/// no shape in two targets, and no occluder in a target.
pub fn check(scene: &Scene) -> Result<(), String> {
    let mut ids = std::collections::HashSet::new();
    for shape in &scene.shapes {
        if !ids.insert(shape.id.as_str()) {
            return Err(format!("the shape id {} is used twice", shape.id));
        }
        let numbers = shape.frame.iter().chain([&shape.angle]).chain(shape.face.iter().flatten());
        if numbers.into_iter().any(|value| !value.is_finite() || value.abs() > MAX_VALUE) {
            return Err(format!("the shape {} has a number out of range", shape.id));
        }
        if shape.frame[2] <= 0.0 || shape.frame[3] <= 0.0 {
            return Err(format!("the shape {} has no size", shape.id));
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
    let [x0, y0, x1, y1] = shapes
        .iter()
        .map(|shape| bounds(shape))
        .fold([f64::MAX, f64::MAX, f64::MIN, f64::MIN], |b, one| {
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
    scene: Scene,
    width: usize,
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

#[cfg(test)]
mod tests {
    use super::*;

    const SIDE: usize = 128;

    fn shape(id: &str, kind: ShapeKind, frame: [f64; 4], depth: i32) -> Shape {
        Shape { id: id.into(), kind, frame, angle: 0.0, face: None, depth, role: None, model: None }
    }

    fn pixel(view: &[u32], x: usize, y: usize) -> bool {
        from_runs(view, SIDE * SIDE)[y * SIDE + x] != 0
    }

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

    #[test]
    fn run_lengths_give_the_mask_back() {
        let mask = [1u8, 1, 0, 0, 0, 1, 0, 1, 1, 1];
        assert_eq!(runs(&mask), [0, 2, 3, 1, 1, 3]);
        assert_eq!(from_runs(&runs(&mask), mask.len()), mask);
        assert_eq!(runs(&[0, 0]), [2]);
    }

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
