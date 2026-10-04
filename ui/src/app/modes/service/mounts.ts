/// <reference lib="webworker" />
import { ChosenFile, CopyDone, DirEntry } from './service-messages';
import { FsResult } from './service-module';

/** The codes of the host's file system calls (the contract's host_fs). */
const NOT_FOUND = 1;
const EXISTS = 2;
const OTHER = 3;
/** How much of a file is written at once. */
const CHUNK = 8 << 20;
/** The file a copy into a folder keeps what it copied in: each file's size and time, by its path below the folder. */
const COPIED = 'copied.json';
/** How many files a copy writes in one turn of the worker's queue. */
const COPY_STEP = 100;
/** A copy keeps its index every this many turns, so a page closed half way keeps what it copied. */
const INDEX_EVERY = 50;

/** The mounts the service's paths start with. */
export type MountName = 'data' | 'kovaak' | 'vods' | 'models';

/** A file system call that failed: its code, and why. */
export class FsError extends Error {
  constructor(
    readonly code: number,
    message: string,
  ) {
    super(message);
  }
}

/** What metadata says of a path: a folder or a file, its size, when it changed (seconds since 1970). */
export interface FsStat {
  dir: boolean;
  len: number;
  modified: number;
}

/** Hears how far a write or a copy is: done of total (bytes, or files). */
export type Progress = (done: number, total: number) => void;

/** A copied file as the copy's index keeps it: its size and time. */
type CopiedFile = [size: number, modified: number];

/** The copy's index: each copied file by its path below the folder. */
type CopiedIndex = Record<string, CopiedFile>;

/** A handle that can move itself (the browser's OPFS; not every browser has it). */
interface MovableHandle {
  move(parent: FileSystemDirectoryHandle, name: string): Promise<void>;
}

/** A file system under one mount: the names below the mount stand for a path. */
interface MountFs {
  readonly writable: boolean;
  file(names: string[]): Promise<File>;
  stat(names: string[]): Promise<FsStat>;
  list(names: string[], limit: number): Promise<DirEntry[]>;
  write(names: string[], data: Blob | Uint8Array, progress?: Progress): Promise<void>;
  mkdirs(names: string[]): Promise<void>;
  removeFile(names: string[]): Promise<void>;
  removeDir(names: string[], all: boolean): Promise<void>;
  rename(from: string[], to: string[]): Promise<void>;
}

const notFound = (path: string) => new FsError(NOT_FOUND, `${path}: not found`);
const readOnly = () => new FsError(OTHER, 'read-only');

/** A browser error as the call's code: not there, not empty, or other (with its message). */
function asFsError(e: unknown, path: string): FsError {
  if (e instanceof FsError) return e;
  const name = e instanceof DOMException ? e.name : '';
  if (name === 'NotFoundError') return notFound(path);
  if (name === 'InvalidModificationError') return new FsError(EXISTS, `${path}: not empty`);
  if (name === 'TypeMismatchError')
    return new FsError(OTHER, `${path}: a file where a folder is, or the other way`);
  return new FsError(OTHER, `${path}: ${e instanceof Error ? e.message : String(e)}`);
}

/** Writes data into a file, replacing it, a chunk at a time (a sync access handle: the worker's own). */
async function writeHandle(
  handle: FileSystemFileHandle,
  data: Blob | Uint8Array,
  progress?: Progress,
): Promise<void> {
  const out = await handle.createSyncAccessHandle();
  try {
    out.truncate(0);
    if (data instanceof Uint8Array) {
      out.write(data, { at: 0 });
    } else {
      for (let at = 0; at < data.size; at += CHUNK) {
        const chunk = new Uint8Array(await data.slice(at, at + CHUNK).arrayBuffer());
        out.write(chunk, { at });
        progress?.(Math.min(at + CHUNK, data.size), data.size);
      }
    }
    out.flush();
  } finally {
    out.close();
  }
}

/** A folder handle's tree: the browser's private file system (writable), or the VODs folder the user opened. */
export class DirMount implements MountFs {
  /** Folder handles by their path below the mount, found once. */
  private readonly dirs = new Map<string, Promise<FileSystemDirectoryHandle>>();

