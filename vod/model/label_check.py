"""Check the automatic labels by hand: a small local page that shows one 256 x 256 training crop at a time, with its
boxes, and saves what you confirm or fix. The checked crops are the first ground truth (MODEL_STATUS.md, "Known
limitations"): the crop scores mean something once the labels they are scored against were looked at.

Which crops: where the current model (on the CPU, ONNX Runtime) and the automatic labels disagree at the threshold,
then crops with a target near the crosshair (the weak case), then random ones; from every split.

On the page: click a target's centre to add a box (drag to size it), click a box to remove it, Enter or "Correct" to
save and go on, "Skip" to leave a crop out. Saved to test_out/vod_model/checked.jsonl, one line per crop:
{"file": ..., "boxes": [[cx, cy, w, h], ...], "verdict": "correct" | "skip" | "unsure", "auto": [...], "model": [...]}.
"skip" means no target in the crop; "unsure" leaves the crop out.
Usage: python vod/model/label_check.py [--n 400] [--port 8773]   then open http://127.0.0.1:8773/
"""
import argparse
import base64
import io
import json
import random
import sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

import numpy as np

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import infer  # noqa: E402

DATA = HERE.parents[1] / "test_out" / "vod_model" / "data"
OUT = HERE.parents[1] / "test_out" / "vod_model" / "checked.jsonl"


def pick(n, seed=1):
    """The crops to check, most informative first: disagreements, then a target near the crosshair, then random."""
    det = infer.OnnxDetector(HERE / "exports" / f"detector_{infer.BEST}_u8in.onnx", threads=4)
    done = set()
    if OUT.exists():
        done = {json.loads(line)["file"] for line in OUT.read_text(encoding="utf-8").splitlines() if line.strip()}
    files = [f for f in sorted(DATA.glob("*/*.npz")) if f"{f.parent.name}/{f.name}" not in done]
    rnd = random.Random(seed)
    rnd.shuffle(files)
    disagree, near, other = [], [], []
    for f in files[:6 * n]:
        z = np.load(f)
        auto = z["boxes"]
        model = det(z["rgb"], z["fixed"], infer.THRESHOLD)
        used, miss = set(), 0
        for b in auto:
            d = [np.hypot(m[0] - b[0], m[1] - b[1]) for m in model]
            j = int(np.argmin(d)) if d else -1
            if j < 0 or d[j] > max(2.0, 0.5 * max(b[2], b[3])) or j in used:
                miss += 1
            else:
                used.add(j)
        extra = len(model) - len(used)
        fx = np.argwhere(z["fixed"] > 0)
        cross = fx.mean(0)[::-1] if len(fx) else None
        item = (f, auto, model)
        if miss or extra:
            disagree.append(item)
        elif cross is not None and any(np.hypot(b[0] - cross[0], b[1] - cross[1]) < 12 for b in auto):
            near.append(item)
        else:
            other.append(item)
    order = disagree + near + other
    return order[:n], dict(disagree=len(disagree), near=len(near), other=len(other))


