"""The labels from a crop-check page's answers (README.md), in label_check's checked.jsonl format: one line per answered
crop, its file relative to the crops' source, as checked_data.py reads it.

Usage: python python/model/crop_check/labels.py <page folder> <answers folder> <out jsonl> <set> [<set> ...]
       [--prefer SET] [--point-size folder|crop]

<answers folder> holds the page's `checks` collection as Claude's ArtifactData tool saves it (list with out_dir: one
<crop id>.json each). Each answer's cross-outs, moves, resizes and drawn boxes are applied to the crop's boxes. A
tapped point becomes a box of the median size of the recording's final boxes (--point-size folder, the default) or of
the crop's own (crop), 16 px without any. Wrong with nothing changed, and Unsure, keep the crop's boxes as unsure; a crop
left with no box is skip. --prefer: where a crop of that set shows the same file and has an answer (a Tighten pass),
that answer wins.
"""
import argparse
import collections
import json
import statistics
from pathlib import Path

DEFAULT_SIZE_PX = 16.0
# an added box is [cx, cy, w, h]; a tapped point is [x, y]
BOX_VALUES, POINT_VALUES = 4, 2
ANSWER_FIELDS = ("verdict", "remove", "add", "edit", "suggested")


def read_answers(folder):
    answers = {}
    for path in folder.glob("*.json"):
        saved = json.loads(path.read_text())
        answers[path.stem] = saved.get("data", saved)
    return answers


def applied(crop, answer):
    """The crop's boxes after an answer and its tapped points; (None, []) when it stays unsure."""
    remove, edit, add = set(answer.get("remove", [])), answer.get("edit") or {}, answer.get("add", [])
    if answer["verdict"] == "unsure" or (answer["verdict"] == "wrong" and not remove and not edit and not add):
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
    parser.add_argument("--prefer")
    parser.add_argument("--point-size", choices=("folder", "crop"), default="folder")
    args = parser.parse_args()
    crops = {crop["id"]: crop for crop in json.loads((args.page / "crops.json").read_text())}
    answers = read_answers(args.answers)
    preferred = {crop["file"]: crop for crop in crops.values() if crop["set"] == args.prefer and crop["id"] in answers}
    final = {}
    for crop_id, crop in crops.items():
        if crop["set"] not in args.sets or crop_id not in answers:
            continue
        base = preferred.get(crop["file"], crop)
        boxes, points = applied(base, answers[base["id"]])
        source = base["set"] + (f":{crop['rule']}" if crop.get("rule") else "")
        final[crop["file"]] = dict(crop=crop, boxes=boxes, points=points, source=source, answer=answers[base["id"]])
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
        lines.append({"file": file, "boxes": [[round(value, 1) for value in box] for box in boxes], "verdict": verdict,
                      "auto": crop["boxes"], "model": crop["boxes"], "source": row["source"],
                      "phone": {field: row["answer"].get(field) for field in ANSWER_FIELDS}})
    args.out.write_text("".join(json.dumps(line) + "\n" for line in lines), encoding="utf8")
    print(len(lines), "crops;", dict(collections.Counter(line["verdict"] for line in lines)),
          dict(collections.Counter(line["source"] for line in lines)))


if __name__ == "__main__":
    main()
