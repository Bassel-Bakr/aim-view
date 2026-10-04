/** The methods the service answers. */
export type ServiceMethod = 'GET' | 'POST';

/** The service's answer to one request: its status, its Content-Type and its body. */
export interface ServiceAnswer {
  status: number;
  type: string;
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
  path: string;
  file: File;
}

/** What is mounted at /vods: the VODs folder's handle, or where the browser has no folder picker its files. */
export type VodsMount = FileSystemDirectoryHandle | ChosenFile[];

/**
 * The worker's start: where the service's module is, where the models are (the folder models.json is in) and where
 * the shipped data is (area_examples.jsonl and area_kinds.json), as addresses.
 */
export interface ServiceStart {
  kind: 'start';
  wasmUrl: string;
  modelsUrl: string;
  dataUrl: string;
}

/** A request to the service: its method, path with query, and body (a Blob for an upload's file). */
export interface ServiceAsk {
  kind: 'ask';
  id: number;
  method: ServiceMethod;
  path: string;
  body: Uint8Array | Blob | null;
}

/** What the page does with its files in the mounts. */
export type FilesOp = 'read' | 'list' | 'write' | 'remove' | 'copy';

/**
 * A file the page reads or writes in the mounts (a mounted path: /data/..., /kovaak/..., /vods/...): read gives the
 * File, list the folder's entries (at most `limit` of them; 0: all), write takes `body`, remove removes a file, copy
 * copies `files` into the folder (only those new or changed since the last copy).
 */
export interface FilesAsk {
  kind: 'files';
  id: number;
  op: FilesOp;
  path: string;
  body: Blob | null;
  files: ChosenFile[];
  limit: number;
}

/** Mounts a VODs folder at /vods (null: none). */
export interface MountAsk {
  kind: 'mount';
  id: number;
  vods: VodsMount | null;
}

/**
 * KovaaK's files the user chose this visit, shown at /kovaak at once (read where they are, until the page closes), over
 * the copies this browser keeps (a copy into /kovaak, FilesAsk's copy, keeps them for later visits).
 */
export interface KovaakAsk {
  kind: 'kovaak';
  id: number;
  files: ChosenFile[];
}

/** What the page tells the service's worker. */
export type ServiceTask = ServiceStart | ServiceAsk | FilesAsk | MountAsk | KovaakAsk;

/** A folder's entry: its name, and whether it is a folder. */
export type DirEntry = [name: string, dir: boolean];

/** What a copy did: how many files it copied. */
export interface CopyDone {
  copied: number;
}

/** What one of the page's file asks gives back. */
export type FilesResult = File | DirEntry[] | CopyDone | null;

/** The service's answer to an ask. */
export interface AskAnswered {
  kind: 'answer';
  id: number;
  answer: ServiceAnswer;
}

/** A file ask's or a mount's end. */
export interface TaskDone {
  kind: 'done';
  id: number;
  result: FilesResult;
}

/** How far an ask is: an upload's bytes written, a copy's files copied, of how many. */
export interface TaskProgress {
  kind: 'progress';
  id: number;
  done: number;
  total: number;
}

/** An ask that failed: why, and the status it answers as (404: no such file). */
export interface TaskFailed {
  kind: 'error';
  id: number;
  status: number;
  error: string;
}

/** What the service's worker tells the page. */
export type ServiceReply = AskAnswered | TaskDone | TaskProgress | TaskFailed;