PAGE = """<!doctype html><html lang="en"><head><meta charset="utf-8"><title>Check the labels</title>
<style>
body { font: 14px/1.4 "Segoe UI", system-ui, sans-serif; background: #121211; color: #fff; margin: 18px; }
#wrap { display: flex; gap: 20px; align-items: flex-start; }
canvas { border-radius: 6px; cursor: crosshair; image-rendering: pixelated; }
button { background: #232321; color: #fff; border: 1px solid rgba(255,255,255,.12); border-radius: 6px; padding: 7px 16px;
  font: inherit; cursor: pointer; margin-right: 6px; }
button.go { background: #3987e5; border-color: #3987e5; }
.muted { color: #8f8e86; } .k { display: inline-block; width: 12px; height: 12px; border-radius: 6px; margin-right: 6px; vertical-align: -1px; }
</style></head><body>
<h2>Check the labels</h2>
<p class="muted">Every target in the crop should have a ring, and nothing else. Click a target's centre to add one (drag
to size it); click a ring to remove it. The crosshair and the HUD are not targets.</p>
<div id="wrap"><canvas id="c" width="768" height="768"></canvas><div>
<p id="pos"></p>
<p><span class="k" style="background:#fab219"></span>your boxes (start from the automatic labels)<br>
<span class="k" style="border:2px dashed #3987e5"></span>what the model found, for reference</p>
<p><button class="go" id="ok">Correct (Enter)</button><button id="skip">Skip</button><button id="unsure">Can't tell</button><button id="undo">Undo</button></p>
<p id="info" class="muted"></p></div></div>
<script>
const S = 3, c = document.getElementById("c"), g = c.getContext("2d");
let cur = null, boxes = [], hist = [], img = new Image(), drag = null;
async function next() {
  const r = await fetch("/api/next").then(r => r.json());
  if (r.done) { document.getElementById("pos").textContent = "All done: " + r.checked + " crops checked."; cur = null; draw(); return; }
  cur = r; boxes = r.auto.map(b => b.slice()); hist = [];
  img = new Image(); img.onload = draw; img.src = "data:image/png;base64," + r.png;
  document.getElementById("pos").textContent = `Crop ${r.index + 1} of ${r.total} (${r.checked} checked) · ${r.why}`;
  document.getElementById("info").textContent = r.file;
}
function draw() {
  g.clearRect(0, 0, 768, 768); if (!cur) return;
  g.imageSmoothingEnabled = false; g.drawImage(img, 0, 0, 768, 768);
  g.setLineDash([5, 4]); g.strokeStyle = "#3987e5"; g.lineWidth = 2;
  for (const m of cur.model) { g.beginPath(); g.arc(m[0] * S, m[1] * S, Math.max(6, m[2] * S / 2 + 6), 0, 7); g.stroke(); }
  g.setLineDash([]); g.strokeStyle = "#fab219"; g.lineWidth = 2;
  for (const b of boxes) { g.beginPath(); g.arc(b[0] * S, b[1] * S, Math.max(4, b[2] * S / 2 + 3), 0, 7); g.stroke(); }
}
const at = (e) => { const r = c.getBoundingClientRect(); return [(e.clientX - r.left) / S, (e.clientY - r.top) / S]; };
c.addEventListener("mousedown", (e) => {
  const [x, y] = at(e);
  const hit = boxes.findIndex(b => Math.hypot(b[0] - x, b[1] - y) <= Math.max(3, b[2] / 2 + 2));
  hist.push(boxes.map(b => b.slice()));
  if (hit >= 0) { boxes.splice(hit, 1); draw(); return; }
  const med = cur.auto.length ? cur.auto.map(b => b[2]).sort((a, b) => a - b)[cur.auto.length >> 1] : 8;
  drag = [x, y, med, med]; boxes.push(drag); draw();
});
c.addEventListener("mousemove", (e) => { if (!drag) return; const [x, y] = at(e); const d = Math.max(2, 2 * Math.hypot(x - drag[0], y - drag[1])); drag[2] = drag[3] = d; draw(); });
addEventListener("mouseup", () => { drag = null; });
async function save(verdict) {
  if (!cur) return;
  await fetch("/api/save", { method: "POST", body: JSON.stringify({ file: cur.file, boxes, verdict, auto: cur.auto, model: cur.model }) });
  next();
}
document.getElementById("ok").onclick = () => save("correct");
document.getElementById("skip").onclick = () => save("skip");
document.getElementById("unsure").onclick = () => save("unsure");
document.getElementById("undo").onclick = () => { if (hist.length) { boxes = hist.pop(); draw(); } };
addEventListener("keydown", (e) => { if (e.key === "Enter") save("correct"); if (e.key === "Backspace") document.getElementById("undo").click(); });
next();
</script></body></html>"""


class Handler(BaseHTTPRequestHandler):
    queue, why, pos = [], {}, 0

    def log_message(self, fmt, *args):
        pass

    def send(self, body, kind="application/json", code=200):
        b = body.encode() if isinstance(body, str) else body
        self.send_response(code)
        self.send_header("Content-Type", kind)
        self.send_header("Content-Length", str(len(b)))
        self.end_headers()
        self.wfile.write(b)

    def checked(self):
        return sum(1 for line in OUT.read_text(encoding="utf-8").splitlines() if line.strip()) if OUT.exists() else 0

    def do_GET(self):
        if self.path == "/":
            return self.send(PAGE, "text/html; charset=utf-8")
        if self.path == "/api/next":
            cls = type(self)
            if cls.pos >= len(cls.queue):
                return self.send(json.dumps(dict(done=True, checked=self.checked())))
            f, auto, model = cls.queue[cls.pos]
            z = np.load(f)
            from PIL import Image
            buf = io.BytesIO()
            Image.fromarray(z["rgb"]).save(buf, "PNG")
            return self.send(json.dumps(dict(
                file=f"{f.parent.name}/{f.name}", index=cls.pos, total=len(cls.queue), checked=self.checked(),
                why=cls.why.get(f, ""), png=base64.b64encode(buf.getvalue()).decode(),
                auto=[[round(float(v), 2) for v in b] for b in auto],
                model=[[round(float(v), 2) for v in m[:4]] for m in model])))
        self.send(json.dumps(dict(error="not found")), code=404)

    def do_POST(self):
        if self.path != "/api/save":
            return self.send(json.dumps(dict(error="not found")), code=404)
        rec = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        OUT.parent.mkdir(parents=True, exist_ok=True)
        with open(OUT, "a", encoding="utf-8") as fh:
            fh.write(json.dumps(rec) + "\n")
        type(self).pos += 1
        self.send(json.dumps(dict(ok=True)))


def main():
    global DATA, OUT
    ap = argparse.ArgumentParser()
    ap.add_argument("--n", type=int, default=400)
    ap.add_argument("--port", type=int, default=8773)
    ap.add_argument("--data", default=str(DATA), help="the dataset folder (train, val, test inside)")
    ap.add_argument("--out", default=str(OUT), help="where the checked crops are saved")
    a = ap.parse_args()
    DATA, OUT = Path(a.data), Path(a.out)
    queue, counts = pick(a.n)
    Handler.queue = queue
    n_dis = min(counts["disagree"], a.n)
    for k, (f, _, _) in enumerate(queue):
        Handler.why[f] = ("the model and the automatic labels disagree" if k < n_dis
                          else "a target near the crosshair" if k < n_dis + counts["near"] else "a random crop")
    print(f"{len(queue)} crops to check ({counts}); http://127.0.0.1:{a.port}/", flush=True)
    ThreadingHTTPServer(("127.0.0.1", a.port), Handler).serve_forever()


if __name__ == "__main__":
    main()
