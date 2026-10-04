/// <reference lib="webworker" />
import { ChosenFile, CopyDone, DirEntry } from './service-messages';
import { FsResult } from './service-module';

/** The codes of the host's file system calls (the contract's host_fs). */
const OK = 0;
const NOT_FOUND = 1;
const EXISTS = 2;
const OTHER = 3;
/** The host's file system calls (the contract's host_fs ops, service/src/disk.rs `Op`). */
const OP_READ = 0;
const OP_WRITE = 1;
const OP_CREATE_DIR_ALL = 2;
const OP_REMOVE_FILE = 3;
const OP_REMOVE_DIR = 4;
const OP_REMOVE_DIR_ALL = 5;
const OP_RENAME = 6;
const OP_READ_DIR = 7;
const OP_METADATA = 8;
/** File times are kept in ms; the service's metadata gives seconds. */
const MS_PER_SECOND = 1000;
const HTTP_NOT_FOUND = 404;
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
  /** A folder's entries (at most limit; 0: all), with each file's size and time where the mount has them cheaply. */
  list(names: string[], limit: number, stat?: boolean): Promise<DirEntry[]>;
  write(names: string[], data: Blob | Uint8Array, progress?: Progress): Promise<void>;
  mkdirs(names: string[]): Promise<void>;
  removeFile(names: string[]): Promise<void>;
  removeDir(names: string[], all: boolean): Promise<void>;
  rename(from: string[], to: string[]): Promise<void>;
}

const notFound = (path: string) => new FsError(NOT_FOUND, `${path}: not found`);
const readOnly = () => new FsError(OTHER, 'read-only');

