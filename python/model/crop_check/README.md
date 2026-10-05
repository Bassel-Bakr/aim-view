# Crop check

Checking the detector's labels on the phone, one crop at a time. You tap Right, Wrong or Can't tell. When a crop is
wrong, you fix it on the crop. The app's Crops page does this (the Crops button in the top bar, or `?page=crops`): it
reads the check folders in `test_out/vod_model/check_*` and saves each answer in the folder's `answers/checks/`. Run
`bun run server:dev` and open `http://<this computer's address>:8770/?page=crops` on the phone. Browser mode has the
page too: Add folder copies a check folder into the browser, Export saves its answers as one file, and Import on the
review server takes that file (a crop's newer answer wins).

The Crops page labels with shapes, as KovaaK's targets are drawn: a pill (a sphere is a round pill) and a box (a cube
seen at an angle gets its third face), each turned to any angle. Shapes join into one target (a head and a body, each
with its role), sit in front of or behind each other, and an occluder hides what is behind it without being a target.
The core works out what each target shows (src/shapes.rs), the page tints those pixels, and `labels.py` takes its
boxes and pixels from the core too, so the page and the training data agree.

Before the app had the page, Claude published `index.html` as a private Artifact, and the Artifact's database kept the
answers. Those answers still load on the Crops page and still turn into labels.

## Files

- `index.html`: the claude.ai page. The crop fills the width and the verdicts sit at the bottom, for one thumb. Zoom with two
  fingers, move and resize a box, draw a new one, tap a point for a target too small to draw, or tap a box to cross it
  out. A dashed guide shows each box's edge. After 3 or more fixes on a recording agree (sizes within 35%), the page
  offers the same fix on its next crops; an answer taken as offered does not teach it again. A target the model sees
  in pieces (a robot) has a lesson of its own: after 3 fixes on a recording that crossed out every piece and drew one
  box over them, the page offers one box per group of pieces, with the size and the place over the pieces of the
  middle of those boxes. One tab per set.
- `make_page.py`: adds a set of crops to a page folder (a tab), and copies the page there.
- `labels.py`: turns the answers into labels that `checked_data.py` reads.
- The Crops page: `ui/src/app/crops/`; its routes: `service/src/crops.rs`.

## The format

`crops.json` is a list with one entry per crop:

- `id`: the set and the file, with dots (`mined.val.1268f463c2_029_1`); the crop's PNG is `crops/<id>.png`.
- `set`, `file` (relative to the crops' source), `folder` (the recording), `kind`, `why` (a list), `rule`.
- `boxes`: `[cx, cy, w, h]` in the 256 x 256 crop's pixels; `scores`: the model's, when known.
- `preset` (optional): `{"remove": [box indexes]}`, boxes that start crossed out, so Right agrees they are no target.

`sets.json` gives each set its tab `title`, its `crossedOut` note (shown over a crop with a crossed-out box), and
`learn: false` for a set whose answers should not teach the page's suggestions (a second pass, such as Tighten).

An answer is a file `answers/checks/<crop id>.json` (a document in the `checks` collection on the claude.ai page):

- `verdict`: `right`, `wrong` or `unsure`.
- `remove`: the indexes of the boxes crossed out; `edit`: `{index: [cx, cy, w, h]}` for boxes moved or resized.
- `add`: new boxes `[cx, cy, w, h]` and tapped points `[x, y]`.
- `suggested`: true when the page's suggestion was taken as offered; `set`, `file`, `at` (milliseconds).
- `scene` (the Crops page): `shapes` (each with `id`, `kind` pill or box, `box` `[cx, cy, w, h]` before turning, `angle`
  in degrees clockwise, a box's `face` `[dx, dy]`, `depth` (greater is nearer), `role` head, body or null, and `model`,
  the index of the model's box it began as), `targets` (lists of joined shape ids; a shape in none is a target of its
  own) and `occluders`. The Crops page writes the fields above from it too, so its suggestions keep learning.

A label line (`labels.py`): `file`, `boxes`, `verdict` (`correct`, `skip` with no box left, or `unsure`), `auto` and
`model` (the crop's boxes before the check), `source` (the set, and `:rule` when the crop has one), and `phone` (the
answer). An answer with a scene has its labels from the core (`aimview-tool crop-labels <page>`): `boxes` (one per
target that shows), `covered` (the box round each target hidden entirely: an ignore box in training) and `mask` (the
targets' visible pixels as run lengths over the crop's rows, the first of pixels not set: `checked_data.py` makes it
the crop's `tmask`).

## A round, step by step

1. Pick the crops: `pick_checks.py` writes a `picks.jsonl`; `build_mined.py` writes a folder of crop files.
2. Make the page, once per set:
   `python python/model/crop_check/make_page.py <page folder> <set> <picks.jsonl or folder> --title "..."`.
   Keep the page folder under `test_out/vod_model/`, beside the crops.
3. You check the crops on the Crops page, on your phone. Its answers land in `<page folder>/answers/checks/`;
   an answer given again moves the one it replaces to `answers/replaced/`.
   (On the claude.ai page instead: Claude publishes `index.html` as an Artifact with the `db` capability, collection
   `checks`, and the files `publish_<set>_<n>.json` lists, then saves the answers with the ArtifactData tool, `list`
   with `out_dir` `<page folder>/answers`.)
4. `python python/model/crop_check/labels.py <page folder> <page folder>/answers/checks <out jsonl> <set> ...`
   (`--also <folder>` adds more answers folders, a crop's newest answer winning, as the Crops page shows it;
   `--prefer <set>` lets a second pass's answers win; `--point-size crop` sizes a tapped point by the crop's own
   boxes instead of the recording's).
5. Keep the out jsonl beside the crops: it is your labelled data (AGENTS.md, "Data the user labelled").
   `checked_data.py` builds the training set from it.

Checked on the 2026-10-04 round: `make_page.py` rebuilds its Mined and Other themes sets entry for entry and PNG for
PNG, and `labels.py` gives `data_moving_themes/checked_phone.jsonl`'s 150 lines from that set (with `--prefer tight`)
and `data_mined/checked_phone.jsonl`'s 110 lines (with `--point-size crop`) exactly. With scenes (2026-10-05):
`labels.py` still gives `data_bars/checked_phone.jsonl` (354 lines) and `hand_robots/checked_phone.jsonl` (312) exactly,
the rows' `phone` gaining an empty `scene`; a crop fixed on the Crops page (a sphere and a drawn pill joined as head
and body, a box occluder) became one box over both and a `tmask` of exactly their pixels.
