"""Adds a set of crops to a crop-check page folder (README.md): each crop as a PNG under crops/, its entry in
crops.json, its tab in sets.json, and the page itself (index.html, copied from beside this script).

Usage: python python/model/crop_check/make_page.py <page folder> <set> <source> [--title TITLE] [--crossed-out NOTE]
       [--no-learn]

<source> is a picks.jsonl from pick_checks.py (its "file" paths relative to the jsonl's folder), or a folder of crop
files (.npz, searched below it; a manifest.jsonl there names each crop's recording and kind by its "stem", the start
of the file's name). A crop file holds rgb (the 256 x 256 crop) and boxes ([cx, cy, w, h] in crop pixels), and may hold
scores, why, mined (the rule that mined it) and fix: for a rule whose name starts with "false", the box the rule thinks
is no target, added after the others and shown crossed out, so Right agrees. A picks line may add folder, kind, why and
rule, and boxes, scores and preset to show instead of the crop file's (second_look.py's). Running it again for a set
replaces that set.

--title names the set's tab; --crossed-out is the note shown over a crop with a crossed-out box; --no-learn keeps the
set's answers out of the page's suggestions (a second pass over crops already checked, such as Tighten).

Prints how to publish it: the Artifact tool takes at most 255 files a publish and 511 a version.
"""
import argparse
import json
import re
import shutil
from pathlib import Path

import numpy as np
from PIL import Image

HERE = Path(__file__).parent
# the Artifact tool's limits: files in one publish, and files in one version of a page
PUBLISH_LIMIT = 255
VERSION_LIMIT = 511


def rounded(boxes):
    """The boxes as lists of plain floats rounded to 0.1 px, for JSON."""
    return [[round(float(value), 1) for value in box] for box in boxes]


def page_id(name):
    """A crop's id with every character the page's database refuses in a document id (an apostrophe, a space) made
    "_": "Cartoon's Micro" could not be saved."""
    return re.sub(r"[^A-Za-z0-9_\-.~:@+]", "_", name)


def entry(set_name, file, crop, folder="", kind="", why=(), rule=None):
    """A crop's entry in crops.json; `file` is its path relative to the source."""
    boxes = rounded(crop["boxes"])
    row = {
        "id": page_id(f"{set_name}.{file.replace('/', '.').removesuffix('.npz')}"), "set": set_name, "file": file,
        "folder": folder, "kind": kind, "why": list(why), "rule": rule, "boxes": boxes,
        "scores": [round(float(score), 2) for score in crop["scores"]] if "scores" in crop.files else [],
    }
    if rule and rule.startswith("false") and "fix" in crop.files:
        row["boxes"] = boxes + rounded([crop["fix"]])
        row["preset"] = {"remove": [len(boxes)]}
    return row


def from_picks(set_name, picks):
    """The crops a picks.jsonl names, with their crop files."""
    for line in picks.read_text(encoding="utf8").splitlines():
        pick = json.loads(line)
        crop = np.load(picks.parent / pick["file"], allow_pickle=True)
        row = entry(set_name, pick["file"], crop, pick.get("folder", ""), pick.get("kind", ""), pick.get("why", []),
                    pick.get("rule"))
        yield crop, row | {key: pick[key] for key in ("boxes", "scores", "preset") if key in pick}


def from_folder(set_name, folder):
    """Every crop file below a folder, with its recording and kind from the folder's manifest.jsonl."""
    manifest = folder / "manifest.jsonl"
    recordings = [json.loads(line) for line in manifest.open(encoding="utf8")] if manifest.exists() else []
    for path in sorted(folder.glob("**/*.npz")):
        crop = np.load(path, allow_pickle=True)
        known = next((row for row in recordings if row.get("stem") and path.name.startswith(row["stem"])), {})
        rule = str(crop["mined"]) if "mined" in crop.files else None
        why = [str(crop["why"])] if "why" in crop.files else []
        yield crop, entry(set_name, path.relative_to(folder).as_posix(), crop, known.get("folder", ""),
                          known.get("kind", ""), why, rule)


def main():
    """Writes the set's PNGs, crops.json, sets.json and index.html into the page folder, a publish_<set>_<n>.json per
    batch of files the Artifact tool takes, and prints how to publish them."""
    parser = argparse.ArgumentParser(description="Adds a set of crops to a crop-check page folder (README.md).")
    parser.add_argument("page", type=Path)
    parser.add_argument("set_name", metavar="set")
    parser.add_argument("source", type=Path)
    parser.add_argument("--title")
    parser.add_argument("--crossed-out")
    parser.add_argument("--no-learn", action="store_true")
    args = parser.parse_args()
    (args.page / "crops").mkdir(parents=True, exist_ok=True)
    index = args.page / "crops.json"
    rows = [row for row in (json.loads(index.read_text()) if index.exists() else []) if row["set"] != args.set_name]
    source = from_picks if args.source.suffix == ".jsonl" else from_folder
    added = []
    for crop, row in source(args.set_name, args.source):
        Image.fromarray(crop["rgb"]).save(args.page / "crops" / f"{row['id']}.png", optimize=True)
        added.append(row)
    index.write_text(json.dumps(rows + added, separators=(",", ":")))
    sets_file = args.page / "sets.json"
    sets = json.loads(sets_file.read_text()) if sets_file.exists() else {}
    info = {"title": args.title} if args.title else {}
    if args.crossed_out:
        info["crossedOut"] = args.crossed_out
    if args.no_learn:
        info["learn"] = False
    sets[args.set_name] = info
    sets_file.write_text(json.dumps(sets, indent=1))
    shutil.copyfile(HERE / "index.html", args.page / "index.html")
    # the new PNGs in batches the Artifact tool takes, crops.json and sets.json with the first
    files = ["crops.json", "sets.json"] + [f"crops/{row['id']}.png" for row in added]
    batches = [files[at:at + PUBLISH_LIMIT] for at in range(0, len(files), PUBLISH_LIMIT)]
    for number, batch in enumerate(batches, 1):
        (args.page / f"publish_{args.set_name}_{number}.json").write_text(json.dumps({path: path for path in batch}))
    total = len(rows) + len(added) + 3
    print(f"{len(added)} crops in set {args.set_name}, {len(rows) + len(added)} in all; publish index.html with "
          f"files from publish_{args.set_name}_1.json to {len(batches)}.json"
          + (f" (over {VERSION_LIMIT} files a version: leave old sets' PNGs out)" if total > VERSION_LIMIT else ""))


if __name__ == "__main__":
    main()
