/// <reference lib="webworker" />
// The area finder (src/areas.rs) in a worker of its own, after a review or without one (Find areas on a recording not
// reviewed yet): it reads every key frame, or, when the recording has few, the frames areas_sample picks over it, each
// decoded and converted as the review's frames are (video-frames.ts, frame-converter.ts), so they are ffmpeg's pixels.
// KovaaK's session box comes from the key frames' Y planes, as the review's HUD watch finds it (src/hud.rs).
import { FinderReply, FinderWork } from './area-finder-messages';
import { Core } from './core';
import { FrameConverter } from './frame-converter';
import { FrameFormat } from './review-messages';
import { VideoFrames } from './video-frames';

const W = 1280;
const H = 720;

const say = (m: FinderReply) => postMessage(m);

addEventListener('message', (e: MessageEvent<FinderWork>) => {
  find(e.data).catch((err: unknown) =>
    say({ kind: 'error', error: err instanceof Error ? err.message : String(err) }),
  );
});

/**
 * The frames the area finder reads (src/areas.rs: sample_frames): null for every key frame, when the recording has
 * enough; else the indexes of frames spread over it.
 */
function finderPicks(core: Core, keys: number, times: number[], duration: number): number[] | null {
  const request = JSON.stringify({ keys, times, duration });
  const answer: unknown = JSON.parse(
    core.takeText(core.textIn(request, (p, n) => core.x.areas_sample(p, n))),
  );
  return Array.isArray(answer) ? (answer as number[]) : null;
}

async function find(work: FinderWork): Promise<void> {
  const video = await VideoFrames.open(work.file, 'any');
  const { times, keys } = await video.frameTimes();
  const core = await Core.load(work.coreUrl);
  const picks = finderPicks(core, keys.length, times, await video.input.computeDuration());
  const frames = new FrameConverter(core);
  const finder = core.x.areas_new();
  const yuv720 = core.reserve((W * H * 3) / 2);
  // the HUD watch, for the session box: made for the first key frame's size, it reads each key frame's Y plane
  let hud = 0;
  for await (const s of video.keySamples()) {
    const block = await frames.write(s);
    // the format is the first frame's, once one is written
    const { width, height, full } = frames.format as FrameFormat;
    hud ||= core.x.hud_new(width, height, Number(full));
    core.x.hud_add_key(hud, block.ptr, width * height);
    if (picks) continue;
    frames.yuv720(block, yuv720);
    core.x.areas_add(finder, yuv720.ptr);
  }
  if (!hud) throw new Error('The video has no frames');
  // the frames picked, in order: each decoded once, from the key frame before it on (a frame picked twice is read twice)
  for await (const s of video.samples.samplesAtTimestamps((picks ?? []).map((i) => times[i]))) {
    if (!s) continue;
    frames.yuv720(await frames.write(s), yuv720);
    core.x.areas_add(finder, yuv720.ptr);
  }
  frames.free();
  const session = core.takeText(core.x.hud_session_box(hud));
  const found = core.takeText(core.textIn(session, (p, n) => core.x.areas_finish(finder, p, n)));
  say({ kind: 'found', found });
}
