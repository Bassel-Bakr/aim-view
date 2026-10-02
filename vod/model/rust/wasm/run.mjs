// Runs the wasm build under Node (or Bun) and times it: node run.mjs [model.onnx] [wasm]
import fs from "node:fs";
const model = process.argv[2] ?? "../../exports/detector_small_fp32.onnx";
const wasmPath = process.argv[3] ?? "target/wasm32-unknown-unknown/release/kovobs_detect_wasm.wasm";
const data = "D:/Projects/flowfix/test_out/vod_model";
const H = 720, W = 1280;
const noop = () => {};
const mod = new WebAssembly.Module(fs.readFileSync(wasmPath));
const imports = {};
for (const i of WebAssembly.Module.imports(mod)) (imports[i.module] ??= {})[i.name] = noop;
const inst = new WebAssembly.Instance(mod, imports);
const e = inst.exports;
const alloc = (n) => e.__wbindgen_malloc(n, 16);
const onnx = fs.readFileSync(model);
const rgb = fs.readFileSync(`${data}/bench_frame_rgb.bin`), fixed = fs.readFileSync(`${data}/bench_frame_fixed.bin`);
const x = new Float32Array(4 * H * W);
for (let p = 0; p < H * W; p++) { for (let c = 0; c < 3; c++) x[c * H * W + p] = rgb[p * 3 + c] / 255; x[3 * H * W + p] = fixed[p]; }
const mp = alloc(onnx.length), xp = alloc(x.byteLength), sp = alloc((H / 4) * (W / 4) * 4);
new Uint8Array(e.memory.buffer, mp, onnx.length).set(onnx);
new Float32Array(e.memory.buffer, xp, x.length).set(x);
const times = [];
let rc = 0;
for (let i = 0; i < 4; i++) {            // each call loads, optimizes and runs: the C function is one-shot
  const t = performance.now();
  rc = e.run_model(mp, onnx.length, xp, H, W, sp);
  times.push(performance.now() - t);
}
console.log("rc", rc, "ms per call (load + optimize + run):", times.map((t) => t.toFixed(0)).join(", "));
const s = new Float32Array(e.memory.buffer, sp, (H / 4) * (W / 4));
const hits = [];
for (let k = 0; k < s.length; k++) if (s[k] > 0.3) hits.push([(k % (W / 4)) * 4, Math.floor(k / (W / 4)) * 4, +s[k].toFixed(3)]);
console.log("cells above 0.3 (x, y of cell, score):", JSON.stringify(hits));
console.log("wasm memory MB:", e.memory.buffer.byteLength / 1048576);
