/**
 * The page's end of the review service's worker (service.worker.ts). In: the /api requests and the
 * page's file tasks (service-api.ts, vods-folder.ts). Out: each one sent to the worker with a
 * number, and its answer, progress or error routed back to the caller.
 */

import { Service } from '@angular/core';
import {
  ChosenFile,
  FilesOp,
  FilesResult,
  ServiceAnswer,
  ServiceMethod,
  ServiceReply,
  ServiceStart,
  ServiceTask,
  VodsMount,
} from './service-messages';

/** The status a task fails with while the service cannot run. */
const UNAVAILABLE = 503;

/** Hears how far a task is: done of total (an upload's bytes, a copy's files). */
export type TaskProgress = (done: number, total: number) => void;

/** A task the worker has not answered yet: how to end it, and who hears its progress. */
interface Waiting {
  /** Ends the task with its answer or result. */
  resolve: (value: ServiceAnswer | FilesResult) => void;
  /** Ends the task with its error. */
  reject: (e: Error) => void;
  /** Hears the task's progress, when the caller asked. */
  progress?: TaskProgress;
}

/**
 * A task the worker turned down: why, and the status it answers as (404: no such file; 503: no
 * service).
 */
export class ServiceFailure extends Error {
  /** Keeps the status and the message. */
  constructor(
    readonly status: number,
    message: string,
  ) {
    super(message);
  }
}

/** What a file task takes besides its op and path. */
export interface FilesTask {
  /** What a write writes. */
  body?: Blob | null;
  /** What a copy copies. */
  files?: ChosenFile[];
  /** The most entries a list gives (0 or none: all). */
  limit?: number;
}

/**
 * The page's side of the review service in a worker (service.worker.ts): it starts the worker on
 * the first task, sends it the service's requests, the page's own file reads and writes in the
 * mounts, and the VODs folder to mount. The interceptor (service-api.ts) sends every /api request
 * through it.
 */
@Service()
export class ServiceHost {
  /** The service's worker, started on the first task; null until then. */
  private worker: Worker | null = null;
  /** Why the worker stopped (it failed to load, or failed outside a task); null while it runs. */
  private stopped: string | null = null;
  /** The last task's number. */
  private next = 0;
  /** The tasks sent and not yet ended, by number. */
  private readonly waiting = new Map<number, Waiting>();

  /** A request to the service: its answer as the service gave it, error statuses included. */
  ask(
    method: ServiceMethod,
    path: string,
    body: Uint8Array | Blob | null,
    progress?: TaskProgress,
  ): Promise<ServiceAnswer> {
    return this.send(
      (id) => ({ kind: 'ask', id, method, path, body }),
      progress,
    ) as Promise<ServiceAnswer>;
  }

  /** One of the page's own file tasks in the mounts. */
  files(
    op: FilesOp,
    path: string,
    task: FilesTask = {},
    progress?: TaskProgress,
  ): Promise<FilesResult> {
    return this.send(
      (id) => ({
        kind: 'files',
        id,
        op,
        path,
        body: task.body ?? null,
        files: task.files ?? [],
        limit: task.limit ?? 0,
      }),
      progress,
    ) as Promise<FilesResult>;
  }

  /** Mounts the VODs folder at /vods (null: none). */
  async mount(vods: VodsMount | null): Promise<void> {
    await this.send((id) => ({ kind: 'mount', id, vods }));
  }

  /**
   * Shows KovaaK's files chosen this visit at /kovaak at once, over the copies this browser keeps.
   */
  async showKovaak(files: ChosenFile[]): Promise<void> {
    await this.send((id) => ({ kind: 'kovaak', id, files }));
  }

  /**
   * Sends a task, made with its new number, to the worker; resolves with its answer or result.
   * Rejects with a `ServiceFailure` once the worker has stopped.
   */
  private send(
    task: (id: number) => ServiceTask,
    progress?: TaskProgress,
  ): Promise<ServiceAnswer | FilesResult> {
    return new Promise((resolve, reject) => {
      const worker = this.started();
      const id = ++this.next;
      this.waiting.set(id, { resolve, reject, progress });
      worker.postMessage(task(id));
    });
  }

  /** The worker, started once: it opens the service with the app's models and shipped data. */
  private started(): Worker {
    if (this.stopped !== null) throw new ServiceFailure(UNAVAILABLE, this.stopped);
    if (this.worker) return this.worker;
    if (typeof Worker === 'undefined')
      throw new ServiceFailure(
        UNAVAILABLE,
        'This browser cannot run the review service: it has no workers',
      );
    const worker = new Worker(new URL('./service.worker', import.meta.url), { type: 'module' });
    worker.onmessage = (event: MessageEvent<ServiceReply>) => this.hear(event.data);
    worker.onerror = (event) => {
      this.stopped = `The review service stopped: ${event.message || 'its worker failed'}`;
      for (const waiter of this.waiting.values())
        waiter.reject(new ServiceFailure(UNAVAILABLE, this.stopped));
      this.waiting.clear();
    };
    const base = new URL(document.baseURI);
    const start: ServiceStart = {
      kind: 'start',
      wasmUrl: new URL('service/aimview_service.wasm', base).href,
      sqliteUrl: new URL('service/sqlite3.wasm', base).href,
      modelsUrl: new URL('models/', base).href,
      dataUrl: new URL('data/', base).href,
    };
    worker.postMessage(start);
    this.worker = worker;
    // the reviews, areas and copies live in this browser's storage: ask it to keep them when the
    // disk runs low (the browser decides; Chrome grants it without asking for a site the user uses)
    void navigator.storage?.persist?.().catch(() => false);
    return worker;
  }

  /** Routes a reply to its task: progress to its listener, else the task's end. */
  private hear(reply: ServiceReply): void {
    const waiter = this.waiting.get(reply.id);
    if (!waiter) return;
    if (reply.kind === 'progress') {
      waiter.progress?.(reply.done, reply.total);
      return;
    }
    this.waiting.delete(reply.id);
    if (reply.kind === 'answer') waiter.resolve(reply.answer);
    else if (reply.kind === 'done') waiter.resolve(reply.result);
    else waiter.reject(new ServiceFailure(reply.status, reply.error));
  }
}
