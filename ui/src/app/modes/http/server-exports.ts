/**
 * The `Exports` of every mode: recordings shared as one zip, and one opened. In: the review
 * service's /api/export (each recording's files) and /api/import, and RecordingSource (the videos,
 * and adding an opened recording's video and stats file as uploads). Out: the zip the user saves,
 * and the recordings an opened zip adds.
 */

import { HttpClient } from '@angular/common/http';
import { inject, Service } from '@angular/core';
import { firstValueFrom } from 'rxjs';
import { Exports, OpenedExport, ShareProgress } from '../../platform/exports';
import { MODE_NAME } from '../../platform/mode';
import { RecordingSource } from '../../platform/recording-source';
import { encodeBatch, readBatch } from '../web-files/file-batch';
import { readZip, ZipItem } from '../web-files/zip-read';
import { ZipSink, ZipWriter } from '../web-files/zip-stream';

/** The export's layout this page writes and opens (service/src/library/export.rs: FORMAT). */
const FORMAT = 1;
/** The manifest's file in the zip. */
const MANIFEST = 'manifest.json';
/** The name the save dialog suggests. */
const ZIP_NAME = 'aim-view-export.zip';
/** Milliseconds in a second: the service's times are seconds. */
const MS_PER_SECOND = 1000;

/** A recording in the manifest (service/src/library/export.rs `export_recording`). */
export interface ExportedRecording {
  /** Its id where it was exported. */
  id: string;
  /** Its folder in the zip. */
  folder: string;
  /** Its video's file name. */
  video: string;
  /** Its scenario's name. */
  scenario: string;
  /** The scenario's facts, when they were known. */
  facts: unknown;
  /** Its stats file's name; null without one. */
  stats: string | null;
  /** The models whose reviews it carries. */
  reviews: string[];
  /** Whether its video is in the zip (the page sets it). */
  video_included?: boolean;
}

/** The manifest: the format, the app's version, when and where it was made, and the recordings. */
export interface ExportManifest {
  /** The layout's version. */
  format: number;
  /** The app's version that made it. */
  app: string;
  /** When it was made (yyyy.mm.dd-hh.mm.ss). */
  made: string;
  /** The mode that made it. */
  mode: string;
  /** The recordings, in order. */
  recordings: ExportedRecording[];
}

/** A sink that keeps the zip's parts, then downloads them as one file (no save dialog). */
class DownloadSink implements ZipSink {
  /** The parts written. */
  private readonly parts: Uint8Array[] = [];

  /** Keeps a copy of the part. */
  async write(chunk: Uint8Array): Promise<void> {
    this.parts.push(chunk.slice());
  }

  /** Downloads the parts as one zip. */
  async close(): Promise<void> {
    const url = URL.createObjectURL(
      new Blob(this.parts as BlobPart[], { type: 'application/zip' }),
    );
    const link = document.createElement('a');
    link.href = url;
    link.download = ZIP_NAME;
    link.click();
    setTimeout(() => URL.revokeObjectURL(url));
  }
}

/** The file the user picked in the save dialog, written as the zip goes. */
class FileSink implements ZipSink {
  /** The open file. */
  constructor(private readonly file: FileSystemWritableFileStream) {}

  /** Writes the part. */
  write(chunk: Uint8Array): Promise<void> {
    return this.file.write(chunk as BufferSource);
  }

  /** Closes the file. */
  close(): Promise<void> {
    return this.file.close();
  }
}

/** Where the zip goes: the file the user picks, or a download where the browser has no save dialog. */
async function zipSink(): Promise<ZipSink> {
  if (!window.showSaveFilePicker) return new DownloadSink();
  const handle = await window.showSaveFilePicker({
    suggestedName: ZIP_NAME,
    types: [{ description: 'Aim View export', accept: { 'application/zip': ['.zip'] } }],
  });
  return new FileSink(await handle.createWritable());
}

/** A zip entry's bytes. */
async function bytesOf(item: ZipItem): Promise<Uint8Array<ArrayBuffer>> {
  return new Uint8Array(await (await item.blob()).arrayBuffer());
}

/** Recordings shared as one zip through the review service (see the module's comment). */
@Service()
export class ServerExports implements Exports {
  /** Asks the service for an export's files and gives it an opened one's. */
  private readonly http = inject(HttpClient);
  /** The videos, and adding an opened recording. */
  private readonly source = inject(RecordingSource);
  /** The mode, for the manifest. */
  private readonly mode = inject(MODE_NAME);

