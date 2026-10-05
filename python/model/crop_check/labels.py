"""The labels from a crop-check page's answers (README.md), in label_check's checked.jsonl format: one line per answered
crop, its file relative to the crops' source, as checked_data.py reads it.

Usage: python python/model/crop_check/labels.py <page folder> <answers folder> <out jsonl> <set> [<set> ...]
       [--also <answers folder> ...] [--prefer SET] [--point-size folder|crop]

<answers folder> (and each --also folder; a crop answered in several takes its newest answer, by `at`, as the Crops
page shows it) holds one <crop id>.json per answer: the page folder's answers/checks/ for the app's Crops page, or
the claude.ai page's `checks` collection as Claude's ArtifactData tool saves it (list with out_dir). An answer drawn on
the Crops page carries its scene (shapes joined into targets, occluders): its labels come from the core
(aimview-tool crop-labels, which reads the page folder's answers*/checks/; the page folder must be test_out's
vod_model/check_*): one box per target that shows, the box round each target hidden entirely ("covered":
checked_data.py makes it an ignore box), and the targets' visible pixels ("mask", run lengths over the crop's rows,
the first of pixels not set). An answer of the claude.ai page has its cross-outs, moves, resizes and drawn boxes
applied to the crop's boxes; a tapped point becomes a box of the median size of the recording's final boxes
(--point-size folder, the default) or of the crop's own (crop), 16 px without any. Wrong with nothing changed (on the
claude.ai page), and Unsure, keep the crop's boxes as unsure; a crop left with no box is skip. --prefer: where a crop
of that set shows the same file and has an answer (a Tighten pass), that answer wins.
"""
import argparse
import collections
import json
import statistics
import sys
from pathlib import Path

DEFAULT_SIZE_PX = 16.0
# an added box is [cx, cy, w, h]; a tapped point is [x, y]
BOX_VALUES, POINT_VALUES = 4, 2
ANSWER_FIELDS = ("verdict", "remove", "add", "edit", "suggested", "scene")
PYTHON = Path(__file__).resolve().parents[2]   # python/, where aimview_tools.py is


def read_answers(folders):
    """Each crop's answer from the folders: where several hold one, the newest (`at`)."""
    answers = {}
    for folder in folders:
        for path in folder.glob("*.json"):
            saved = json.loads(path.read_text())
            answer = saved.get("data", saved)
            held = answers.get(path.stem)
            if held is None or answer.get("at", 0) > held.get("at", 0):
                answers[path.stem] = answer
    return answers


def scene_labels(page, answers):
    """The core's labels of the answers drawn on the Crops page (those with a scene), by crop id; none to make without
    running the tool."""
    if not any(answer.get("scene") for answer in answers.values()):
        return {}
    sys.path.insert(0, str(PYTHON))
    import aimview_tools
    labels = aimview_tools.run("crop-labels", page.name, "--data", page.resolve().parent.parent)["labels"]
    # the tool reads every answers*/checks of the page: its label must be of the answer chosen here
    for crop_id, answer in answers.items():
        label = labels.get(crop_id)
        if answer.get("scene") and (label is None or label["at"] != answer.get("at")):
            raise SystemExit(f"{crop_id}: the core labelled another answer than this one (a newer answer in an answers "
                             "folder not given? add it with --also)")
    return labels


def applied(crop, answer, label):
    """The crop's boxes after an answer and its tapped points; (None, []) when it stays unsure. `label`: the core's,
    for an answer drawn on the Crops page."""
    if answer["verdict"] == "unsure":
        return None, []
    if label is not None:
        return [list(box) for box in label["boxes"]], []
    remove, edit, add = set(answer.get("remove", [])), answer.get("edit") or {}, answer.get("add", [])
    if answer["verdict"] == "wrong" and not remove and not edit and not add:
        return None, []
    boxes = [list(edit.get(str(i), box)) for i, box in enumerate(crop["boxes"]) if i not in remove]
    boxes += [list(box) for box in add if len(box) == BOX_VALUES]
    return boxes, [point for point in add if len(point) == POINT_VALUES]


def median_size(boxes):
    if not boxes:
        return DEFAULT_SIZE_PX, DEFAULT_SIZE_PX
    return statistics.median(box[2] for box in boxes), statistics.median(box[3] for box in boxes)


def main():
    parser = argparse.ArgumentParser(description="The labels from a crop-check page's answers (README.md).")
    parser.add_argument("page", type=Path)
    parser.add_argument("answers", type=Path)
    parser.add_argument("out", type=Path)
    parser.add_argument("sets", nargs="+")
    parser.add_argument("--also", type=Path, nargs="+", default=[], help="more answers folders")
    parser.add_argument("--prefer")
    parser.add_argument("--point-size", choices=("folder", "crop"), default="folder")
    args = parser.parse_args()
    crops = {crop["id"]: crop for crop in json.loads((args.page / "crops.json").read_text())}
    answers = read_answers([args.answers, *args.also])
    labels = scene_labels(args.page, answers)
    preferred = {crop["file"]: crop for crop in crops.values() if crop["set"] == args.prefer and crop["id"] in answers}
    final = {}
    for crop_id, crop in crops.items():
        if crop["set"] not in args.sets or crop_id not in answers:
            continue
        base = preferred.get(crop["file"], crop)
        answer = answers[base["id"]]
        label = labels.get(base["id"]) if answer.get("scene") else None
        boxes, points = applied(base, answer, label)
        source = base["set"] + (f":{crop['rule']}" if crop.get("rule") else "")
        final[crop["file"]] = dict(crop=crop, boxes=boxes, points=points, source=source, answer=answer, label=label)
    sizes = collections.defaultdict(list)
    for row in final.values():
        sizes[row["crop"]["folder"]] += row["boxes"] or []
    lines = []
    for file, row in sorted(final.items()):
        crop, boxes = row["crop"], row["boxes"]
        if boxes is None:
            boxes, verdict = crop["boxes"], "unsure"
        else:
            width, height = median_size(sizes[crop["folder"]] if args.point_size == "folder" else boxes)
            boxes = boxes + [[x, y, round(width, 1), round(height, 1)] for x, y in row["points"]]
            verdict = "correct" if boxes else "skip"
        line = {"file": file, "boxes": [[round(value, 1) for value in box] for box in boxes], "verdict": verdict,
                "auto": crop["boxes"], "model": crop["boxes"], "source": row["source"],
                "phone": {field: row["answer"].get(field) for field in ANSWER_FIELDS}}
        if row["label"] is not None and verdict != "unsure":
            line["covered"] = [[round(value, 1) for value in box] for box in row["label"]["ignore"]]
            line["mask"] = row["label"]["mask"]
        lines.append(line)
    args.out.write_text("".join(json.dumps(line) + "\n" for line in lines), encoding="utf8")
    print(len(lines), "crops;", dict(collections.Counter(line["verdict"] for line in lines)),
          dict(collections.Counter(line["source"] for line in lines)))


if __name__ == "__main__":
    main()
