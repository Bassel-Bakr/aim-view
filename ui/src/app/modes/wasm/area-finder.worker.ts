/// <reference lib="webworker" />
// The area finder (src/areas.rs) in a worker of its own, after a review or without one (Find areas on a recording not
// reviewed yet): it reads every key frame, or, when the recording has few, the frames areas_sample picks over it, each
// decoded and converted as the review's frames are (video-frames.ts, frame-converter.ts), so they are ffmpeg's pixels.
// KovaaK's session box comes from the key frames' Y planes, as the review's HUD watch finds it (src/hud.rs).
import { FinderReply, FinderWork } from './area-finder-messages';
import { Core, FRAME_YUV420_BYTES } from './core';
import { FrameConverter } from './frame-converter';
import { FrameFormat } from './review-messages';
import { VideoFrames } from './video-frames';

const say = (reply: FinderReply) => postMessage(reply);

addEventListener('message', (event: MessageEvent<FinderWork>) => {
  find(event.data).catch((error: unknown) =>
    say({ kind: 'error', error: error instanceof Error ? error.message : String(error) }),
  );
});

/**
 * The frames the area finder reads (src/areas.rs: sample_frames): null for every key frame, when the recording has
 * enough; else the indexes of frames spread over it.
 */
function finderPicks(core: Core, keys: number, times: number[], duration: number): number[] | null {
  const request = JSON.stringify({ keys, times, duration });
  const answer: unknown = JSON.parse(
    core.takeText(core.textIn(request, (ptr, len) => core.exports.areas_sample(ptr, len))),
  );
  return Array.isArray(answer) ? (answer as number[]) : null;
}

async function find(work: FinderWork): Promise<void> {
  const video = await VideoFrames.open(work.file, 'any');
  const { times, keys } = await video.frameTimes();
  const core = await Core.load(work.coreUrl);
  const picks = finderPicks(core, keys.length, times, await video.input.computeDuration());
  const frames = new FrameConverter(core);
  const finder = core.exports.areas_new();
  const yuv720 = core.reserve(FRAME_YUV420_BYTES);
  // the HUD watch, for the session box: made for the first key frame's size, it reads each key frame's Y plane
  let hud = 0;
  for await (const sample of video.keySamples()) {
    const block = await frames.write(sample);
    // the format is the first frame's, once one is written
    const { width, height, full } = frames.format as FrameFormat;
    hud ||= core.exports.hud_new(width, height, Number(full));
    core.exports.hud_add_key(hud, block.ptr, width * height);
    if (picks) continue;
    frames.yuv720(block, yuv720);
    core.exports.areas_add(finder, yuv720.ptr);
  }
  if (!hud) throw new Error('The video has no frames');
  // the frames picked, in order: each decoded once, from the key frame before it on (a frame picked twice is read twice)
  for await (const sample of video.samples.samplesAtTimestamps(
    (picks ?? []).map((i) => times[i]),
  )) {
    if (!sample) continue;
    frames.yuv720(await frames.write(sample), yuv720);
    core.exports.areas_add(finder, yuv720.ptr);
  }
  frames.free();
  const session = core.takeText(core.exports.hud_session_box(hud));
  const found = core.takeText(
    core.textIn(session, (ptr, len) => core.exports.areas_finish(finder, ptr, len)),
  );
  say({ kind: 'found', found });
}