  /** The zip: the manifest, each recording's files, then its video when asked. */
  async export(ids: readonly string[], videos: boolean, progress: ShareProgress): Promise<void> {
    const sink = await zipSink();
    const asked = this.http.post(
      '/api/export',
      { ids, mode: this.mode },
      { responseType: 'arraybuffer' },
    );
    const files = readBatch(new Uint8Array(await firstValueFrom(asked)));
    const manifestFile = files.find((file) => file.path === MANIFEST);
    if (!manifestFile) throw new Error('The service gave no manifest');
    const manifest = JSON.parse(new TextDecoder().decode(manifestFile.bytes)) as ExportManifest;
    for (const recording of manifest.recordings) recording.video_included = videos;
    const zip = new ZipWriter(sink);
    const text = new TextEncoder().encode(JSON.stringify(manifest, null, 2));
    await zip.addBytes(MANIFEST, text, Date.now());
    for (const file of files.filter((entry) => entry !== manifestFile))
      await zip.addBytes(file.path, file.bytes, file.modified * MS_PER_SECOND);
    const count = manifest.recordings.length;
    for (const [index, recording] of videos ? manifest.recordings.entries() : []) {
      progress(index / count);
      const video = await this.source.videoFile(recording.id);
      let sent = 0;
      await zip.addStream(
        `${recording.folder}/video/${recording.video}`,
        video.stream(),
        Date.now(),
        (bytes) => {
          sent += bytes;
          progress((index + sent / Math.max(video.size, 1)) / count);
        },
      );
    }
    await zip.finish();
    progress(1);
  }

  /** Each recording of the zip with its video: added as an upload with its stats file, then its reviews and marks. */
  async open(zip: File, progress: ShareProgress): Promise<OpenedExport> {
    const items = await readZip(zip);
    const byPath = new Map(items.map((item) => [item.path, item]));
    const manifestItem = byPath.get(MANIFEST);
    if (!manifestItem)
      throw new Error('This zip is not an Aim View export: it has no manifest.json');
    const manifest = JSON.parse(await (await manifestItem.blob()).text()) as ExportManifest;
    if (manifest.format !== FORMAT)
      throw new Error(`This export's format (${manifest.format}) is not one this app opens`);
    const opened: OpenedExport = { added: [], skipped: [] };
    for (const [index, recording] of manifest.recordings.entries()) {
      progress(index / manifest.recordings.length);
      const video = byPath.get(`${recording.folder}/video/${recording.video}`);
      if (!video) {
        opened.skipped.push(`${recording.video}: its video is not in the export`);
        continue;
      }
      opened.added.push(await this.openRecording(recording, video, items));
    }
    progress(1);
    return opened;
  }

  /** One recording: its video and stats file added, then its reviews, marks and facts sent; gives its new id. */
  private async openRecording(
    recording: ExportedRecording,
    video: ZipItem,
    items: ZipItem[],
  ): Promise<string> {
    const files = [new File([await video.blob()], recording.video)];
    const stats =
      recording.stats &&
      items.find((item) => item.path === `${recording.folder}/stats/${recording.stats}`);
    if (stats && recording.stats) files.push(new File([await stats.blob()], recording.stats));
    const added = await this.source.add(files);
    const id = added.ids[0];
    if (!id) throw new Error(`${recording.video} could not be added`);
    const prefix = `${recording.folder}/`;
    const kept = items.filter(
      (item) =>
        item.path.startsWith(`${prefix}reviews/`) || item.path.startsWith(`${prefix}marks/`),
    );
    const batch = await Promise.all(
      kept.map(async (item) => ({
        path: item.path.slice(prefix.length),
        file: new File([await bytesOf(item)], item.path),
      })),
    );
    if (recording.facts) {
      const facts = JSON.stringify({ scenario: recording.scenario, facts: recording.facts });
      batch.push({ path: 'facts.json', file: new File([facts], 'facts.json') });
    }
    // a Blob is sent as its bytes in every mode (HttpClient would write a typed array as JSON)
    const body = new Blob([(await encodeBatch(batch)) as BlobPart]);
    await firstValueFrom(this.http.post('/api/import', body, { params: { id } }));
    return id;
  }
}