  constructor(
    private readonly root: Promise<FileSystemDirectoryHandle>,
    readonly writable: boolean,
  ) {}

  /** A folder of the tree; with create, made where it is missing. */
  private dir(names: string[], create = false): Promise<FileSystemDirectoryHandle> {
    if (!names.length) return this.root;
    const key = names.join('/');
    const known = this.dirs.get(key);
    if (known) return known;
    const found = this.dir(names.slice(0, -1), create).then((parent) =>
      parent.getDirectoryHandle(names[names.length - 1], { create }),
    );
    this.dirs.set(key, found);
    found.catch(() => this.dirs.delete(key));
    return found;
  }

  /** Forgets the folder handles at and below a path (it was removed or moved). */
  private forget(names: string[]): void {
    const key = names.join('/');
    for (const k of [...this.dirs.keys()])
      if (k === key || k.startsWith(`${key}/`)) this.dirs.delete(k);
  }

  /** What is at the path: a file's handle, or a folder's. */
  private async entry(names: string[]): Promise<FileSystemHandle> {
    if (!names.length) return this.root;
    const parent = await this.dir(names.slice(0, -1));
    const name = names[names.length - 1];
    try {
      return await parent.getFileHandle(name);
    } catch (e) {
      if (!(e instanceof DOMException && e.name === 'TypeMismatchError')) throw e;
      return parent.getDirectoryHandle(name);
    }
  }

  async file(names: string[]): Promise<File> {
    const entry = await this.entry(names);
    if (entry.kind !== 'file') throw new FsError(OTHER, `${names.join('/')}: a folder`);
    return (entry as FileSystemFileHandle).getFile();
  }

  async stat(names: string[]): Promise<FsStat> {
    const entry = await this.entry(names);
    if (entry.kind === 'directory') return { dir: true, len: 0, modified: 0 };
    const f = await (entry as FileSystemFileHandle).getFile();
    return { dir: false, len: f.size, modified: f.lastModified / 1000 };
  }

  async list(names: string[], limit: number): Promise<DirEntry[]> {
    const out: DirEntry[] = [];
    for await (const [name, entry] of (await this.dir(names)).entries()) {
      out.push([name, entry.kind === 'directory']);
      if (limit && out.length >= limit) break;
    }
    return out;
  }

  /** Replaces a file; its folder must be there, as std::fs::write wants it. */
  async write(names: string[], data: Blob | Uint8Array, progress?: Progress): Promise<void> {
    if (!this.writable) throw readOnly();
    const parent = await this.dir(names.slice(0, -1));
    const handle = await parent.getFileHandle(names[names.length - 1], { create: true });
    await writeHandle(handle, data, progress);
  }

  async mkdirs(names: string[]): Promise<void> {
    if (this.writable) await this.dir(names, true);
    else await this.dir(names).catch(() => Promise.reject(readOnly()));
  }

  async removeFile(names: string[]): Promise<void> {
    if (!this.writable) throw readOnly();
    const parent = await this.dir(names.slice(0, -1));
    const name = names[names.length - 1];
    await parent.getFileHandle(name);
    await parent.removeEntry(name);
  }

  async removeDir(names: string[], all: boolean): Promise<void> {
    if (!this.writable || !names.length) throw readOnly();
    const parent = await this.dir(names.slice(0, -1));
    const name = names[names.length - 1];
    await parent.getDirectoryHandle(name);
    await parent.removeEntry(name, { recursive: all });
    this.forget(names);
  }

