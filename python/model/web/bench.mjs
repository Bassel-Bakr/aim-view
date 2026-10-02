// Browser benchmark of the detector (deployment path C): every model file on WebGPU and on WASM, on a real frame.
// Times are per whole frame: converting the bytes to the model's input plus the model.
// Served by serve_static.py, which sends the cross-origin isolation headers that multi-threaded WASM needs.
import * as ort from "./node_modules/onnxruntime-web/dist/ort.all.min.mjs";

const W = 1280, H = 720, THR = 0.3;
const MODELS = ["detector_tiny_fp32", "detector_small_fp32", "detector_small_v2_fp32", "detector_small_v2_fp16",
  "detector_small_v2_u8in", "detector_small_v2_embed", "detector_small_v2_int8", "detector_full_fp32"];
ort.env.wasm.wasmPaths = new URL("./node_modules/onnxruntime-web/dist/", location.href).href;
ort.env.wasm.numThreads = self.crossOriginIsolated ? Math.min(8, navigator.hardwareConcurrency) : 1;

const status = (t) => (document.getElementById("status").textContent = t);
const results = (window.results = { webgpu: !!navigator.gpu, isolated: self.crossOriginIsolated,
  threads: ort.env.wasm.numThreads, runs: [] });

async function bytes(url) {
  const r = await fetch(url);
  if (!r.ok) throw new Error(`${url}: ${r.status}`);
  return new Uint8Array(await r.arrayBuffer());
}

function input(rgb, fixed) {
  const x = new Float32Array(4 * W * H);
  for (let i = 0, n = W * H; i < n; i++) {
    x[i] = rgb[3 * i] / 255;
    x[n + i] = rgb[3 * i + 1] / 255;
    x[2 * n + i] = rgb[3 * i + 2] / 255;
    x[3 * n + i] = fixed[i];
  }
  return new ort.Tensor("float32", x, [1, 4, H, W]);
}

function decode(score, reg) {
  const gw = W / 4, gh = H / 4, n = gw * gh, out = [];
  for (let i = 0; i < n; i++) {
    const s = score.data[i];
    if (s > THR) {
      const x = i % gw, y = (i / gw) | 0;
      out.push([(x + reg.data[i]) * 4, (y + reg.data[n + i]) * 4, Math.exp(reg.data[2 * n + i]),
        Math.exp(reg.data[3 * n + i]), s]);
    }
  }
  return out;
}

function fromDets(t) {
  // an _embed model's output: the best boxes, (1, K, 5) cx, cy, w, h, score
  const out = [];
  for (let i = 0; i < t.dims[1]; i++) {
    const r = t.data.subarray(5 * i, 5 * i + 5);
    if (r[4] > THR) out.push(Array.from(r));
  }
  return out;
}

function draw(rgb, dets) {
  const c = document.getElementById("view").getContext("2d");
  const img = c.createImageData(W, H);
  for (let i = 0; i < W * H; i++) {
    img.data[4 * i] = rgb[3 * i]; img.data[4 * i + 1] = rgb[3 * i + 1];
    img.data[4 * i + 2] = rgb[3 * i + 2]; img.data[4 * i + 3] = 255;
  }
  c.putImageData(img, 0, 0);
  c.strokeStyle = "#fab219";
  c.lineWidth = 2;
  for (const [x, y, w, h] of dets) c.strokeRect(x - w / 2 - 3, y - h / 2 - 3, w + 6, h + 6);
}

async function run() {
  const rgb = await bytes("../../../test_out/vod_model/bench_frame_rgb.bin");
  const fixed = await bytes("../../../test_out/vod_model/bench_frame_fixed.bin");
  const tbody = document.querySelector("#table tbody");
  const backends = navigator.gpu ? ["webgpu", "wasm"] : ["wasm"];
  for (const name of MODELS) {
    for (const ep of backends) {
      status(`${name} on ${ep}…`);
      const row = { model: name, backend: ep };
      try {
        const t0 = performance.now();
        const sess = await ort.InferenceSession.create(`../exports/${name}.onnx`, { executionProviders: [ep] });
        row.load_ms = +(performance.now() - t0).toFixed(1);
        // a whole frame: the float models need the bytes converted in JavaScript first, the _u8in model takes them as
        // they are
        const u8 = name.endsWith("_u8in") || name.endsWith("_embed");
        const feeds = () => (u8 ? { rgb: new ort.Tensor("uint8", rgb, [1, H, W, 3]),
          fixed: new ort.Tensor("uint8", fixed, [1, H, W]) } : { x: input(rgb, fixed) });
        let out;
        for (let i = 0; i < 3; i++) out = await sess.run(feeds());
        const times = [], prep = [];
        for (let i = 0; i < 20; i++) {
          const t = performance.now();
          const f = feeds();
          prep.push(performance.now() - t);
          out = await sess.run(f);
          times.push(performance.now() - t);
        }
        times.sort((a, b) => a - b);
        prep.sort((a, b) => a - b);
        row.median_ms = +times[10].toFixed(2);
        row.p90_ms = +times[18].toFixed(2);
        row.prep_ms = +prep[10].toFixed(2);
        const dets = out.dets ? fromDets(out.dets) : decode(out.score, out.reg);
        row.targets = dets.length;
        row.detections = dets.map((d) => d.map((v) => +v.toFixed(2)));
        if (name === "detector_small_v2_u8in") draw(rgb, dets);
        await sess.release();
      } catch (e) {
        row.error = String(e).slice(0, 200);
      }
      results.runs.push(row);
      tbody.insertAdjacentHTML("beforeend", `<tr><td>${name}</td><td>${ep}</td><td>${row.load_ms ?? "–"} ms</td>
        <td>${row.median_ms ?? row.error ?? "–"} ms</td><td>${row.p90_ms ?? "–"} ms</td><td>${row.prep_ms ?? "–"} ms</td><td>${row.targets ?? "–"}</td></tr>`);
    }
  }
  results.done = true;
  status(`Done. WebGPU ${results.webgpu ? "available" : "not available"}; WASM threads ${results.threads}.`);
}

run().catch((e) => { status(`Error: ${e}`); results.error = String(e); results.done = true; });
