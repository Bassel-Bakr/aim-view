import {
  ALL_FORMATS,
  BlobSource,
  EncodedPacketSink,
  Input,
  InputVideoTrack,
  VideoSample,
  VideoSampleSink,
  VideoSinkDecoderOptions,
} from 'mediabunny';

/**
 * The decoder a worker asks for. 'software': the browser's software decoder, where it has one for the video (the review,
 * which copies out every frame). 'any': the browser's choice, a hardware one where it has one (the area finder, which
 * copies out only the frames it reads: the frames it decodes on the way to them cost the CPU nothing). Both give the
 * same bytes. A recording with 11 key frames, the 90 frames picked over it: 6 s with the hardware decoder, 35 s with
 * the software one (both beside a review).
 */
export type DecoderChoice = 'software' | 'any';

/** Every frame's time from 0 on, in order, and the key frames' times. */
export interface FrameTimes {
  times: number[];
  keys: number[];
}

/**
 * The decoder to ask for. For 'software', the browser's software decoder, where it has one for the video. A hardware
 * decoder's frames are on the GPU, and copying each one back takes longer than the software decoder does, while the
 * detector waits for the GPU (av1 at 2560x1440: 50 frames a second with the hardware decoder, 76 with the software
 * one). Both give the same bytes. Chrome has no software decoder for HEVC: there, the hardware one.
 */
async function decoderOptions(
  track: InputVideoTrack,
  choice: DecoderChoice,
): Promise<VideoSinkDecoderOptions> {
  if (choice === 'any') return {};
  const config = await track.getDecoderConfig();
  if (!config) return {};
  const software = await VideoDecoder.isConfigSupported({
    ...config,
    hardwareAcceleration: 'prefer-software',
  }).catch(() => null);
  return software?.supported ? { hardwareAcceleration: 'prefer-software' } : {};
}

/**
 * A recording opened for decoding in a worker (Mediabunny, the browser's own decoder), as the review and the area
 * finder read it. Frames before time 0 are the edit list's pre-roll: ffmpeg drops them, so these do too.
 */
export class VideoFrames {
  private constructor(
    readonly input: Input,
    readonly track: InputVideoTrack,
    readonly samples: VideoSampleSink,
  ) {}

  static async open(file: Blob, decoder: DecoderChoice): Promise<VideoFrames> {
    const input = new Input({ formats: ALL_FORMATS, source: new BlobSource(file) });
    const track = await input.getPrimaryVideoTrack();
    if (!track) throw new Error('The file has no video');
    const options = await decoderOptions(track, decoder);
    return new VideoFrames(input, track, new VideoSampleSink(track, options));
  }

  /** The recording's frame times, from its packets alone (none decoded). */
  async frameTimes(): Promise<FrameTimes> {
    const packets = new EncodedPacketSink(this.track);
    const only = { metadataOnly: true };
    const times: number[] = [];
    const keys: number[] = [];
    for (
      let packet = await packets.getFirstPacket(only);
      packet;
      packet = await packets.getNextPacket(packet, only)
    ) {
      if (packet.timestamp < 0) continue;
      times.push(packet.timestamp);
      if (packet.type === 'key') keys.push(packet.timestamp);
    }
    times.sort((a, b) => a - b);
    return { times, keys };
  }

  /** Every key frame from time 0 on, decoded, in order (ffmpeg -skip_frame nokey). */
  async *keySamples(): AsyncGenerator<VideoSample> {
    const packets = new EncodedPacketSink(this.track);
    for (
      let packet = await packets.getFirstKeyPacket();
      packet;
      packet = await packets.getNextKeyPacket(packet)
    ) {
      if (packet.timestamp < 0) continue;
      const sample = await this.samples.getSample(packet.timestamp);
      if (sample) yield sample;
    }
  }
}
