import { HttpRequest, provideHttpClient, withInterceptors } from '@angular/common/http';
import { provideHttpClientTesting } from '@angular/common/http/testing';
import { TestBed } from '@angular/core/testing';
import { served } from '../../fake-api';
import { DESKTOP_API, desktopApi } from './desktop-api';
import { DesktopRecordings } from './desktop-recordings';

/** One upload request as the app's protocol got it. */
interface SentPart {
  at: string | null;
  size: string | null;
  upload: string | null;
  bytes: number;
  asBytes: boolean;
}

/** The answers of the app's protocol: each upload request kept in `sent`, parts joined as the service does. */
function uploads(sent: SentPart[]): Record<string, unknown> {
  return {
    [`${DESKTOP_API}/api/vods`]: [],
    [`${DESKTOP_API}/api/upload`]: (req: HttpRequest<unknown>) => {
      const body = req.body as ArrayBuffer;
      const part: SentPart = {
        at: req.params.get('at'),
        size: req.params.get('size'),
        upload: req.params.get('upload'),
        bytes: body.byteLength,
        asBytes: body instanceof ArrayBuffer,
      };
      sent.push(part);
      const end = Number(part.at ?? 0) + part.bytes;
      if (part.size !== null && end < Number(part.size)) return { upload: '7-0', received: end };
      return { id: 'uploads/run.mp4', saved: 'run.mp4' };
    },
  };
}

describe('DesktopRecordings', () => {
  beforeEach(() =>
    TestBed.configureTestingModule({
      providers: [provideHttpClient(withInterceptors([desktopApi])), provideHttpClientTesting()],
    }),
  );

  it('sends a large video in parts, each after the last, and adds the upload the last one makes', async () => {
    const source = TestBed.inject(DesktopRecordings);
    const mib = 1024 * 1024;
    const video = new File([new Uint8Array(20 * mib)], 'run.mp4', { type: 'video/mp4' });
    const sent: SentPart[] = [];
    const added = await served(source.add([video]), uploads(sent));
    expect(added.ids).toEqual(['uploads/run.mp4']);
    const size = String(20 * mib);
    expect(sent).toEqual([
      { at: '0', size, upload: null, bytes: 8 * mib, asBytes: true },
      { at: String(8 * mib), size, upload: '7-0', bytes: 8 * mib, asBytes: true },
      { at: String(16 * mib), size, upload: '7-0', bytes: 4 * mib, asBytes: true },
    ]);
  });

  it('sends a small file whole, as its bytes', async () => {
    const source = TestBed.inject(DesktopRecordings);
    const video = new File([new Uint8Array(1000)], 'run.mp4', { type: 'video/mp4' });
    const sent: SentPart[] = [];
    await served(source.add([video]), uploads(sent));
    expect(sent).toEqual([{ at: null, size: null, upload: null, bytes: 1000, asBytes: true }]);
  });
});