/** A browser error as the call's code: not there, not empty, or other (with its message). */
function asFsError(error: unknown, path: string): FsError {
  if (error instanceof FsError) return error;
  const name = error instanceof DOMException ? error.name : '';
  if (name === 'NotFoundError') return notFound(path);
  if (name === 'InvalidModificationError') return new FsError(EXISTS, `${path}: not empty`);
  if (name === 'TypeMismatchError')
    return new FsError(OTHER, `${path}: a file where a folder is, or the other way`);
  return new FsError(OTHER, `${path}: ${error instanceof Error ? error.message : String(error)}`);
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
    for (const dirKey of [...this.dirs.keys()])
      if (dirKey === key || dirKey.startsWith(`${key}/`)) this.dirs.delete(dirKey);
  }

  /** What is at the path: a file's handle, or a folder's. */
  private async entry(names: string[]): Promise<FileSystemHandle> {
    if (!names.length) return this.root;
    const parent = await this.dir(names.slice(0, -1));
    const name = names[names.length - 1];
    try {
      return await parent.getFileHandle(name);
    } catch (error) {
      if (!(error instanceof DOMException && error.name === 'TypeMismatchError')) throw error;
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
    const file = await (entry as FileSystemFileHandle).getFile();
    return { dir: false, len: file.size, modified: file.lastModified / MS_PER_SECOND };
  }

  /**
   * The entries, and unless stat is false each file's size and time, its files read all at once: the service then
   * needs no call a file for them (the recordings list reads thousands).
   */
  async list(names: string[], limit: number, stat = true): Promise<DirEntry[]> {
    const handles: FileSystemHandle[] = [];
    for await (const entry of (await this.dir(names)).values()) {
      handles.push(entry);
      if (limit && handles.length >= limit) break;
    }
    return Promise.all(
      handles.map(async (handle): Promise<DirEntry> => {
        if (handle.kind === 'directory') return [handle.name, true, 0, 0];
        if (!stat) return [handle.name, false, null, null];
        // a file gone since the listing: its metadata is asked for later, and says so
        const file = await (handle as FileSystemFileHandle).getFile().catch(() => null);
        return file
          ? [handle.name, false, file.size, file.lastModified / MS_PER_SECOND]
          : [handle.name, false, null, null];
      }),
    );
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
      names.forEach((name, depth) => {
        const dir = names.slice(0, depth).join('/');
        const entries = this.tree.get(dir) ?? new Map<string, File | null>();
        this.tree.set(dir, entries);
        entries.set(name, depth === names.length - 1 ? file : null);
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
    const file = this.at(names);
    if (!file) throw new FsError(OTHER, `${names.join('/')}: a folder`);
    return file;
  }

  async stat(names: string[]): Promise<FsStat> {
    const file = this.at(names);
    return file
      ? { dir: false, len: file.size, modified: file.lastModified / MS_PER_SECOND }
      : { dir: true, len: 0, modified: 0 };
  }

  async list(names: string[], limit: number): Promise<DirEntry[]> {
    const entries = this.tree.get(names.join('/'));
    if (!entries) throw notFound(names.join('/'));
    const out = [...entries].map(([name, file]): DirEntry =>
      file ? [name, false, file.size, file.lastModified / MS_PER_SECOND] : [name, true, 0, 0],
    );
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

/** A file kept in a pack: the pack's number, where the file starts in it, its length, and its time (ms since 1970). */
type PackedFile = [pack: number, at: number, len: number, modified: number];

/** The packs' index (index.json): each file by its path below the mount, and the next pack's number. */
interface PackIndex {
  next: number;
  files: Record<string, PackedFile>;
}

/** A pack holds at most this many bytes, or PACK_FILES files. */
const PACK_BYTES = 32 << 20;
const PACK_FILES = 4000;
const PACK_INDEX = 'index.json';

/** Bytes written into a file of a folder, replacing it (the worker's sync access handle: one open, one flush). */
async function writeWhole(
  dir: FileSystemDirectoryHandle,
  name: string,
  parts: Uint8Array[],
): Promise<void> {
  const out = await (await dir.getFileHandle(name, { create: true })).createSyncAccessHandle();
  try {
    out.truncate(0);
    let at = 0;
    for (const part of parts) at += out.write(part, { at });
    out.flush();
  } finally {
    out.close();
  }
}

/**
 * Copies kept in a few large files (packs) and an index (index.json), read-only to the service: thousands of small
 * files are written far faster this way than one file each (a file each took 100 ms or more on a busy machine). A file
 * copied again goes into a new pack; its old copy stays in its pack, unread.
 */
export class PackStore {
  private index: Promise<PackIndex> | null = null;
  /** Each folder's entries by name: a file's place, or null for a folder. */
  private tree = new Map<string, Map<string, PackedFile | null>>();
  private readonly packs = new Map<number, Promise<File>>();

  constructor(private readonly root: Promise<FileSystemDirectoryHandle>) {}

  private read(): Promise<PackIndex> {
    this.index ??= this.root
      .then((dir) => dir.getFileHandle(PACK_INDEX))
      .then((handle) => handle.getFile())
      .then((file) => file.text())
      .then((text) => JSON.parse(text) as PackIndex)
      .catch((): PackIndex => ({ next: 0, files: {} }))
      .then((index) => {
        this.tree = treeOf(index.files);
        return index;
      });
    return this.index;
  }

  /** A path's file (its place), a folder (null), or nothing there (undefined). */
  async at(names: string[]): Promise<PackedFile | null | undefined> {
    await this.read();
    if (!names.length) return null;
    return this.tree.get(names.slice(0, -1).join('/'))?.get(names[names.length - 1]);
  }

  async file(names: string[]): Promise<File> {
    const place = await this.at(names);
    if (!place) throw notFound(names.join('/'));
    const [pack, at, len, modified] = place;
    let file = this.packs.get(pack);
    if (!file) {
      file = this.root
        .then((dir) => dir.getFileHandle(`${pack}.pack`))
        .then((handle) => handle.getFile());
      this.packs.set(pack, file);
    }
    return new File([(await file).slice(at, at + len)], names[names.length - 1], {
      lastModified: modified,
    });
  }

  async list(names: string[]): Promise<DirEntry[] | null> {
    await this.read();
    const entries = this.tree.get(names.join('/'));
    return entries
      ? [...entries].map(([name, place]): DirEntry => {
          if (!place) return [name, true, 0, 0];
          const [, , len, modified] = place;
          return [name, false, len, modified / MS_PER_SECOND];
        })
      : null;
  }

  /**
   * Copies the files new or changed (size or time) since the last copy into new packs; each pack's files are read
   * first, then the pack and the index are written in one turn of the worker's queue. Resolves to how many it copied.
   */
  async copy(
    files: readonly ChosenFile[],
    turn: <T>(step: () => Promise<T>) => Promise<T>,
    progress: Progress,
  ): Promise<CopyDone> {
    const index = await this.read();
    const fresh = files.filter(({ path, file }) => {
      const kept = index.files[path];
      if (!kept) return true;
      const [, , len, modified] = kept;
      return len !== file.size || modified !== file.lastModified;
    });
    let done = 0;
    while (done < fresh.length) {
      progress(done, fresh.length);
      const batch: ChosenFile[] = [];
      let bytes = 0;
      for (const chosen of fresh.slice(done)) {
        if (batch.length && (batch.length >= PACK_FILES || bytes + chosen.file.size > PACK_BYTES))
          break;
        batch.push(chosen);
        bytes += chosen.file.size;
      }
      const parts = await Promise.all(
        batch.map(async (chosen) => new Uint8Array(await chosen.file.arrayBuffer())),
      );
      await turn(async () => {
        const dir = await this.root;
        const pack = index.next++;
        await writeWhole(dir, `${pack}.pack`, parts);
        let at = 0;
        batch.forEach(({ path, file }, partIndex) => {
          index.files[path] = [pack, at, parts[partIndex].length, file.lastModified];
          at += parts[partIndex].length;
        });
        await writeWhole(dir, PACK_INDEX, [new TextEncoder().encode(JSON.stringify(index))]);
        this.tree = treeOf(index.files);
      });
      done += batch.length;
    }
    progress(fresh.length, fresh.length);
    return { copied: fresh.length };
  }
}

/** Each folder's entries by name, from the files' paths: a file's place, or null for a folder. */
function treeOf(files: Record<string, PackedFile>): Map<string, Map<string, PackedFile | null>> {
  const tree = new Map<string, Map<string, PackedFile | null>>([['', new Map()]]);
  for (const [path, place] of Object.entries(files)) {
    const names = path.split('/').filter(Boolean);
    names.forEach((name, depth) => {
      const dir = names.slice(0, depth).join('/');
      const entries = tree.get(dir) ?? new Map<string, PackedFile | null>();
      tree.set(dir, entries);
      entries.set(name, depth === names.length - 1 ? place : null);
    });
  }
  return tree;
}

/**
 * KovaaK's files (/kovaak), read-only to the service: the files the user chose this visit, read where they are at
 * once; under them the copies this browser keeps for later visits, in packs; under those the copies an earlier
 * version kept one file each. A file is read from the first of them that has it, and a folder lists them all.
 */
export class KovaakMount implements MountFs {
  readonly writable = false;
  private chosen: FilesMount | null = null;

  constructor(
    readonly packs: PackStore,
    private readonly older: DirMount,
  ) {}

  /** The files chosen this visit, shown at once over the kept copies. */
  show(files: readonly ChosenFile[]): void {
    this.chosen = new FilesMount(files);
  }

  /** The first answer of the layers that has the path. */
  private async first<T>(
    names: string[],
    get: (fs: MountFs | PackStore) => Promise<T>,
  ): Promise<T> {
    for (const fs of [this.chosen, this.packs, this.older]) {
      if (!fs) continue;
      try {
        return await get(fs);
      } catch (error) {
        if (asFsError(error, names.join('/')).code !== NOT_FOUND) throw error;
      }
    }
    throw notFound(names.join('/'));
  }

  file(names: string[]): Promise<File> {
    return this.first(names, (fs) => fs.file(names));
  }

  stat(names: string[]): Promise<FsStat> {
    return this.first(names, async (fs) => {
      if (fs instanceof PackStore) {
        const place = await fs.at(names);
        if (place === undefined) throw notFound(names.join('/'));
        if (!place) return { dir: true, len: 0, modified: 0 };
        const [, , len, modified] = place;
        return { dir: false, len, modified: modified / MS_PER_SECOND };
      }
      return fs.stat(names);
    });
  }

  async list(names: string[], limit: number): Promise<DirEntry[]> {
    const all = new Map<string, DirEntry>();
    let found = false;
    for (const fs of [this.chosen, this.packs, this.older]) {
      if (!fs) continue;
      // the older copies' times would cost a file read each (70,000 stats files): asked for when needed
      const entries =
        fs instanceof PackStore
          ? await fs.list(names)
          : await fs.list(names, 0, fs !== this.older).catch(() => null);
      if (!entries) continue;
      found = true;
      for (const entry of entries) {
        const [name] = entry;
        if (!all.has(name)) all.set(name, entry);
      }
    }
    if (!found) throw notFound(names.join('/'));
    const out = [...all.values()];
    return limit ? out.slice(0, limit) : out;
  }

  write(): Promise<void> {
    return Promise.reject(readOnly());
  }

  async mkdirs(names: string[]): Promise<void> {
    await this.stat(names).catch(() => Promise.reject(readOnly()));
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
    const response = await fetch(this.url(name), { method });
    const page = (response.headers.get('content-type') ?? '').startsWith('text/html');
    if (response.status === HTTP_NOT_FOUND || (response.ok && page)) throw notFound(name);
    if (!response.ok)
      throw new FsError(OTHER, `${name}: ${response.status} ${response.statusText}`);
    return response;
  }

  async file(names: string[]): Promise<File> {
    if (names.length !== 1) throw notFound(names.join('/'));
    const response = await this.get(names[0], 'GET');
    const modified = Date.parse(response.headers.get('last-modified') ?? '') || 0;
    return new File([await response.blob()], names[0], { lastModified: modified });
  }

  stat(names: string[]): Promise<FsStat> {
    if (!names.length) return Promise.resolve({ dir: true, len: 0, modified: 0 });
    if (names.length !== 1) return Promise.reject(notFound(names.join('/')));
    let known = this.stats.get(names[0]);
    if (!known) {
      known = this.get(names[0], 'HEAD').then((response) => ({
        dir: false,
        len: Number(response.headers.get('content-length') ?? 0),
        modified: (Date.parse(response.headers.get('last-modified') ?? '') || 0) / MS_PER_SECOND,
      }));
      this.stats.set(names[0], known);
    }
    return known;
  }

  /** models.json and the files of each model it names. */
  async list(names: string[], limit: number): Promise<DirEntry[]> {
    if (names.length) throw notFound(names.join('/'));
    this.models ??= this.file(['models.json'])
      .then((file) => file.text())
      .then((text) => Object.keys((JSON.parse(text) as ModelsFile).models ?? {}))
      .catch(() => []);
    const files = (await this.models).flatMap((model) => [
      `detector_${model}.json`,
      `detector_${model}_u8in.onnx`,
    ]);
    const out = ['models.json', ...files].map((name): DirEntry => [name, false, null, null]);
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

const ok = (bytes = new Uint8Array()): FsResult => ({ code: OK, bytes });

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
    if (!path.startsWith('/') || names.some((name) => name === '.' || name === '..'))
      throw new FsError(OTHER, `${path}: not an absolute path`);
    const fs = this.table.get(names[0] as MountName);
    if (!fs) throw notFound(path);
    return { fs, names: names.slice(1), path };
  }

  /** The service's file system call (the contract's host_fs ops), answered as a result block's code and bytes. */
  async host(op: number, path: string, arg: Uint8Array): Promise<FsResult> {
    try {
      if (op === OP_METADATA && !path.split('/').some(Boolean))
        return ok(new TextEncoder().encode(JSON.stringify({ dir: true, len: 0, modified: 0 })));
      const place = this.place(path);
      switch (op) {
        case OP_READ:
          return ok(new Uint8Array(await (await place.fs.file(place.names)).arrayBuffer()));
        case OP_WRITE:
          await place.fs.write(place.names, arg);
          return ok();
        case OP_CREATE_DIR_ALL:
          await place.fs.mkdirs(place.names);
          return ok();
        case OP_REMOVE_FILE:
          await place.fs.removeFile(place.names);
          return ok();
        case OP_REMOVE_DIR:
        case OP_REMOVE_DIR_ALL:
          await place.fs.removeDir(place.names, op === OP_REMOVE_DIR_ALL);
          return ok();
        case OP_RENAME: {
          const to = this.place(new TextDecoder().decode(arg));
          if (to.fs !== place.fs)
            throw new FsError(OTHER, `${path}: cannot be moved to another mount`);
          await place.fs.rename(place.names, to.names);
          return ok();
        }
        case OP_READ_DIR: {
          const entries = await place.fs.list(place.names, 0);
          return ok(new TextEncoder().encode(JSON.stringify(entries)));
        }
        case OP_METADATA:
          return ok(new TextEncoder().encode(JSON.stringify(await place.fs.stat(place.names))));
        default:
          throw new FsError(OTHER, `no file system call ${op}`);
      }
    } catch (error) {
      const failed = asFsError(error, path);
      return { code: failed.code, bytes: new TextEncoder().encode(failed.message) };
    }
  }

  /** A file, for the page. */
  file(path: string): Promise<File> {
    const place = this.place(path);
    return place.fs
      .file(place.names)
      .catch((error: unknown) => Promise.reject(asFsError(error, path)));
  }

  /** A folder's entries (at most limit; 0: all), for the page. */
  list(path: string, limit: number): Promise<DirEntry[]> {
    const place = this.place(path);
    return place.fs
      .list(place.names, limit)
      .catch((error: unknown) => Promise.reject(asFsError(error, path)));
  }

  /** Writes a file, making its folders. */
  async write(path: string, data: Blob | Uint8Array, progress?: Progress): Promise<void> {
    const place = this.place(path);
    try {
      await place.fs.mkdirs(place.names.slice(0, -1));
      await place.fs.write(place.names, data, progress);
    } catch (error) {
      throw asFsError(error, path);
    }
  }

  /** Removes a file. */
  async removeFile(path: string): Promise<void> {
    const place = this.place(path);
    await place.fs
      .removeFile(place.names)
      .catch((error: unknown) => Promise.reject(asFsError(error, path)));
  }

  /** Whether a file or folder is there. */
  async exists(path: string): Promise<boolean> {
    const place = this.place(path);
    return place.fs.stat(place.names).then(
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
    const place = this.place(dir);
    if (place.fs instanceof KovaakMount && !place.names.length)
      return place.fs.packs.copy(files, turn, progress);
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
      if (!kept) return true;
      const [size, modified] = kept;
      return size !== file.size || modified !== file.lastModified;
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