  /** Moves a file or folder, replacing a file there (or an empty folder), as std::fs::rename does. */
  async rename(from: string[], to: string[]): Promise<void> {
    if (!this.writable || !from.length || !to.length) throw readOnly();
    const entry = await this.entry(from);
    const target = await this.dir(to.slice(0, -1));
    const name = to[to.length - 1];
    // a file there is replaced; a folder there only when it is empty (else: not empty)
    if (await this.entry(to).catch(() => null)) await target.removeEntry(name);
    this.forget(from);
    this.forget(to);
    const movable = entry as unknown as Partial<MovableHandle>;
    if (typeof movable.move === 'function') {
      try {
        await movable.move(target, name);
        return;
      } catch {
        // this browser cannot move it: copied, then removed
      }
    }
    await this.copyEntry(entry, target, name);
    const parent = await this.dir(from.slice(0, -1));
    await parent.removeEntry(from[from.length - 1], { recursive: true });
  }

  /** A file or folder copied into a folder under a name. */
  private async copyEntry(
    entry: FileSystemHandle,
    into: FileSystemDirectoryHandle,
    name: string,
  ): Promise<void> {
    if (entry.kind === 'file') {
      const file = await (entry as FileSystemFileHandle).getFile();
      await writeHandle(await into.getFileHandle(name, { create: true }), file);
      return;
    }
    const dir = await into.getDirectoryHandle(name, { create: true });
    for await (const [child, handle] of (entry as FileSystemDirectoryHandle).entries())
      await this.copyEntry(handle, dir, child);
  }
}

/** A folder chosen as files (a folder input, where the browser has no folder picker): read-only, this visit only. */
export class FilesMount implements MountFs {
  readonly writable = false;
  /** Each folder's entries by name: a file, or null for a folder. */
  private readonly tree = new Map<string, Map<string, File | null>>();

  constructor(files: readonly ChosenFile[]) {
    this.tree.set('', new Map());
    for (const { path, file } of files) {
      const names = path.split('/').filter(Boolean);
      names.forEach((name, k) => {
        const dir = names.slice(0, k).join('/');
        const entries = this.tree.get(dir) ?? new Map<string, File | null>();
        this.tree.set(dir, entries);
        entries.set(name, k === names.length - 1 ? file : null);
      });
    }
  }

  private at(names: string[]): File | null {
    if (!names.length) return null;
    const entries = this.tree.get(names.slice(0, -1).join('/'));
    const found = entries?.get(names[names.length - 1]);
    if (found === undefined) throw notFound(names.join('/'));
    return found;
  }

  async file(names: string[]): Promise<File> {
    const f = this.at(names);
    if (!f) throw new FsError(OTHER, `${names.join('/')}: a folder`);
    return f;
  }

  async stat(names: string[]): Promise<FsStat> {
    const f = this.at(names);
    return f
      ? { dir: false, len: f.size, modified: f.lastModified / 1000 }
      : { dir: true, len: 0, modified: 0 };
  }

  async list(names: string[], limit: number): Promise<DirEntry[]> {
    const entries = this.tree.get(names.join('/'));
    if (!entries) throw notFound(names.join('/'));
    const out = [...entries].map(([name, f]): DirEntry => [name, f === null]);
    return limit ? out.slice(0, limit) : out;
  }

  write(): Promise<void> {
    return Promise.reject(readOnly());
  }

  async mkdirs(names: string[]): Promise<void> {
    if (!this.tree.has(names.join('/'))) throw readOnly();
  }

  removeFile(): Promise<void> {
    return Promise.reject(readOnly());
  }

  removeDir(): Promise<void> {
    return Promise.reject(readOnly());
  }

  rename(): Promise<void> {
    return Promise.reject(readOnly());
  }
}

/** The models shipped beside the app, read over HTTP (read-only): models.json and each model's two files. */
export class HttpMount implements MountFs {
  readonly writable = false;
  private readonly stats = new Map<string, Promise<FsStat>>();
  private models: Promise<string[]> | null = null;

  constructor(private readonly base: string) {}

  private url(name: string): string {
    return new URL(encodeURIComponent(name), this.base).href;
  }

  /** A file of the folder; a page the server gives for any path (an HTML page) is no file. */
  private async get(name: string, method: 'GET' | 'HEAD'): Promise<Response> {
    const res = await fetch(this.url(name), { method });
    const page = (res.headers.get('content-type') ?? '').startsWith('text/html');
    if (res.status === 404 || (res.ok && page)) throw notFound(name);
    if (!res.ok) throw new FsError(OTHER, `${name}: ${res.status} ${res.statusText}`);
    return res;
  }

