"""Tests for the detector's code (REPRODUCE.md, "Tests"). Standard library unittest; no pytest needed.
  python python/model/test_model.py
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
CROP_PX = 256
TARGET_RADIUS_PX = 4                # the outline test's target
OUTLINE_REACH_PX = 4.5              # how far past the target an outline may reach
CHANGED = 1e-6                      # a pixel changed by more than this was drawn on
HTTP_BAD_REQUEST = 400


def by_x(detections):
    """Detections in order of their x."""
    return detections[np.argsort(detections[:, 0])]


def by_place(detections):
    """Detections in order of their place (x, then y)."""
    return detections[np.lexsort((detections[:, 1], detections[:, 0]))]


class Augment(unittest.TestCase):
    def test_flips_and_turns_move_boxes_with_pixels(self):
        for seed in range(32):
            random.seed(seed)
            x, y = 37, 101
            image = torch.zeros(1, 3, CROP_PX, CROP_PX)
            image[0, :, y, x] = 1
            fixed, target_mask = torch.zeros(1, 1, CROP_PX, CROP_PX), torch.zeros(1, 1, CROP_PX, CROP_PX)
            boxes = torch.tensor([[[x, y, 4.0, 2.0]]])
            image, _, _, boxes = train.flip_rot(image, fixed, target_mask, boxes, torch.tensor([1]))
            lit_y, lit_x = divmod(int(image[0, 0].flatten().argmax()), CROP_PX)
            self.assertEqual((lit_x, lit_y), (int(boxes[0, 0, 0]), int(boxes[0, 0, 1])), f"seed {seed}")

    def test_crosshairs_keep_targets_and_mark_the_fixed_map(self):
        random.seed(0)
        image, fixed = torch.full((4, 3, 64, 64), 0.5), torch.zeros(4, 1, 64, 64)
        boxes = torch.tensor([[[30.0, 30.0, 8.0, 8.0]]] * 4)
        before = boxes.clone()
        image, fixed = train.crosshairs(image, fixed, boxes, torch.tensor([1] * 4), p=1.0, on_target=1.0,
                                        jitter=0.6)
        self.assertTrue(torch.equal(boxes, before))
        for crop in range(4):
            self.assertGreater(int(fixed[crop].sum()), 0)
            ys, xs = torch.nonzero(fixed[crop, 0], as_tuple=True)    # drawn near the target (within jitter + arm)
            self.assertLess(abs(float(xs.float().mean()) - 30), 8)
            self.assertLess(abs(float(ys.float().mean()) - 30), 8)


    def test_outlines_ring_the_target_and_spare_the_crosshair(self):
        random.seed(1)
        image = torch.full((1, 3, 64, 64), 0.5)
        target_mask, fixed = torch.zeros(1, 1, 64, 64), torch.zeros(1, 1, 64, 64)
        yy, xx = torch.meshgrid(torch.arange(64), torch.arange(64), indexing="ij")
        target_mask[0, 0][(xx - 30) ** 2 + (yy - 30) ** 2 <= TARGET_RADIUS_PX ** 2] = 1
        fixed[0, 0, 28:33, 34:37] = 1                                 # a crosshair touching it
        before = image.clone()
        out = train.outlines(image.clone(), target_mask, fixed, p=1.0)
        changed = (out - before).abs().sum(1)[0] > CHANGED
        self.assertTrue(changed.any())
        self.assertFalse(changed[target_mask[0, 0] > 0].any())        # the target itself is untouched
        self.assertFalse(changed[fixed[0, 0] > 0].any())              # so is the crosshair
        distance = ((xx - 30) ** 2 + (yy - 30) ** 2).float().sqrt()
        self.assertLessEqual(float(distance[changed].max()), TARGET_RADIUS_PX + OUTLINE_REACH_PX)  # it hugs the target


class Decoding(unittest.TestCase):
    def test_exported_decoding_matches_pytorch_decoding(self):
        torch.manual_seed(0)
        model = net.build(json.loads((HERE / "configs" / "tiny.json").read_text())).eval()
        frames = torch.rand(1, 4, 64, 96)
        with torch.no_grad():
            out = model(frames)
            score, reg = export.Exported(model).eval()(frames)
        a = net.decode(out, threshold=0.0)[0].numpy()
        b = infer.decode_np(score.numpy(), reg.numpy(), threshold=0.0)
        self.assertEqual(len(a), len(b))
        np.testing.assert_allclose(by_place(a), by_place(b), atol=1e-4)


class Splits(unittest.TestCase):
    def test_no_scenario_in_two_splits(self):
        manifest = DATA / "manifest.jsonl"
        if not manifest.exists():
            self.skipTest("no dataset built")
        seen = {}
        for line in manifest.read_text(encoding="utf-8").splitlines():
            row = json.loads(line)
            seen.setdefault(row["folder"], set()).add(row["split"])
        self.assertEqual([folder for folder, splits in seen.items() if len(splits) > 1], [])

    def test_end_to_end_vods_are_in_test(self):
        import build_data
        for folder in build_data.TEST_FOLDERS:
            self.assertEqual(build_data.split_of(folder), "test", folder)


@unittest.skipUnless((EXPORTS / "detector_small_fp32.onnx").exists() and FRAME.exists() and EXPECTED.exists(),
                     "needs the exports and the benchmark frame")
class Exports(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        sample = np.load(FRAME)
        cls.rgb, cls.fixed = sample["rgb"], sample["fixed"]
        cls.expected = np.array(json.loads(EXPECTED.read_text())["detections"], np.float32)

    def test_fp32_and_fp16_give_the_expected_detections(self):
        for name in ("detector_small_fp32", "detector_small_fp16"):
            found = by_x(infer.OnnxDetector(EXPORTS / f"{name}.onnx")(self.rgb, self.fixed, 0.3))
            self.assertEqual(len(found), len(self.expected), name)
            np.testing.assert_allclose(found[:, :2], self.expected[:, :2], atol=0.05, err_msg=name)

    def test_uint8_input_graph_equals_the_float_graph(self):
        f32 = EXPORTS / f"detector_{infer.BEST}_fp32.onnx"
        u8 = EXPORTS / f"detector_{infer.BEST}_u8in.onnx"
        if not u8.exists():
            self.skipTest("no _u8in export")
        a = infer.OnnxDetector(f32)(self.rgb, self.fixed, 0.3)
        b = infer.OnnxDetector(u8)(self.rgb, self.fixed, 0.3)
        np.testing.assert_allclose(a, b, atol=1e-5)

    def test_embed_graph_gives_the_same_boxes(self):
        embed = EXPORTS / f"detector_{infer.BEST}_embed.onnx"
        if not embed.exists():
            self.skipTest("no _embed export")
        a = infer.OnnxDetector(EXPORTS / f"detector_{infer.BEST}_fp32.onnx")
        b = infer.OnnxDetector(embed)
        for rgb, fixed in ((self.rgb, self.fixed), (self.rgb[296:552, 576:832], self.fixed[296:552, 576:832])):
            found_a, found_b = a(rgb, fixed, 0.3), b(rgb, fixed, 0.3)
            self.assertEqual(len(found_a), len(found_b))
            np.testing.assert_allclose(by_x(found_a), by_x(found_b), atol=1e-4)

    def test_int8_finds_the_same_targets(self):
        found = by_x(infer.OnnxDetector(EXPORTS / "detector_small_int8.onnx")(self.rgb, self.fixed, 0.3))
        self.assertEqual(len(found), len(self.expected))
        np.testing.assert_allclose(found[:, :2], self.expected[:, :2], atol=1.0)

    @staticmethod
    def get(request):
        with urllib.request.urlopen(request) as answer:
            return json.load(answer)

    def test_http_api(self):
        import serve
        serve.Handler.detector = infer.OnnxDetector(EXPORTS / "detector_small_fp32.onnx", 2)
        serve.Handler.model = "detector_small_fp32.onnx"
        server = ThreadingHTTPServer(("127.0.0.1", 0), serve.Handler)
        threading.Thread(target=server.serve_forever, daemon=True).start()
        base = f"http://127.0.0.1:{server.server_address[1]}"
        try:
            self.assertEqual(self.get(base + "/health")["model"], "detector_small_fp32.onnx")
            body = self.rgb.tobytes() + self.fixed.tobytes()
            answer = self.get(urllib.request.Request(base + "/detect?w=1280&h=720&fixed=1", body))
            found = np.array(sorted(answer["detections"]), np.float32)
            np.testing.assert_allclose(found[:, :2], self.expected[:, :2], atol=0.05)
            with self.assertRaises(urllib.error.HTTPError) as caught:
                self.get(urllib.request.Request(base + "/detect?w=1280&h=720", body[:100]))
            self.assertEqual(caught.exception.code, HTTP_BAD_REQUEST)
            caught.exception.close()
        finally:
            server.shutdown()


if __name__ == "__main__":
    unittest.main(verbosity=2)
