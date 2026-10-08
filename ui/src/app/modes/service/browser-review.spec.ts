import { BrowserDevice, RunPart } from '../wasm/review-messages';
import { partsDevices } from './browser-review';

/** A run's part whose detector ran on `device`. */
const part = (device: BrowserDevice): RunPart => ({
  setup: '',
  track: '',
  watch: '',
  fixed: new Uint8Array(),
  device,
});

describe('where a browser review ran', () => {
  it('names the one device every part ran on', () => {
    expect(partsDevices([part('webgpu'), part('webgpu')])).toBe('WebGPU');
  });

  it('names both devices when a part fell back to the CPU', () => {
    expect(partsDevices([part('webgpu'), part('wasm')])).toBe('WebGPU and WebAssembly');
  });
});
