/**
 * The messages between the page (service-host.ts) and the review service's worker
 * (service.worker.ts), and the types they carry. In: the page's API requests, file asks and
 * mounts. Out: the service's answers, the file asks' results, progress and errors.
 */

/** The methods the service answers. */
export type ServiceMethod = 'GET' | 'POST';

/** The service's answer to one request: its status, its Content-Type and its body. */
export interface ServiceAnswer {
  /** The HTTP status. */
  status: number;
  /** The body's Content-Type. */
  type: string;
  /** The body's bytes. */
  body: Uint8Array;
}

/** Sends one request to the service in the worker (service_handle), in the worker's own queue. */
export type ServiceCall = (
  method: ServiceMethod,
  path: string,
  body: Uint8Array,
) => Promise<ServiceAnswer>;

/** A file of a folder chosen as files, by its path below the folder: one of /vods's files. */
export interface ChosenFile {
  /** Its path below the folder chosen, with forward slashes. */
  path: string;
  /** The file itself. */
  file: File;
}

/**
 * What is mounted at /vods: the VODs folder's handle, or where the browser has no folder picker
 * its files.
 */
export type VodsMount = FileSystemDirectoryHandle | ChosenFile[];

/**
 * The worker's start: where the service's module is, where the models are (the folder models.json
 * is in) and where the shipped data is (area_examples.jsonl and area_kinds.json), as addresses.
 */
export interface ServiceStart {
  /** Tells the task apart. */
  kind: 'start';
  /** The address of the service's WebAssembly. */
  wasmUrl: string;
  /** The address of SQLite's WebAssembly (the service's database). */
  sqliteUrl: string;
  /** The address of the models' folder. */
  modelsUrl: string;
  /** The address of the shipped data's folder. */
  dataUrl: string;
}

/**
 * A request to the service: its method, path with query, and body (a Blob for an upload's file).
 */
export interface ServiceAsk {
  /** Tells the task apart. */
  kind: 'ask';
  /** The ask's number, which its answer carries back. */
  id: number;
  /** The request's method. */
  method: ServiceMethod;
  /** The request's path with its query (/api/...). */
  path: string;
  /** The request's body; null for none. */
  body: Uint8Array | Blob | null;
}

/** What the page does with its files in the mounts. */
export type FilesOp = 'read' | 'list' | 'write' | 'remove' | 'copy';

/**
 * A file the page reads or writes in the mounts (a mounted path: /data/..., /kovaak/...,
 * /vods/...): read gives the File, list the folder's entries (at most `limit` of them; 0: all),
 * write takes `body`, remove removes a file, copy copies `files` into the folder (only those new or
 * changed since the last copy).
 */
export interface FilesAsk {
  /** Tells the task apart. */
  kind: 'files';
  /** The ask's number, which its result carries back. */
  id: number;
  /** What to do with the path. */
  op: FilesOp;
  /** The mounted path: a file, or for list and copy a folder. */
  path: string;
  /** What write writes; null for the other ops. */
  body: Blob | null;
  /** What copy copies; empty for the other ops. */
  files: ChosenFile[];
  /** The most entries list gives; 0 for all. */
  limit: number;
}

/** Mounts a VODs folder at /vods (null: none). */
export interface MountAsk {
  /** Tells the task apart. */
  kind: 'mount';
  /** The ask's number, which its end carries back. */
  id: number;
  /** The folder to mount; null to unmount. */
  vods: VodsMount | null;
}

/**
 * KovaaK's files the user chose this visit, shown at /kovaak at once (read where they are, until
 * the page closes), over the copies this browser keeps (a copy into /kovaak, FilesAsk's copy,
 * keeps them for later visits).
 */
export interface KovaakAsk {
  /** Tells the task apart. */
  kind: 'kovaak';
  /** The ask's number, which its end carries back. */
  id: number;
  /** The files, by their paths below /kovaak. */
  files: ChosenFile[];
}

/** What the page tells the service's worker. */
export type ServiceTask = ServiceStart | ServiceAsk | FilesAsk | MountAsk | KovaakAsk;

/**
 * A folder's entry: its name, whether it is a folder, and its size and time (seconds since 1970)
 * when the listing has them at no cost (null: the service asks for its metadata when it needs it).
 * A folder's size and time are 0.
 */
export type DirEntry = [name: string, dir: boolean, len: number | null, modified: number | null];

/** What a copy did: how many files it copied. */
export interface CopyDone {
  /** The files copied: those new or changed since the last copy. */
  copied: number;
}

/** What one of the page's file asks gives back. */
export type FilesResult = File | DirEntry[] | CopyDone | null;

/** The service's answer to an ask. */
export interface AskAnswered {
  /** Tells the reply apart. */
  kind: 'answer';
  /** The number of the ask it answers. */
  id: number;
  /** The service's answer. */
  answer: ServiceAnswer;
}

/** A file ask's or a mount's end. */
export interface TaskDone {
  /** Tells the reply apart. */
  kind: 'done';
  /** The number of the ask it ends. */
  id: number;
  /** What the ask gives back (null for write, remove, mount and kovaak). */
  result: FilesResult;
}

/** How far an ask is: an upload's bytes written, a copy's files copied, of how many. */
export interface TaskProgress {
  /** Tells the reply apart. */
  kind: 'progress';
  /** The number of the ask. */
  id: number;
  /** The bytes or files done so far. */
  done: number;
  /** The bytes or files in all. */
  total: number;
}

/** An ask that failed: why, and the status it answers as (404: no such file). */
export interface TaskFailed {
  /** Tells the reply apart. */
  kind: 'error';
  /** The number of the ask that failed. */
  id: number;
  /** The HTTP status the page answers with. */
  status: number;
  /** Why it failed, in words. */
  error: string;
}

/** What the service's worker tells the page. */
export type ServiceReply = AskAnswered | TaskDone | TaskProgress | TaskFailed;
