/// <reference lib="webworker" />
/**
 * Loads and calls the review service built as WebAssembly (browser-service/), in its worker. In:
 * the module's address, the worker's file system calls (mounts.ts), the service's config and each
 * request, and its database (service-database.ts). Out: the service's answers, its file system
 * calls awaited through Binaryen's Asyncify, its database calls answered at once.
 */
import { ServiceAnswer } from './service-messages';

/** Asyncify's state (Binaryen) while it unwinds the stack for an async call (0 is running). */
const UNWINDING = 1;
/** Asyncify's state while it rewinds the stack after one. */
const REWINDING = 2;
/** The space Asyncify saves the stack in while an async call runs, in bytes (1 MiB). */
const STACK_BYTES = 1 << 20;
/** A u32 in the module's memory, as the blocks and Asyncify's data hold them (little-endian). */
const U32_BYTES = 4;
/**
 * A block's header: two u32s (a result block's code and length; Asyncify's data's start and
 * end).
 */
const HEADER_BYTES = 2 * U32_BYTES;
/** A file system call's code when it failed for another reason than the ones the contract names. */
const FS_OTHER = 3;
/** Milliseconds in a second, for Date's times. */
const MS_PER_SECOND = 1000;
/** Seconds in a minute, for the time zone offset Date gives in minutes. */
const SECONDS_PER_MINUTE = 60;

/**
 * The service's exports (browser-service/): its memory and allocator, the two calls (service_open,
 * service_handle; each answers a result block it allocated), and Asyncify's controls (wasm-opt
 * --asyncify).
 */
export interface ServiceExports {
  /** The module's memory, which every pointer is an offset into; it can grow. */
  memory: WebAssembly.Memory;
  /** Reserves len bytes; gives their offset. */
  alloc(len: number): number;
  /** Frees what one alloc call reserved. */
  dealloc(ptr: number, len: number): void;
  /** Opens the service's library with the config (JSON); gives a block of [code, len, why]. */
  service_open(config: number, len: number): number;
  /** Answers one request (its JSON and body); gives a block of [status, type, body]. */
  service_handle(req: number, reqLen: number, body: number, bodyLen: number): number;
  /** Starts unwinding the stack into the Asyncify data at data. */
  asyncify_start_unwind(data: number): void;
  /** Ends the unwinding, once the export has returned. */
  asyncify_stop_unwind(): void;
  /** Starts rewinding the stack from the Asyncify data, before the export is called again. */
  asyncify_start_rewind(data: number): void;
  /** Ends the rewinding, once the call that unwound is reached again. */
  asyncify_stop_rewind(): void;
  /** Asyncify's state: 0 running, `UNWINDING` or `REWINDING`. */
  asyncify_get_state(): number;
}

/**
 * What a file system call gives back: its code (0 ok, 1 not found, 2 exists or not empty, 3 other)
 * and bytes.
 */
export interface FsResult {
  /** 0 ok, 1 not found, 2 exists or not empty, 3 other. */
  code: number;
  /** What the call gives: a file's bytes, a listing, or why it failed. */
  bytes: Uint8Array;
}

/**
 * The host's file system call: the op, the path and the argument's bytes (copied out of the
 * module's memory).
 */
export type HostFs = (op: number, path: string, arg: Uint8Array) => Promise<FsResult>;

/**
 * The host's database call (service/src/sql.rs: host_sql): the op, the statement and its values in
 * the binary form; answered at once, with a code (0 done) and bytes as a file system call is.
 */
export type HostSql = (op: number, sql: string, values: Uint8Array) => FsResult;

/**
 * The request service_handle takes: its method and path, and for an upload the file its body was
 * written to.
 */
export interface HandleRequest {
  /** GET or POST. */
  method: string;
  /** The path with its query (/api/...). */
  path: string;
  /** The mounted path the upload's body was written to, which the service moves in place. */
  upload?: string;
}

