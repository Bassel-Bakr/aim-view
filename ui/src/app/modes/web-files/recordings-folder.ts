import { inject, Injectable, signal } from '@angular/core';
import { BrowserStore } from './browser-store';
import { ItemCount } from '../../platform/recording-source';
import { isVideo, isVideoName } from './video-files';

const KEY = 'recordings-folder';
/** How far below the folder videos are looked for (KovOBS keeps one folder per scenario). */
const DEPTH = 3;

/** A video in the recordings folder: its path below the folder, and the file. */
export interface FolderEntry {
  path: string;
  file: File;
}

/**
 * The recordings folder as the user is told about it: its name, whether the browser needs leave to read it again,
 * whether it is being read and how many of its videos are read, why it could not be opened, and the name of a
 * remembered folder that could not be found.
 */
export interface RecordingsFolderState {
  name: string | null;
  ask: boolean;
  busy: boolean;
  count: ItemCount | null;
  refused: string | null;
  gone: string | null;
}

const NONE: RecordingsFolderState = {
  name: null,
  ask: false,
  busy: false,
  count: null,
  refused: null,
  gone: null,
};

/** Whether an error says a file or folder is no longer there (moved, deleted, or on a drive not connected). */
const isGone = (e: unknown) => e instanceof DOMException && e.name === 'NotFoundError';

/** A video file below the folder, not read yet: its path and its handle. */
export interface VideoHandle {
  path: string;
  handle: FileSystemFileHandle;
}

/** The video files below a folder, a few levels down, by name. */
async function videoHandles(
  dir: FileSystemDirectoryHandle,
  prefix = '',
  depth = DEPTH,
): Promise<VideoHandle[]> {
  const out: VideoHandle[] = [];
  for await (const [name, entry] of dir.entries()) {
    const path = prefix ? `${prefix}/${name}` : name;
    if (entry.kind === 'directory') {
      if (depth > 1)
        out.push(...(await videoHandles(entry as FileSystemDirectoryHandle, path, depth - 1)));
    } else if (isVideoName(name)) {
      out.push({ path, handle: entry as FileSystemFileHandle });
    }
  }
  return out;
}

/**
 * The videos below a folder, a few levels down: listed first, then read one by one (counted). A video gone since it
 * was listed is left out.
 */
export async function folderVideos(
  dir: FileSystemDirectoryHandle,
  counted: (count: ItemCount) => void = () => undefined,
): Promise<FolderEntry[]> {
  const handles = await videoHandles(dir);
  const out: FolderEntry[] = [];
  for (const [k, v] of handles.entries()) {
    try {
      out.push({ path: v.path, file: await v.handle.getFile() });
    } catch (e) {
      if (!isGone(e)) throw e;
    }
    counted({ done: k + 1, total: handles.length });
  }
  return out;
}

/** The videos among the files of a folder chosen as files (a folder input), by their paths below it. */
export function chosenVideos(files: readonly File[]): FolderEntry[] {
  return files.filter(isVideo).map((file) => ({
    path: (file.webkitRelativePath || file.name).split('/').slice(1).join('/') || file.name,
    file,
  }));
}

/**
 * The folder of recordings the user opens in the browser (KovOBS's, say), remembered across visits: the browser asks
 * once a visit before reading it again. Where the browser has no folder picker, the folder is chosen as files instead,
 * for that visit only. A remembered folder that cannot be found is said so and stays remembered: it may be on a drive
 * that is not connected.
 */
@Injectable({ providedIn: 'root' })
export class RecordingsFolder {
  private readonly store = inject(BrowserStore);
  private handle: FileSystemDirectoryHandle | null = null;
  readonly state = signal<RecordingsFolderState>(NONE);
  /** Whether this browser has the folder picker (Chromium does; others choose a folder as files). */
  readonly picker =
    typeof window !== 'undefined' && typeof window.showDirectoryPicker === 'function';

  /** The folder from a visit before: its videos when the browser still lets it be read, else null. */
  async restore(): Promise<FolderEntry[] | null> {
    const handle = await this.store.get<FileSystemDirectoryHandle>(KEY).catch(() => undefined);
    if (!handle) return null;
    this.handle = handle;
    const leave = await handle
      .queryPermission({ mode: 'read' })
      .catch(() => 'prompt' as PermissionState);
    if (leave === 'granted') return this.read();
    this.patch({ name: handle.name, ask: true });
    return null;
  }

  /** The user picks the folder (in a click); null when it was not opened. */
  async open(): Promise<FolderEntry[] | null> {
    const pick = window.showDirectoryPicker;
    if (!pick) {
      this.patch({ refused: 'This browser has no folder picker' });
      return null;
    }
    try {
      this.handle = await pick.call(window, { id: 'recordings', mode: 'read' });
    } catch (e) {
      const closed = e instanceof DOMException && e.name === 'AbortError';
      this.patch({ refused: closed ? null : String(e) });
      return null;
    }
    await this.store.set(KEY, this.handle).catch(() => undefined);
    return this.read();
  }

  /** The user gives leave to read the remembered folder again (in a click). */
  async allow(): Promise<FolderEntry[] | null> {
    const handle = this.handle;
    if (!handle) return null;
    try {
      const leave = await handle.requestPermission({ mode: 'read' });
      return leave === 'granted' ? this.read() : null;
    } catch (e) {
      if (!isGone(e)) throw e;
      this.patch({ name: null, ask: false, gone: handle.name });
      return null;
    }
  }

  /** Forgets the folder: it is not read again on the next visit. */
  async forget(): Promise<void> {
    this.handle = null;
    this.state.set(NONE);
    await this.store.remove(KEY).catch(() => undefined);
  }

  /** The folder chosen as files (a folder input): read for this visit only. */
  chosen(files: readonly File[]): FolderEntry[] {
    const name = files[0]?.webkitRelativePath.split('/')[0] || null;
    this.patch({ name, ask: false, refused: null, gone: null });
    return chosenVideos(files);
  }

  /** The folder's videos; null when it cannot be read (the state says why). */
  private async read(): Promise<FolderEntry[] | null> {
    const handle = this.handle as FileSystemDirectoryHandle;
    this.patch({
      name: handle.name,
      ask: false,
      busy: true,
      count: null,
      refused: null,
      gone: null,
    });
    try {
      return await folderVideos(handle, (count) => this.patch({ count }));
    } catch (e) {
      if (isGone(e)) this.patch({ name: null, gone: handle.name });
      else this.patch({ name: null, refused: e instanceof Error ? e.message : String(e) });
      return null;
    } finally {
      this.patch({ busy: false, count: null });
    }
  }

  private patch(change: Partial<RecordingsFolderState>): void {
    this.state.update((s) => ({ ...s, ...change }));
  }
}
