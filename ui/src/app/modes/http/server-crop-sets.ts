/**
 * Server mode's `CropSets`, which the desktop app uses too and browser mode extends. In: the review
 * service's crop routes (/api/crop_pages, /api/crops, /api/crop_image and the answer routes). Out:
 * the check folders, their crops and the user's answers, for the Crops page.
 */

import { HttpClient, httpResource, HttpResourceRef } from '@angular/common/http';
import { inject, Service } from '@angular/core';
import { firstValueFrom } from 'rxjs';
import { CropAnswer, CropAnswers, CropEntry, CropPage } from '../../api';
import { CropAnswerMap, CropImport, CropSets } from '../../platform/crop-sets';

/**
 * The check folders on the review server (service/src/crops.rs): test_out/vod_model/check_*, read
 * where they are (/api/crop_pages, /api/crops, /api/crop_image), and each crop's answer kept in the
 * folder's answers/checks/ (/api/crop_answers, /api/crop_answer), where
 * python/model/crop_check/labels.py reads it.
 */
@Service()
export class ServerCropSets implements CropSets {
  /** Sends the image, answer, export and import requests. */
  protected readonly http = inject(HttpClient);
  /** The server reads the check folders where they are, so the page adds none. */
  readonly addFolder: ((files: readonly File[]) => Promise<string>) | null = null;

  /** Every check folder and its sets (GET /api/crop_pages). */
  pages(): HttpResourceRef<CropPage[] | undefined> {
    return httpResource<CropPage[]>(() => '/api/crop_pages');
  }

  /** A set's crops (GET /api/crops), once the folder and the set are known. */
  crops(
    page: () => string | undefined,
    set: () => string | undefined,
  ): HttpResourceRef<CropEntry[] | undefined> {
    return httpResource<CropEntry[]>(() => {
      const [folder, name] = [page(), set()];
      return folder && name
        ? { url: '/api/crops', params: { page: folder, set: name } }
        : undefined;
    });
  }

  /** A set's answers by crop id (GET /api/crop_answers), once the folder and the set are known. */
  answers(
    page: () => string | undefined,
    set: () => string | undefined,
  ): HttpResourceRef<CropAnswerMap | undefined> {
    return httpResource<CropAnswerMap>(() => {
      const [folder, name] = [page(), set()];
      return folder && name
        ? { url: '/api/crop_answers', params: { page: folder, set: name } }
        : undefined;
    });
  }

  /** A crop's picture as the service reads it from the folder (GET /api/crop_image). */
  image(page: string, id: string): Promise<Blob> {
    const params = { page, id };
    return firstValueFrom(this.http.get('/api/crop_image', { params, responseType: 'blob' }));
  }

  /** Keeps a crop's answer in the folder (POST /api/crop_answer); gives it as kept. */
  save(page: string, id: string, answer: CropAnswer): Promise<CropAnswer> {
    return firstValueFrom(
      this.http.post<CropAnswer>('/api/crop_answer', answer, { params: { page, id } }),
    );
  }

  /** Every answer of the check folder as one document (GET /api/crop_export). */
  exportAnswers(page: string): Promise<CropAnswers> {
    return firstValueFrom(this.http.get<CropAnswers>('/api/crop_export', { params: { page } }));
  }

  /** Sends an exported document to the service (POST /api/crop_import); a newer answer wins. */
  importAnswers(page: string, document: CropAnswers): Promise<CropImport> {
    return firstValueFrom(
      this.http.post<CropImport>('/api/crop_import', document, { params: { page } }),
    );
  }
}
