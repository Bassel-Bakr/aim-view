"""Tests for the detector's code (REPRODUCE.md, "Tests"). Standard library unittest; no pytest needed.
  python vod/model/test_model.py
Tests that need generated files (the dataset manifest, the exports, the benchmark frame) skip when those are missing.
"""
import json
import random
import sys
import threading
import unittest
import urllib.error
import urllib.request
from http.server import ThreadingHTTPServer
from pathlib import Path

import numpy as np
import torch

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parent))
import export  # noqa: E402
import infer  # noqa: E402
import net  # noqa: E402
import train  # noqa: E402

DATA = ROOT / "test_out" / "vod_model" / "data"
EXPORTS = HERE / "exports"
FRAME = ROOT / "test_out" / "vod_model" / "bench_frame.npz"
EXPECTED = ROOT / "test_out" / "vod_model" / "bench_expected.json"


class Augment(unittest.TestCase):
    def test_flips_and_turns_move_boxes_with_pixels(self):
        S = 256
        for seed in range(32):
            random.seed(seed)
            x, y = 37, 101
            img = torch.zeros(1, 3, S, S)
            img[0, :, y, x] = 1
            fixed, tmask = torch.zeros(1, 1, S, S), torch.zeros(1, 1, S, S)
            boxes = torch.tensor([[[x, y, 4.0, 2.0]]])
            img, _, _, boxes = train.flip_rot(img, fixed, tmask, boxes, torch.tensor([1]))
            iy, ix = divmod(int(img[0, 0].flatten().argmax()), S)
            self.assertEqual((ix, iy), (int(boxes[0, 0, 0]), int(boxes[0, 0, 1])), f"seed {seed}")

    def test_crosshairs_keep_targets_and_mark_the_fixed_map(self):
        random.seed(0)
        img, fixed = torch.full((4, 3, 64, 64), 0.5), torch.zeros(4, 1, 64, 64)
        boxes = torch.tensor([[[30.0, 30.0, 8.0, 8.0]]] * 4)
        before = boxes.clone()
        img, fixed = train.crosshairs(img, fixed, boxes, torch.tensor([1] * 4), p=1.0, on_target=1.0, jitter=0.6)
        self.assertTrue(torch.equal(boxes, before))
        for b in range(4):
            self.assertGreater(int(fixed[b].sum()), 0)
            ys, xs = torch.nonzero(fixed[b, 0], as_tuple=True)    # drawn near the target (within jitter + arm)
            self.assertLess(abs(float(xs.float().mean()) - 30), 8)
            self.assertLess(abs(float(ys.float().mean()) - 30), 8)


    def test_outlines_ring_the_target_and_spare_the_crosshair(self):
        random.seed(1)
        img = torch.full((1, 3, 64, 64), 0.5)
        tmask, fixed = torch.zeros(1, 1, 64, 64), torch.zeros(1, 1, 64, 64)
        yy, xx = torch.meshgrid(torch.arange(64), torch.arange(64), indexing="ij")
        tmask[0, 0][(xx - 30) ** 2 + (yy - 30) ** 2 <= 16] = 1         # a target of radius 4
        fixed[0, 0, 28:33, 34:37] = 1                                 # a crosshair touching it
        before = img.clone()
        out = train.outlines(img.clone(), tmask, fixed, p=1.0)
        changed = (out - before).abs().sum(1)[0] > 1e-6
        self.assertTrue(changed.any())
        self.assertFalse(changed[tmask[0, 0] > 0].any())              # the target itself is untouched
        self.assertFalse(changed[fixed[0, 0] > 0].any())              # so is the crosshair
        d = ((xx - 30) ** 2 + (yy - 30) ** 2).float().sqrt()
        self.assertLessEqual(float(d[changed].max()), 4 + 4.5)        # the ring hugs the target


class Decoding(unittest.TestCase):
    def test_exported_decoding_matches_pytorch_decoding(self):
        torch.manual_seed(0)
        model = net.build(json.loads((HERE / "configs" / "tiny.json").read_text())).eval()
        x = torch.rand(1, 4, 64, 96)
        with torch.no_grad():
            out = model(x)
            score, reg = export.Exported(model).eval()(x)
        a = net.decode(out, thr=0.0)[0].numpy()
        b = infer.decode_np(score.numpy(), reg.numpy(), thr=0.0)
        key = lambda d: d[np.lexsort((d[:, 1], d[:, 0]))]
        self.assertEqual(len(a), len(b))
        np.testing.assert_allclose(key(a), key(b), atol=1e-4)


