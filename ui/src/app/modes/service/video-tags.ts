/**
 * A video's container-level tags, read with Mediabunny in the service's worker, whatever the
 * container it reads (MP4, MOV, MKV, WebM, MPEG-TS...). In: the video's file, from the mounts'
 * host_fs call (mounts.ts). Out: the tags whose values are text, by key as the file writes them,
 * for the service (service/src/video_tags.rs), which matches a key in any case.
 */

/** A tag whose value is text: its key and its value. */
type TextTag = [key: string, value: string];

/**
 * The video's raw tags whose values are text (an MP4's mdta keys, a Matroska's SimpleTags...);
 * none for a file Mediabunny cannot read as a video.
 */
export async function videoTags(file: Blob): Promise<Record<string, string>> {
  const { ALL_FORMATS, BlobSource, Input } = await import('mediabunny');
  const input = new Input({ formats: ALL_FORMATS, source: new BlobSource(file) });
  try {
    const raw = (await input.getMetadataTags()).raw ?? {};
    const text = Object.entries(raw).filter((tag): tag is TextTag => typeof tag[1] === 'string');
    return Object.fromEntries(text);
  } catch {
    return {};
  } finally {
    input.dispose();
  }
}