  async file(names: string[]): Promise<File> {
    if (names.length !== 1) throw notFound(names.join('/'));
    const res = await this.get(names[0], 'GET');
    const modified = Date.parse(res.headers.get('last-modified') ?? '') || 0;
    return new File([await res.blob()], names[0], { lastModified: modified });
  }

  stat(names: string[]): Promise<FsStat> {
    if (!names.length) return Promise.resolve({ dir: true, len: 0, modified: 0 });
    if (names.length !== 1) return Promise.reject(notFound(names.join('/')));
    let known = this.stats.get(names[0]);
    if (!known) {
      known = this.get(names[0], 'HEAD').then((res) => ({
        dir: false,
        len: Number(res.headers.get('content-length') ?? 0),
        modified: (Date.parse(res.headers.get('last-modified') ?? '') || 0) / 1000,
      }));
      this.stats.set(names[0], known);
    }
    return known;
  }

  /** models.json and the files of each model it names. */
  async list(names: string[], limit: number): Promise<DirEntry[]> {
    if (names.length) throw notFound(names.join('/'));
    this.models ??= this.file(['models.json'])
      .then((f) => f.text())
      .then((text) => Object.keys((JSON.parse(text) as ModelsFile).models ?? {}))
      .catch(() => []);
    const files = (await this.models).flatMap((m) => [
      `detector_${m}.json`,
      `detector_${m}_u8in.onnx`,
    ]);
    const out = ['models.json', ...files].map((n): DirEntry => [n, false]);
    return limit ? out.slice(0, limit) : out;
  }

  write(): Promise<void> {
    return Promise.reject(readOnly());
  }

  async mkdirs(names: string[]): Promise<void> {
    if (names.length) throw readOnly();
  }

  removeFile(): Promise<void> {
    return Promise.reject(readOnly());
  }

  removeDir(): Promise<void> {
    return Promise.reject(readOnly());
  }

  rename(): Promise<void> {
    return Promise.reject(readOnly());
  }
}

/** models.json, as far as the folder's listing reads it: the models it names. */
interface ModelsFile {
  models?: Record<string, unknown>;
}

/** A path's place: its mount and the names below it. */
interface Place {
  fs: MountFs;
  names: string[];
  path: string;
}

const ok = (bytes = new Uint8Array()): FsResult => ({ code: 0, bytes });

/**
 * The service's file system in the browser (the contract's mounts): /data and /kovaak in the browser's private file
 * system, /vods the VODs folder the user opened (missing until then), /models the models over HTTP. It answers the
 * service's host_fs calls and the page's own reads and writes.
 */
export class Mounts {
  private readonly table = new Map<MountName, MountFs>();

  /** Mounts a file system at /name (null: nothing there). */
  set(name: MountName, fs: MountFs | null): void {
    if (fs) this.table.set(name, fs);
    else this.table.delete(name);
  }

  /** Whether something is mounted at /name. */
  has(name: MountName): boolean {
    return this.table.has(name);
  }

  /** Where a path is: its mount and the names below it. Throws for a path outside the mounts, or with . or ..  */
  private place(path: string): Place {
    const names = path.split('/').filter(Boolean);
    if (!path.startsWith('/') || names.some((n) => n === '.' || n === '..'))
      throw new FsError(OTHER, `${path}: not an absolute path`);
    const fs = this.table.get(names[0] as MountName);
    if (!fs) throw notFound(path);
    return { fs, names: names.slice(1), path };
  }

