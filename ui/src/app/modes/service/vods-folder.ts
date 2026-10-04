import { HttpClient } from '@angular/common/http';
import { inject, Injectable, signal } from '@angular/core';
import { firstValueFrom } from 'rxjs';
import { errorMessage } from '../../api';
import { BrowserStore } from '../web-files/browser-store';
import { isVideo } from '../web-files/video-files';
import { ServiceHost } from './service-host';
import { ChosenFile, VodsMount } from './service-messages';

/** Where the VODs folder's handle is remembered (IndexedDB), as the browser mode has always kept it. */
const KEY = 'recordings-folder';
/** Where the service finds the VODs folder (the contract's /vods). */
const MOUNTED = '/vods';

/**
 * The VODs folder as the user is told about it: its name, whether the browser needs leave to read it again, whether
 * it is being opened, why it could not be opened, and the name of a remembered folder that could not be found.
 */
export interface VodsFolderState {
  name: string | null;
  ask: boolean;
  busy: boolean;
  refused: string | null;
  gone: string | null;
}

const NONE: VodsFolderState = { name: null, ask: false, busy: false, refused: null, gone: null };

/** Whether an error says a file or folder is no longer there (moved, deleted, or on a drive not connected). */
const isGone = (e: unknown) => e instanceof DOMException && e.name === 'NotFoundError';

/** The videos among a folder input's files, by their paths below the folder chosen. */
function chosenVideos(files: readonly File[]): ChosenFile[] {
  return files.filter(isVideo).map((file) => ({
    path: (file.webkitRelativePath || file.name).split('/').slice(1).join('/') || file.name,
    file,
  }));
}

/**
 * The folder of recordings the user opens (KovOBS's, one folder per scenario), remembered across visits: the browser
 * asks once a visit before it is read again. It is mounted in the review service at /vods, which lists its videos and
 * reads them where they are. Where the browser has no folder picker, the folder is chosen as files instead, for that
 * visit only. A remembered folder that cannot be found is said so and stays remembered: it may be on a drive that is
 * not connected.
 */
@Injectable({ providedIn: 'root' })
export class VodsFolder {
  private readonly store = inject(BrowserStore);
  private readonly host = inject(ServiceHost);
  private readonly http = inject(HttpClient);
  private handle: FileSystemDirectoryHandle | null = null;
  readonly state = signal<VodsFolderState>(NONE);
  /** Whether this browser has the folder picker (Chromium does; others choose a folder as files). */
  readonly picker =
    typeof window !== 'undefined' && typeof window.showDirectoryPicker === 'function';

  /** The folder from a visit before: mounted when the browser still lets it be read (true), else asks for leave. */
  async restore(): Promise<boolean> {
    const handle = await this.store.get<FileSystemDirectoryHandle>(KEY).catch(() => undefined);
    if (!handle) return false;
    this.handle = handle;
    const leave = await handle
      .queryPermission({ mode: 'read' })
      .catch(() => 'prompt' as PermissionState);
    if (leave === 'granted') return this.mount(handle, handle.name);
    this.patch({ name: handle.name, ask: true });
    return false;
  }

  /** The user picks the folder (in a click); true when it is mounted. */
  async open(): Promise<boolean> {
    const pick = window.showDirectoryPicker;
    if (!pick) {
      this.patch({ refused: 'This browser has no folder picker' });
      return false;
    }
    try {
      this.handle = await pick.call(window, { id: 'recordings', mode: 'read' });
    } catch (e) {
      const closed = e instanceof DOMException && e.name === 'AbortError';
      this.patch({ refused: closed ? null : String(e) });
      return false;
    }
    await this.store.set(KEY, this.handle).catch(() => undefined);
    return this.mount(this.handle, this.handle.name);
  }

  /** The user gives leave to read the remembered folder again (in a click); true when it is mounted. */
  async allow(): Promise<boolean> {
    const handle = this.handle;
    if (!handle) return false;
    try {
      const leave = await handle.requestPermission({ mode: 'read' });
      return leave === 'granted' && this.mount(handle, handle.name);
    } catch (e) {
      if (!isGone(e)) throw e;
      this.patch({ name: null, ask: false, gone: handle.name });
      return false;
    }
  }

  /** The folder chosen as files (a folder input): mounted for this visit only. */
  chosen(files: readonly File[]): Promise<boolean> {
    const name = files[0]?.webkitRelativePath.split('/')[0] || null;
    return this.mount(chosenVideos(files), name);
  }

  /** Forgets the folder: it is unmounted, and not opened again on the next visit. */
  async forget(): Promise<void> {
    this.handle = null;
    this.state.set(NONE);
    await this.store.remove(KEY).catch(() => undefined);
    await this.host.mount(null).catch(() => undefined);
  }

  /**
   * Mounts the folder at /vods and tells the service it is the VODs folder; false when it cannot be read (the state
   * says why).
   */
  private async mount(vods: VodsMount, name: string | null): Promise<boolean> {
    this.patch({ name, ask: false, busy: true, refused: null, gone: null });
    try {
      if (!Array.isArray(vods)) await vods.values().next();
      await this.host.mount(vods);
      await firstValueFrom(this.http.post('/api/folder', null, { params: { path: MOUNTED } }));
      return true;
    } catch (e) {
      if (isGone(e)) this.patch({ name: null, gone: name });
      else this.patch({ name: null, refused: errorMessage(e) });
      return false;
    } finally {
      this.patch({ busy: false });
    }
  }

  private patch(change: Partial<VodsFolderState>): void {
    this.state.update((s) => ({ ...s, ...change }));
  }
}
