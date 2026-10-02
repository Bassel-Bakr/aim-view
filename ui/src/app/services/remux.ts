import {
  ALL_FORMATS,
  BlobSource,
  Conversion,
  Input,
  Mp4OutputFormat,
  Output,
  StreamTarget,
  StreamTargetChunk,
} from 'mediabunny';

/**
 * Collects an MP4 writer's output into a Blob, so a large video is not held in one buffer. The writes come in order,
 * except the muxer's last one: it fills in the mdat box's size near the start. So the first chunk stays in memory,
 * open to that write, and every later one becomes a part of the Blob as it comes.
 */
export class BlobSink {
  private head: Uint8Array<ArrayBuffer> | null = null;
  private readonly parts: Blob[] = [];
  private end = 0;
  readonly stream = new WritableStream<StreamTargetChunk>({ write: (c) => this.write(c) });

  write({ data, position }: StreamTargetChunk): void {
    if (position === this.end) {
      if (this.head === null) this.head = data.slice();
      else this.parts.push(new Blob([data]));
      this.end += data.byteLength;
    } else if (this.head && position + data.byteLength <= this.head.byteLength) {
      this.head.set(data, position);
    } else {
      throw new Error(`a write at byte ${position} the remux cannot place`);
    }
  }

  blob(type: string): Blob {
    return new Blob(this.head ? [this.head, ...this.parts] : [], {
      type,
    });
  }
}

/**
 * A video remuxed into MP4 in the browser: its streams are copied, not encoded again (a stream MP4 cannot hold is
 * encoded, with the browser's own encoder). progress gets the share done, 0 to 1. Rejects when it cannot be done.
 */
export async function remuxToMp4(file: Blob, progress: (share: number) => void): Promise<Blob> {
  const sink = new BlobSink();
  const input = new Input({ formats: ALL_FORMATS, source: new BlobSource(file) });
  const output = new Output({
    format: new Mp4OutputFormat(),
    target: new StreamTarget(sink.stream, { chunked: true }),
  });
  const conversion = await Conversion.init({ input, output, showWarnings: false });
  if (!conversion.isValid) {
    const why = conversion.discardedTracks.map((t) => t.reason.replaceAll('_', ' '));
    throw new Error(`no track it can keep (${why.join(', ')})`);
  }
  conversion.onProgress = progress;
  await conversion.execute();
  return sink.blob('video/mp4');
}
