import { TestBed } from '@angular/core/testing';
import { Core, CoreExports } from './core';
import { CoreModule, JoinedReview } from './core-module';
import { RunPart } from './review-messages';

/** What the stand-in core was given: the review's setup, each run's parts in order, and the detector's name. */
interface CoreCalls {
  setup: string;
  parts: string[][];
  detector: string;
}

/**
 * The core's join exports in JavaScript, over a memory of its own: it keeps what it was given and answers `answer`
 * (the join itself is the core's: src/session.rs, `Joining`).
 */
function standInCore(answer: string, calls: CoreCalls): Core {
  const memory = new WebAssembly.Memory({ initial: 2 });
  let next = 8;
  const alloc = (len: number) => {
    const at = next;
    next += (len + 7) & ~7;
    return at;
  };
  const read = (ptr: number, len: number) =>
    new TextDecoder().decode(new Uint8Array(memory.buffer, ptr, len));
  const out = (text: string) => {
    const bytes = new TextEncoder().encode(text);
    const at = alloc(4 + bytes.length);
    new DataView(memory.buffer).setUint32(at, bytes.length, true);
    new Uint8Array(memory.buffer, at + 4, bytes.length).set(bytes);
    return at;
  };
  const exports: Partial<CoreExports> = {
    memory,
    alloc,
    dealloc: () => undefined,
    review_new: (ptr, len) => {
      calls.setup = read(ptr, len);
      return 1;
    },
    review_free: () => undefined,
    review_joining: () => 2,
    joining_add: (_j, track, trackLen, watch, watchLen) => {
      calls.parts.push([read(track, trackLen), read(watch, watchLen)]);
      return 1;
    },
    joining_finish: (_j, ptr, len) => {
      calls.detector = read(ptr, len);
      return out(answer);
    },
  };
  return Object.assign(Object.create(Core.prototype) as Core, { x: exports as CoreExports });
}

/** A run's parts as a review worker gives them. */
function part(run: number): RunPart {
  return {
    setup: '{"runs":2}',
    track: `track ${run}`,
    watch: `watch ${run}`,
    fixed: new Uint8Array(4),
    device: 'webgpu',
  };
}

function joinWith(answer: string, calls: CoreCalls) {
  vi.spyOn(Core, 'load').mockResolvedValue(standInCore(answer, calls));
  return TestBed.inject(CoreModule).joinReview([part(0), part(1)], 'onnxruntime-web (WebGPU)');
}

const noCalls = (): CoreCalls => ({ setup: '', parts: [], detector: '' });

describe('CoreModule', () => {
  afterEach(() => vi.restoreAllMocks());

  it("hands the core the runs' parts in order and gives back its joined review", async () => {
    const calls = noCalls();
    const answer: JoinedReview = {
      tracks: { fps: 60, frames: [], fixed: 0.01, detector: 'onnxruntime-web (WebGPU)' },
      readings: { camera: [], countdown: [] },
      hud: null,
    };
    expect(await joinWith(JSON.stringify(answer), calls)).toEqual(answer);
    expect(calls.setup).toBe('{"runs":2}');
    expect(calls.parts).toEqual([
      ['track 0', 'watch 0'],
      ['track 1', 'watch 1'],
    ]);
    expect(calls.detector).toBe('onnxruntime-web (WebGPU)');
  });

  it('says why the core would not join the runs', async () => {
    const refused = JSON.stringify({ error: "the review's runs do not join up" });
    await expect(joinWith(refused, noCalls())).rejects.toThrow("the review's runs do not join up");
  });
});