  /** The service's file system call (the contract's host_fs ops), answered as a result block's code and bytes. */
  async host(op: number, path: string, arg: Uint8Array): Promise<FsResult> {
    try {
      if (op === 8 && !path.split('/').some(Boolean))
        return ok(new TextEncoder().encode(JSON.stringify({ dir: true, len: 0, modified: 0 })));
      const p = this.place(path);
      switch (op) {
        case 0:
          return ok(new Uint8Array(await (await p.fs.file(p.names)).arrayBuffer()));
        case 1:
          await p.fs.write(p.names, arg);
          return ok();
        case 2:
          await p.fs.mkdirs(p.names);
          return ok();
        case 3:
          await p.fs.removeFile(p.names);
          return ok();
        case 4:
        case 5:
          await p.fs.removeDir(p.names, op === 5);
          return ok();
        case 6: {
          const to = this.place(new TextDecoder().decode(arg));
          if (to.fs !== p.fs) throw new FsError(OTHER, `${path}: cannot be moved to another mount`);
          await p.fs.rename(p.names, to.names);
          return ok();
        }
        case 7: {
          const entries = await p.fs.list(p.names, 0);
          return ok(new TextEncoder().encode(JSON.stringify(entries)));
        }
        case 8:
          return ok(new TextEncoder().encode(JSON.stringify(await p.fs.stat(p.names))));
        default:
          throw new FsError(OTHER, `no file system call ${op}`);
      }
    } catch (e) {
      const err = asFsError(e, path);
      return { code: err.code, bytes: new TextEncoder().encode(err.message) };
    }
  }

  /** A file, for the page. */
  file(path: string): Promise<File> {
    const p = this.place(path);
    return p.fs.file(p.names).catch((e: unknown) => Promise.reject(asFsError(e, path)));
  }

  /** A folder's entries (at most limit; 0: all), for the page. */
  list(path: string, limit: number): Promise<DirEntry[]> {
    const p = this.place(path);
    return p.fs.list(p.names, limit).catch((e: unknown) => Promise.reject(asFsError(e, path)));
  }

  /** Writes a file, making its folders. */
  async write(path: string, data: Blob | Uint8Array, progress?: Progress): Promise<void> {
    const p = this.place(path);
    try {
      await p.fs.mkdirs(p.names.slice(0, -1));
      await p.fs.write(p.names, data, progress);
    } catch (e) {
      throw asFsError(e, path);
    }
  }

  /** Removes a file. */
  async removeFile(path: string): Promise<void> {
    const p = this.place(path);
    await p.fs.removeFile(p.names).catch((e: unknown) => Promise.reject(asFsError(e, path)));
  }

  /** Whether a file or folder is there. */
  async exists(path: string): Promise<boolean> {
    const p = this.place(path);
    return p.fs.stat(p.names).then(
      () => true,
      () => false,
    );
  }

  /**
   * Copies files into a folder, below it at their paths: only those new or changed (size or time) since the last copy,
   * by its index (copied.json in the folder). `turn` runs each step in the worker's queue, so the service's requests
   * are answered between them. Resolves to how many it copied.
   */
  async copyIn(
    dir: string,
    files: readonly ChosenFile[],
    turn: <T>(step: () => Promise<T>) => Promise<T>,
    progress: Progress,
  ): Promise<CopyDone> {
    const indexPath = `${dir}/${COPIED}`;
    const index = await turn(async (): Promise<CopiedIndex> => {
      try {
        return JSON.parse(await (await this.file(indexPath)).text()) as CopiedIndex;
      } catch {
        return {};
      }
    });
    const fresh = files.filter(({ path, file }) => {
      const kept = index[path];
      return !kept || kept[0] !== file.size || kept[1] !== file.lastModified;
    });
    for (let at = 0; at < fresh.length; at += COPY_STEP) {
      progress(at, fresh.length);
      await turn(async () => {
        for (const { path, file } of fresh.slice(at, at + COPY_STEP)) {
          await this.write(`${dir}/${path}`, file);
          index[path] = [file.size, file.lastModified];
        }
        if (at + COPY_STEP >= fresh.length || (at / COPY_STEP) % INDEX_EVERY === INDEX_EVERY - 1)
          await this.write(indexPath, new TextEncoder().encode(JSON.stringify(index)));
      });
    }
    progress(fresh.length, fresh.length);
    return { copied: fresh.length };
  }
}
