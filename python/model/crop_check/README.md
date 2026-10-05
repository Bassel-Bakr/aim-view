# Crop check

A phone page for checking the detector's labels, one crop at a time. You tap Right, Wrong or Unsure. When a crop is
wrong, you fix its boxes on the crop. Claude publishes the page as a private Artifact, and the Artifact's database keeps
your answers as you go.

## Files

- `index.html`: the page. The crop fills the width and the verdicts sit at the bottom, for one thumb. Zoom with two
  fingers, move and resize a box, draw a new one, tap a point for a target too small to draw, or tap a box to cross it
  out. A dashed guide shows each box's edge. After 3 or more fixes on a recording agree (sizes within 35%), the page
  offers the same fix on its next crops; an answer taken as offered does not teach it again. A target the model sees
  in pieces (a robot) has a lesson of its own: after 3 fixes on a recording that crossed out every piece and drew one
  box over them, the page offers one box per group of pieces, with the size and the place over the pieces of the
  middle of those boxes. One tab per set.
- `make_page.py`: adds a set of crops to a page folder (a tab), and copies the page there.
- `labels.py`: turns the answers into labels that `checked_data.py` reads.

## The format

`crops.json` is a list with one entry per crop:

- `id`: the set and the file, with dots (`mined.val.1268f463c2_029_1`); the crop's PNG is `crops/<id>.png`.
- `set`, `file` (relative to the crops' source), `folder` (the recording), `kind`, `why` (a list), `rule`.
- `boxes`: `[cx, cy, w, h]` in the 256 x 256 crop's pixels; `scores`: the model's, when known.
- `preset` (optional): `{"remove": [box indexes]}`, boxes that start crossed out, so Right agrees they are no target.

`sets.json` gives each set its tab `title`, its `crossedOut` note (shown over a crop with a crossed-out box), and
`learn: false` for a set whose answers should not teach the page's suggestions (a second pass, such as Tighten).

An answer is a document in the `checks` collection, named by the crop's id:

- `verdict`: `correct`, `wrong` or `unsure`.
- `remove`: the indexes of the boxes crossed out; `edit`: `{index: [cx, cy, w, h]}` for boxes moved or resized.
- `add`: new boxes `[cx, cy, w, h]` and tapped points `[x, y]`.
- `suggested`: true when the page's suggestion was taken as offered; `set`, `file`, `at` (milliseconds).

A label line (`labels.py`): `file`, `boxes`, `verdict` (`correct`, `skip` with no box left, or `unsure`), `auto` and
`model` (the crop's boxes before the check), `source` (the set, and `:rule` when the crop has one), and `phone` (the
answer).

## A round, step by step

1. Pick the crops: `pick_checks.py` writes a `picks.jsonl`; `build_mined.py` writes a folder of crop files.
2. Make the page, once per set:
   `python python/model/crop_check/make_page.py <page folder> <set> <picks.jsonl or folder> --title "..."`.
   Keep the page folder under `test_out/vod_model/`, beside the crops.
3. Claude publishes `index.html` as an Artifact with the `db` capability (collection `checks`) and the files that
   `publish_<set>_<n>.json` lists. A publish takes at most 255 files and a version 511, so leave an older set's PNGs
   out when a page grows past that.
4. You check the crops on your phone.
5. Claude saves the answers with the ArtifactData tool: `list` on the page's url, collection `checks`, with `out_dir`
   `<page folder>/answers` (one `answers/checks/<id>.json` each).
6. `python python/model/crop_check/labels.py <page folder> <page folder>/answers/checks <out jsonl> <set> ...`
   (`--prefer <set>` lets a second pass's answers win; `--point-size crop` sizes a tapped point by the crop's own
   boxes instead of the recording's).
7. Keep the out jsonl beside the crops: it is your labelled data (AGENTS.md, "Data the user labelled").
   `checked_data.py` builds the training set from it.

Checked on the 2026-10-04 round: `make_page.py` rebuilds its Mined and Other themes sets entry for entry and PNG for
PNG, and `labels.py` gives `data_moving_themes/checked_phone.jsonl`'s 150 lines from that set (with `--prefer tight`)
and `data_mined/checked_phone.jsonl`'s 110 lines (with `--point-size crop`) exactly.
