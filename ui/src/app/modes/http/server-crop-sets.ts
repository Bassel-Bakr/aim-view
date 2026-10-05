import { HttpClient, httpResource, HttpResourceRef } from '@angular/common/http';
import { inject, Service } from '@angular/core';
import { firstValueFrom } from 'rxjs';
import { CropAnswer, CropAnswers, CropEntry, CropPage } from '../../api';
import { CropAnswerMap, CropImport, CropSets } from '../../platform/crop-sets';

/**
 * The check folders on the review server (service/src/crops.rs): test_out/vod_model/check_*, read where they are
 * (/api/crop_pages, /api/crops, /api/crop_image), and each crop's answer kept in the folder's answers/checks/
 * (/api/crop_answers, /api/crop_answer), where python/model/crop_check/labels.py reads it.
 */
@Service()
export class ServerCropSets implements CropSets {
  protected readonly http = inject(HttpClient);
  readonly addFolder: ((files: readonly File[]) => Promise<string>) | null = null;

  pages(): HttpResourceRef<CropPage[] | undefined> {
    return httpResource<CropPage[]>(() => '/api/crop_pages');
  }

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

  image(page: string, id: string): Promise<Blob> {
    const params = { page, id };
    return firstValueFrom(this.http.get('/api/crop_image', { params, responseType: 'blob' }));
  }

  save(page: string, id: string, answer: CropAnswer): Promise<CropAnswer> {
    return firstValueFrom(
      this.http.post<CropAnswer>('/api/crop_answer', answer, { params: { page, id } }),
    );
  }

  exportAnswers(page: string): Promise<CropAnswers> {
    return firstValueFrom(this.http.get<CropAnswers>('/api/crop_export', { params: { page } }));
  }

  importAnswers(page: string, document: CropAnswers): Promise<CropImport> {
    return firstValueFrom(
      this.http.post<CropImport>('/api/crop_import', document, { params: { page } }),
    );
  }
}