/** A failed service_open: why. */
export class OpenFailed extends Error {}

/**
 * The service as WebAssembly (browser-service/), with its file system calls answered by `fs`. The
 * module waits for them with Binaryen's Asyncify: host_fs starts the call and unwinds the stack;
 * the export's caller awaits the call, rewinds the stack and calls the export again, which then
 * takes the answer. One call runs at a time.
 */
export class ServiceModule {
  /** The instance's exports. */
  private readonly exports: ServiceExports;
  /** The Asyncify data: the stack's save space, with its start and end in front. */
  private readonly data: number;
  /**
   * The file system call under way while the stack is unwound, and its answer once in (a result
   * block).
   */
  private pending: Promise<FsResult> | null = null;
  /** The result block host_fs gives when the stack is rewound; 0 when none waits. */
  private answer = 0;

  /** Keeps the instance's exports and reserves the Asyncify data. */
  private constructor(instance: WebAssembly.Instance) {
    this.exports = instance.exports as unknown as ServiceExports;
    this.data = this.exports.alloc(HEADER_BYTES + STACK_BYTES);
  }

  /**
   * Loads the module from `url`, its file system calls answered by `fs` and its database calls by
   * `sql`. Rejects when it cannot be fetched.
   */
  static async load(url: string, fs: HostFs, sql: HostSql): Promise<ServiceModule> {
    let module: ServiceModule | null = null;
    const imports: WebAssembly.Imports = {
      host: {
        host_fs: (op: number, path: number, pathLen: number, arg: number, argLen: number) =>
          (module as ServiceModule).hostFs(fs, op, path, pathLen, arg, argLen),
        host_sql: (
          op: number,
          statement: number,
          statementLen: number,
          values: number,
          valuesLen: number,
        ) => (module as ServiceModule).hostSql(sql, op, statement, statementLen, values, valuesLen),
        host_now: () => Date.now() / MS_PER_SECOND,
        host_utc_offset: (secs: number) =>
          -new Date(secs * MS_PER_SECOND).getTimezoneOffset() * SECONDS_PER_MINUTE,
      },
    };
    const response = await fetch(url);
    if (!response.ok)
      throw new Error(`The service could not be loaded: ${url} (${response.status})`);
    const bytes = await response.arrayBuffer();
    const { instance } = await WebAssembly.instantiate(bytes, imports);
    module = new ServiceModule(instance);
    return module;
  }

  /** Opens the service's library with the config (JSON); rejects with the service's reason. */
  async open(config: string): Promise<void> {
    const text = new TextEncoder().encode(config);
    const block = await this.withBytes([text], ([configPtr]) =>
      this.exports.service_open(configPtr, text.length),
    );
    const view = this.view();
    const [code, len] = [view.getUint32(block, true), view.getUint32(block + U32_BYTES, true)];
    const why = new TextDecoder().decode(this.bytes(block + HEADER_BYTES, len));
    this.exports.dealloc(block, HEADER_BYTES + len);
    if (code !== 0) throw new OpenFailed(why || 'The service could not open its library');
  }

  /** One request, answered by api::handle (service/src/api.rs). */
  async handle(request: HandleRequest, body: Uint8Array): Promise<ServiceAnswer> {
    const req = new TextEncoder().encode(JSON.stringify(request));
    const block = await this.withBytes([req, body], ([requestPtr, bodyPtr]) =>
      this.exports.service_handle(requestPtr, req.length, bodyPtr, body.length),
    );
    const view = this.view();
    const status = view.getUint32(block, true);
    const typeLen = view.getUint32(block + U32_BYTES, true);
    const type = new TextDecoder().decode(this.bytes(block + HEADER_BYTES, typeLen));
    const bodyAt = block + HEADER_BYTES + typeLen;
    const bodyLen = view.getUint32(bodyAt, true);
    const out = this.bytes(bodyAt + U32_BYTES, bodyLen).slice();
    this.exports.dealloc(block, HEADER_BYTES + typeLen + U32_BYTES + bodyLen);
    return { status, type, body: out };
  }