class Splits(unittest.TestCase):
    def test_no_scenario_in_two_splits(self):
        m = DATA / "manifest.jsonl"
        if not m.exists():
            self.skipTest("no dataset built")
        seen = {}
        for line in m.read_text(encoding="utf-8").splitlines():
            r = json.loads(line)
            seen.setdefault(r["folder"], set()).add(r["split"])
        self.assertEqual([f for f, s in seen.items() if len(s) > 1], [])

    def test_end_to_end_vods_are_in_test(self):
        import build_data
        for f in build_data.TEST_FOLDERS:
            self.assertEqual(build_data.split_of(f), "test", f)


@unittest.skipUnless((EXPORTS / "detector_small_fp32.onnx").exists() and FRAME.exists() and EXPECTED.exists(),
                     "needs the exports and the benchmark frame")
class Exports(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        z = np.load(FRAME)
        cls.rgb, cls.fixed = z["rgb"], z["fixed"]
        cls.expected = np.array(json.loads(EXPECTED.read_text())["detections"], np.float32)

    def test_fp32_and_fp16_give_the_expected_detections(self):
        for name in ("detector_small_fp32", "detector_small_fp16"):
            d = infer.OnnxDetector(EXPORTS / f"{name}.onnx")(self.rgb, self.fixed, 0.3)
            d = d[np.argsort(d[:, 0])]
            self.assertEqual(len(d), len(self.expected), name)
            np.testing.assert_allclose(d[:, :2], self.expected[:, :2], atol=0.05, err_msg=name)

    def test_uint8_input_graph_equals_the_float_graph(self):
        f32 = EXPORTS / f"detector_{infer.BEST}_fp32.onnx"
        u8 = EXPORTS / f"detector_{infer.BEST}_u8in.onnx"
        if not u8.exists():
            self.skipTest("no _u8in export")
        a = infer.OnnxDetector(f32)(self.rgb, self.fixed, 0.3)
        b = infer.OnnxDetector(u8)(self.rgb, self.fixed, 0.3)
        np.testing.assert_allclose(a, b, atol=1e-5)

    def test_embed_graph_gives_the_same_boxes(self):
        emb = EXPORTS / f"detector_{infer.BEST}_embed.onnx"
        if not emb.exists():
            self.skipTest("no _embed export")
        a = infer.OnnxDetector(EXPORTS / f"detector_{infer.BEST}_fp32.onnx")
        b = infer.OnnxDetector(emb)
        for rgb, fixed in ((self.rgb, self.fixed), (self.rgb[296:552, 576:832], self.fixed[296:552, 576:832])):
            da, db = a(rgb, fixed, 0.3), b(rgb, fixed, 0.3)
            order = lambda d: d[np.argsort(d[:, 0])]
            self.assertEqual(len(da), len(db))
            np.testing.assert_allclose(order(da), order(db), atol=1e-4)

    def test_int8_finds_the_same_targets(self):
        d = infer.OnnxDetector(EXPORTS / "detector_small_int8.onnx")(self.rgb, self.fixed, 0.3)
        d = d[np.argsort(d[:, 0])]
        self.assertEqual(len(d), len(self.expected))
        np.testing.assert_allclose(d[:, :2], self.expected[:, :2], atol=1.0)

    @staticmethod
    def get(req):
        with urllib.request.urlopen(req) as r:
            return json.load(r)

    def test_http_api(self):
        import serve
        serve.Handler.det = infer.OnnxDetector(EXPORTS / "detector_small_fp32.onnx", 2)
        serve.Handler.model = "detector_small_fp32.onnx"
        srv = ThreadingHTTPServer(("127.0.0.1", 0), serve.Handler)
        threading.Thread(target=srv.serve_forever, daemon=True).start()
        base = f"http://127.0.0.1:{srv.server_address[1]}"
        try:
            self.assertEqual(self.get(base + "/health")["model"], "detector_small_fp32.onnx")
            body = self.rgb.tobytes() + self.fixed.tobytes()
            r = self.get(urllib.request.Request(base + "/detect?w=1280&h=720&fixed=1", body))
            d = np.array(sorted(r["detections"]), np.float32)
            np.testing.assert_allclose(d[:, :2], self.expected[:, :2], atol=0.05)
            with self.assertRaises(urllib.error.HTTPError) as e:
                self.get(urllib.request.Request(base + "/detect?w=1280&h=720", body[:100]))
            self.assertEqual(e.exception.code, 400)
            e.exception.close()
        finally:
            srv.shutdown()


if __name__ == "__main__":
    unittest.main(verbosity=2)
