import { TestBed } from '@angular/core/testing';
import { AreaBox, TrackFrame } from '../../api';
import { Core, CoreExports } from './core';
import { CoreModule } from './core-module';
import { FrameFormat, HudReading, RunPart } from './review-messages';

/** A watch's part as the stand-in core writes it: its frames. */
interface WatchPart {
  frames: number;
}

/** A tracker's part as the stand-in core writes it: its frames, linked. */
interface TrackerPart {
  frames: TrackFrame[];
}

/** What the stand-in core was asked: the HUD watch's size and range, and the areas the tracker ignores. */
interface CoreCalls {
  hudNew: number[];
  areas?: number[];
  ends?: number[];
}

const FORMAT: FrameFormat = { width: 2560, height: 1440, matrix: 0, full: 0 };
const AREA: AreaBox = [0.75, 0, 1, 0.25, 'challenge_results'];

const READING: HudReading = {
  game: 'kovaak',
  kills: [1, 3],
  shots: [1, 2, 3],
  hits: [1, 3],
  final: { kills: 2, hits: 2, shots: 3 },
  checked: 1,
  points: null,
};

/**
 * The core's join exports in JavaScript, over a memory of its own: the tracker adds each part's frames, and the camera
 * and HUD watches join as src/hud.rs's do (a run but the last reads the next run's first frame, left out of the join).
 */
function standInCore(reading: HudReading | null, calls: CoreCalls): Core {
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
  const tracked: TrackFrame[] = [];
  const counts = [0, 0];
  const join = (watch: number, ptr: number, len: number) => {
    const part = JSON.parse(read(ptr, len)) as WatchPart;
    counts[watch] += part.frames - (counts[watch] > 0 ? 1 : 0);
    return counts[watch];
  };
  const exports: Partial<CoreExports> = {
    memory,
    alloc,
    dealloc: () => undefined,
    tracker_new_ends: (areas, ends, count) => {
      calls.areas = [...new Float64Array(memory.buffer, areas, 4 * count)];
      calls.ends = [...new Uint8Array(memory.buffer, ends, count)];
      return 1;
    },
    tracker_add_part: (_t, ptr, len) => {
      const part = JSON.parse(read(ptr, len)) as TrackerPart;
      tracked.push(...part.frames);
      return part.frames.length;
    },
    tracker_finish: () => out(JSON.stringify(tracked)),
    camera_new_areas: () => 2,
    camera_add_part: (_c, ptr, len) => join(0, ptr, len),
    camera_finish: () =>
      out(
        JSON.stringify({
          camera: tracked.map(() => null),
          countdown: tracked.map(() => false),
        }),
      ),
    hud_new: (w, h, full) => {
      calls.hudNew = [w, h, full];
      return 3;
    },
    hud_add_part: (_h, ptr, len) => join(1, ptr, len),
    hud_finish: () => out(JSON.stringify(reading)),
    review_version: () => 2,
  };
  return Object.assign(Object.create(Core.prototype) as Core, { x: exports as CoreExports });
}

/** A run's part: its tracked frames from `first` on, and its watches' frames. */
function part(first: number, frames: number, camera: number, hud: number): RunPart {
  const track: TrackerPart = {
    frames: Array.from({ length: frames }, (_, k) => ({ i: first + k, t: [] })),
  };
  const watch = (n: number): WatchPart => ({ frames: n });
  return {
    frames,
    track: JSON.stringify(track),
    camera: JSON.stringify(watch(camera)),
    hud: JSON.stringify(watch(hud)),
    fps: 60,
    fixed: new Uint8Array(4),
    format: FORMAT,
    device: 'webgpu',
    keyFrames: 2,
  };
}

function joinWith(reading: HudReading | null, calls: CoreCalls, parts: RunPart[]) {
  vi.spyOn(Core, 'load').mockResolvedValue(standInCore(reading, calls));
  return TestBed.inject(CoreModule).joinRuns(parts, 0, [AREA]);
}

describe('CoreModule', () => {
  afterEach(() => vi.restoreAllMocks());

  it("joins the runs' HUD parts beside the tracks and the camera's, and keeps what the HUD read", async () => {
    const calls: CoreCalls = { hudNew: [] };
    // the first run reads the second run's first frame too: 3 frames and 1 more
    const joined = await joinWith(READING, calls, [part(0, 3, 4, 4), part(3, 2, 2, 2)]);
    expect(joined.frames.map((f) => f.i)).toEqual([0, 1, 2, 3, 4]);
    expect(joined.readings.camera.length).toBe(5);
    expect(joined.hud).toEqual(READING);
    expect(joined.version).toBe(2);
    expect(calls.hudNew).toEqual([2560, 1440, 0]);
    // the joined tracker ignores the areas the runs ignored
    expect(calls.areas).toEqual(AREA.slice(0, 4));
    // and knows the challenge's end screen, which is left out only while it shows
    expect(calls.ends).toEqual([1]);
  });

  it('gives no HUD reading when the HUD could not be read', async () => {
    const joined = await joinWith(null, { hudNew: [] }, [part(0, 3, 3, 3)]);
    expect(joined.hud).toBeNull();
  });

  it("refuses runs whose HUD parts do not add up to the tracks' frames", async () => {
    const parts = [part(0, 3, 4, 4), part(3, 2, 2, 1)];
    await expect(joinWith(READING, { hudNew: [] }, parts)).rejects.toThrow(
      "The review's runs do not join up",
    );
  });
});