  /**
   * Copies the inputs into the module's memory, runs the call (through Asyncify), and frees them;
   * an empty input is passed as no bytes at 0.
   */
  private async withBytes(inputs: Uint8Array[], call: (ptrs: number[]) => number): Promise<number> {
    const ptrs = inputs.map((input) => {
      if (!input.length) return 0;
      const ptr = this.exports.alloc(input.length);
      this.bytes(ptr, input.length).set(input);
      return ptr;
    });
    try {
      return await this.run(() => call(ptrs));
    } finally {
      ptrs.forEach((ptr, index) => {
        if (ptr) this.exports.dealloc(ptr, inputs[index].length);
      });
    }
  }

  /**
   * Runs an export, waiting for each file system call it makes: unwound, awaited, rewound, called
   * again. Gives what the export finally returns.
   */
  private async run(call: () => number): Promise<number> {
    let out = call();
    while (this.exports.asyncify_get_state() === UNWINDING) {
      this.exports.asyncify_stop_unwind();
      const result = await (this.pending as Promise<FsResult>);
      this.pending = null;
      this.answer = this.resultBlock(result);
      this.exports.asyncify_start_rewind(this.data);
      out = call();
    }
    return out;
  }

  /**
   * host_fs: starts the call and unwinds the stack; called again while rewinding, it gives the
   * answer.
   */
  private hostFs(
    fs: HostFs,
    op: number,
    pathPtr: number,
    pathLen: number,
    argPtr: number,
    argLen: number,
  ): number {
    if (this.exports.asyncify_get_state() === REWINDING) {
      this.exports.asyncify_stop_rewind();
      const block = this.answer;
      this.answer = 0;
      return block;
    }
    const path = new TextDecoder().decode(this.bytes(pathPtr, pathLen));
    const arg = this.bytes(argPtr, argLen).slice();
    this.pending = fs(op, path, arg).catch((error: unknown): FsResult => ({
      code: FS_OTHER,
      bytes: new TextEncoder().encode(error instanceof Error ? error.message : String(error)),
    }));
    const view = this.view();
    view.setUint32(this.data, this.data + HEADER_BYTES, true);
    view.setUint32(this.data + U32_BYTES, this.data + HEADER_BYTES + STACK_BYTES, true);
    this.exports.asyncify_start_unwind(this.data);
    // the module ignores what an unwinding call returns
    return 0;
  }

  /**
   * host_sql: the database answers at once (no Asyncify wait), as a result block the module
   * frees.
   */
  private hostSql(
    sql: HostSql,
    op: number,
    statementPtr: number,
    statementLen: number,
    valuesPtr: number,
    valuesLen: number,
  ): number {
    const statement = new TextDecoder().decode(this.bytes(statementPtr, statementLen));
    const values = this.bytes(valuesPtr, valuesLen).slice();
    return this.resultBlock(sql(op, statement, values));
  }

  /** A result block the module frees: [u32 code][u32 len][len bytes]. */
  private resultBlock(result: FsResult): number {
    const block = this.exports.alloc(HEADER_BYTES + result.bytes.length);
    const view = this.view();
    view.setUint32(block, result.code, true);
    view.setUint32(block + U32_BYTES, result.bytes.length, true);
    this.bytes(block + HEADER_BYTES, result.bytes.length).set(result.bytes);
    return block;
  }

  /** A DataView of the module's memory, fresh each time (the memory can grow). */
  private view(): DataView {
    return new DataView(this.exports.memory.buffer);
  }

  /** len bytes of the module's memory at ptr, a fresh view each time. */
  private bytes(ptr: number, len: number): Uint8Array {
    return new Uint8Array(this.exports.memory.buffer, ptr, len);
  }
}
