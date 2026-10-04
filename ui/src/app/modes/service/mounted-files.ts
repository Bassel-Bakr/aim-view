import { HttpClient, HttpEventType, HttpResponse } from '@angular/common/http';
import { inject, Service } from '@angular/core';
import { filter, firstValueFrom, lastValueFrom, map, tap } from 'rxjs';
import { ChosenFile, CopyDone, DirEntry } from './service-messages';

/** Where the page's own files in the service's mounts are asked for: /files/<mounted path>. */
export const FILES = '/files';

/** What a copy into a folder of the mounts is sent as (POST /files/<folder>). */
export interface CopyRequest {
  files: ChosenFile[];
}

/** Hears how many files a copy has copied, of how many. */
export type CopyProgress = (done: number, total: number) => void;

/** The address of a mounted path (/data/..., /kovaak/..., /vods/...), each name encoded. */
function filesUrl(path: string): string {
  return FILES + path.split('/').map(encodeURIComponent).join('/');
}

/**
 * Where a recording's video is in the mounts, from its id (as service/src/library/recordings.rs `resolve` reads it in
 * the app's layout): an upload ("uploads/<name>") in /data/uploads, anything else below the VODs folder.
 */
export function recordingPath(id: string): string {
  return id.startsWith('uploads/') ? `/data/${id}` : `/vods/${id}`;
}

/**
 * The page's own files in the review service's mounts, through the worker's queue (service-api.ts answers /files/):
 * a recording's video to play or review, the area finder's files, a mouse log to forget, KovaaK's folders copied in.
 */
@Service()
export class MountedFiles {
  private readonly http = inject(HttpClient);

  /** A file of the mounts. */
  read(path: string): Promise<Blob> {
    return firstValueFrom(this.http.get(filesUrl(path), { responseType: 'blob' }));
  }

  /** A text file of the mounts; null when it is not there (or cannot be read). */
  text(path: string): Promise<string | null> {
    return firstValueFrom(this.http.get(filesUrl(path), { responseType: 'text' })).catch(
      () => null,
    );
  }

  /** A folder's entries, at most `limit` of them (0: all). */
  list(path: string, limit = 0): Promise<DirEntry[]> {
    const params = { limit: String(limit) };
    return firstValueFrom(this.http.get<DirEntry[]>(`${filesUrl(path)}/`, { params }));
  }

  /** Writes a file, making its folders. */
  async write(path: string, body: Blob | string): Promise<void> {
    await firstValueFrom(this.http.put(filesUrl(path), body));
  }

  /** Removes a file. */
  async remove(path: string): Promise<void> {
    await firstValueFrom(this.http.delete(filesUrl(path)));
  }

  /** Copies files into a folder at their paths below it: only those new or changed since the last copy. */
  copyIn(dir: string, files: ChosenFile[], progress: CopyProgress): Promise<CopyDone> {
    const body: CopyRequest = { files };
    return lastValueFrom(
      this.http
        .post<CopyDone>(filesUrl(dir), body, { reportProgress: true, observe: 'events' })
        .pipe(
          tap((e) => {
            if (e.type === HttpEventType.UploadProgress) progress(e.loaded, e.total ?? 0);
          }),
          filter((e): e is HttpResponse<CopyDone> => e.type === HttpEventType.Response),
          map((e) => e.body ?? { copied: 0 }),
        ),
    );
  }
}
