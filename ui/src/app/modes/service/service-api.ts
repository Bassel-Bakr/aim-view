import {
  HttpErrorResponse,
  HttpEvent,
  HttpEventType,
  HttpHeaders,
  HttpInterceptorFn,
  HttpRequest,
  HttpResponse,
} from '@angular/common/http';
import { inject } from '@angular/core';
import { Observable } from 'rxjs';
import { CopyRequest, FILES } from './mounted-files';
import { ServiceFailure, ServiceHost, TaskProgress } from './service-host';
import { FilesResult } from './service-messages';

/** An answer as the interceptor hands it on: its status, Content-Type and body (bytes, or a file of the mounts). */
interface Answered {
  status: number;
  type: string;
  body: Uint8Array | Blob;
}

const STATUS_TEXT: Record<number, string> = {
  200: 'OK',
  206: 'Partial Content',
  400: 'Bad Request',
  404: 'Not Found',
  409: 'Conflict',
  500: 'Internal Server Error',
  501: 'Not Implemented',
  503: 'Service Unavailable',
};

const JSON_TYPE = 'application/json';

/** A request's body as the service takes it: none, bytes, or a file (an upload's, kept as a Blob). */
function bodyOf(req: HttpRequest<unknown>): Uint8Array | Blob | null {
  const b = req.body;
  if (b === null || b === undefined) return null;
  if (b instanceof Blob || b instanceof Uint8Array) return b;
  if (b instanceof ArrayBuffer) return new Uint8Array(b);
  return new TextEncoder().encode(typeof b === 'string' ? b : JSON.stringify(b));
}

/** An answer's body as the request wants it: JSON, text, a Blob or an ArrayBuffer. */
async function parsed(req: HttpRequest<unknown>, a: Answered): Promise<unknown> {
  if (req.responseType === 'blob')
    return a.body instanceof Blob ? a.body : new Blob([a.body as BlobPart], { type: a.type });
  const bytes = a.body instanceof Blob ? new Uint8Array(await a.body.arrayBuffer()) : a.body;
  if (req.responseType === 'arraybuffer') return bytes.slice().buffer;
  const text = new TextDecoder().decode(bytes);
  if (req.responseType === 'text') return text;
  return jsonOr(text);
}

/** JSON as a value; text that is not JSON as itself; nothing as null. */
function jsonOr(text: string): unknown {
  if (!text) return null;
  try {
    return JSON.parse(text);
  } catch {
    return text;
  }
}

/**
 * Answers a request from a task of the worker's: the request's events (sent, upload progress where asked), then the
 * response, or the error the service answered with (its status and JSON body, as the review server's).
 */
function answered(
  req: HttpRequest<unknown>,
  task: (progress: TaskProgress) => Promise<Answered>,
): Observable<HttpEvent<unknown>> {
  return new Observable<HttpEvent<unknown>>((events) => {
    let open = true;
    events.next({ type: HttpEventType.Sent });
    const progress: TaskProgress = (done, total) => {
      if (open && req.reportProgress)
        events.next({ type: HttpEventType.UploadProgress, loaded: done, total });
    };
    task(progress)
      .then(async (a) => {
        const body = await parsed(req, a);
        if (!open) return;
        const meta = {
          status: a.status,
          statusText: STATUS_TEXT[a.status] ?? '',
          url: req.urlWithParams,
          headers: new HttpHeaders({ 'Content-Type': a.type }),
        };
        if (a.status >= 200 && a.status < 300) {
          events.next(new HttpResponse({ ...meta, body }));
          events.complete();
        } else {
          const text =
            a.body instanceof Blob ? await a.body.text() : new TextDecoder().decode(a.body);
          events.error(new HttpErrorResponse({ ...meta, error: jsonOr(text) }));
        }
      })
      .catch((e: unknown) => {
        if (!open) return;
        const status = e instanceof ServiceFailure ? e.status : 500;
        events.error(
          new HttpErrorResponse({
            status,
            statusText: STATUS_TEXT[status] ?? '',
            url: req.urlWithParams,
            error: { error: e instanceof Error ? e.message : String(e) },
          }),
        );
      });
    return () => (open = false);
  });
}

/** A request to the service: its answer as it gave it. */
function serviceAnswer(
  host: ServiceHost,
  req: HttpRequest<unknown>,
  progress: TaskProgress,
): Promise<Answered> {
  const method = req.method === 'GET' ? 'GET' : 'POST';
  return host.ask(method, req.urlWithParams, bodyOf(req), progress);
}

/** A path below /files as the mounted path it stands for. */
function mountedPath(url: string): string {
  return url
    .slice(FILES.length)
    .split('/')
    .map((n) => decodeURIComponent(n))
    .join('/');
}

/** A JSON answer. */
function json(value: FilesResult): Answered {
  return { status: 200, type: JSON_TYPE, body: new TextEncoder().encode(JSON.stringify(value)) };
}

/**
 * A task with the page's own files in the mounts (/files/<mounted path>): GET a file (the file itself), GET a folder
 * (a path ending in /: its entries, `limit` of them), PUT a file, DELETE a file, POST a copy into a folder; POST
 * /files/kovaak?show=1 shows KovaaK's files chosen this visit at /kovaak at once (no copy).
 */
async function filesAnswer(
  host: ServiceHost,
  req: HttpRequest<unknown>,
  progress: TaskProgress,
): Promise<Answered> {
  const path = mountedPath(req.url);
  if (req.method === 'GET' && path.endsWith('/')) {
    const limit = Number(req.params.get('limit') ?? 0);
    return json(await host.files('list', path.slice(0, -1), { limit }));
  }
  if (req.method === 'GET') {
    const file = (await host.files('read', path)) as File;
    return { status: 200, type: file.type, body: file };
  }
  if (req.method === 'PUT') {
    const body = bodyOf(req) ?? new Uint8Array();
    await host.files('write', path, {
      body: body instanceof Blob ? body : new Blob([body as BlobPart]),
    });
    return json(null);
  }
  if (req.method === 'DELETE') {
    await host.files('remove', path);
    return json(null);
  }
  const files = (req.body as CopyRequest | null)?.files ?? [];
  if (req.params.get('show') === '1') {
    await host.showKovaak(files);
    return json(null);
  }
  return json(await host.files('copy', path, { files }, progress));
}

/**
 * Browser mode's transport: the service's requests (/api/...) are answered by the review service in the page's own
 * worker, and the page's own files in its mounts (/files/...) too. Everything else goes out as usual (a link's video,
 * the local server that downloads links).
 */
export const serviceApi: HttpInterceptorFn = (req, next) => {
  const api = req.url.startsWith('/api/');
  if (!api && !req.url.startsWith(`${FILES}/`)) return next(req);
  const host = inject(ServiceHost);
  return answered(req, (progress) =>
    api ? serviceAnswer(host, req, progress) : filesAnswer(host, req, progress),
  );
};
