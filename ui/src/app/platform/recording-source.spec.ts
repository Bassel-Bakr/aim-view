import { HttpRequest } from '@angular/common/http';
import { LinkInfo, Recording } from '../api';
import { answer, ApiRoutes, NO_SERVER, recording, Refused } from '../fake-api';
import { DEFAULT_LINK_SERVER, NO_LINK_SERVER } from '../modes/web-files/browser-links';
import { MODE_CASES, setUp } from './contract-case';
import { RecordingSource } from './recording-source';

const NAME = 'Air - 1 - 2026.10.01-16.23.03.mp4';
const STATS_NAME = 'Air - Challenge - 2026.10.01-16.23.05 Stats.csv';
const STATS = 'Kill #,Timestamp\n1,16:22:20.833\n\nKills:,1\nScore:,1\nScenario:,Air\n';

/** A review server that keeps what is sent to it, the way python/server.py does. */
function fakeServer(): ApiRoutes {
  let list: Recording[] = [];
  return {
    '/api/vods': () => list,
    '/api/upload': (req: HttpRequest<unknown>) => {
      const name = req.params.get('name') ?? '';
      const id = req.params.get('id');
      if (id) {
        list = list.map((r) => (r.id === id ? { ...r, stats: true } : r));
        return { id, saved: name, job: { stage: 'none' }, stats: true };
      }
      const added = `uploads/${name}`;
      list = [recording({ id: added, scenario: 'Air', stats: false, analysed: false }), ...list];
      return { id: added, saved: name };
    },
  };
}

const LINK = 'https://www.youtube.com/watch?v=abc';
const LINK_NAME = 'Air - 1 - 2026.10.01-16.23.03.mp4';
const LINK_INFO: LinkInfo = {
  title: 'Air - 1 - 2026.10.01-16.23.03',
  duration: 3,
  formats: [
    { id: '400', width: 2560, height: 1440, fps: 60, codec: 'AV1', size: 4e8 },
    { id: '136', width: 1280, height: 720, fps: 30, codec: 'H.264', size: null },
  ],
};

/** Each route at its path, and at the link server's address too (the browser mode asks it there). */
function anywhere(routes: ApiRoutes): ApiRoutes {
  const out: ApiRoutes = { ...routes };
  for (const [path, route] of Object.entries(routes)) out[`${DEFAULT_LINK_SERVER}${path}`] = route;
  return out;
}

/**
 * A review server that downloads links: it names the recording from the title, answers its id at once, says the
 * download is half done when first asked and done when asked again, then lists the video and streams it. `asked`
 * keeps the bodies of /api/link.
 */
function linkServer(asked: unknown[]): ApiRoutes {
  const id = `uploads/${LINK_NAME}`;
  let list: Recording[] = [];
  let polls = 0;
  const row = recording({ id, scenario: 'Air', stats: false, analysed: false });
  return anywhere({
    // the browser mode sends the video it downloaded to its review service
    '/api/upload': () => {
      list = [row];
      return { id, saved: LINK_NAME };
    },
    '/api/vods': () => list,
    '/api/link/formats': LINK_INFO,
    '/api/link': (req: HttpRequest<unknown>) => {
      asked.push(req.body);
      return { id, saved: LINK_NAME, title: LINK_INFO.title, recording: row };
    },
    '/api/job': () => {
      if (!polls++) return { stage: 'downloading', done: 1, total: 2, link: true };
      list = [row];
      return { stage: 'none' };
    },
    '/video': () => new Blob(['video']),
  });
}

/** Answers the fake server until the recording's video is in (ready) or failed: it asks again every half second. */
async function untilIn(source: RecordingSource, id: string, routes: ApiRoutes): Promise<void> {
  const end = Date.now() + 3000;
  while (source.video(id)?.state === 'downloading' && Date.now() < end) {
    await answer(routes);
    await new Promise((r) => setTimeout(r, 50));
  }
}

for (const mode of MODE_CASES) {
  describe(`RecordingSource (${mode.name} mode)`, () => {
    it('lists an added video with its stats file, ready to play', async () => {
      const source = setUp(mode, RecordingSource);
      const files = [new File(['v'], NAME), new File([STATS], STATS_NAME)];
      const added = await mode.finish(source.add(files), fakeServer());
      expect(added.notStats).toEqual([]);
      const [id] = added.ids;
      expect(source.recordings().find((r) => r.id === id)).toMatchObject({
        scenario: 'Air',
        stats: true,
      });
      expect(source.video(id)?.state).toBe('ready');
      expect(source.problem()).toBeNull();
    });

    it('names a .csv that is not a stats file, and adds the video all the same', async () => {
      const source = setUp(mode, RecordingSource);
      const files = [new File(['v'], NAME), new File(['a,b\n1,2\n'], 'notes.csv')];
      const added = await mode.finish(source.add(files), fakeServer());
      expect(added.notStats).toEqual(['notes.csv']);
      expect(source.recordings().find((r) => r.id === added.ids[0])?.stats).toBe(false);
    });

    it('offers a folder of recordings to open, unless the mode lists its own', () => {
      const source = setUp(mode, RecordingSource);
      const folder = source.folder();
      if (mode.name === 'server') expect(folder).toBeNull();
      else expect(folder).toMatchObject({ label: 'VODs folder', busy: false });
    });

    it('empties the list where it can be cleared', async () => {
      const source = setUp(mode, RecordingSource);
      await mode.finish(source.add([new File(['v'], NAME)]), fakeServer());
      if (!source.clearable) return;
      await source.clear();
      expect(source.recordings()).toEqual([]);
      expect(source.folder()).toMatchObject({ label: 'VODs folder' });
    });

    it('changes a row after a change made elsewhere', async () => {
      const source = setUp(mode, RecordingSource);
      const [id] = (await mode.finish(source.add([new File(['v'], NAME)]), fakeServer())).ids;
      source.patch(id, { analysed: true });
      expect(source.recordings().find((r) => r.id === id)?.analysed).toBe(true);
    });

    it("reads a link's qualities, and lists the recording at once while its video downloads", async () => {
      const source = setUp(mode, RecordingSource);
      const asked: unknown[] = [];
      const routes = linkServer(asked);
      const info = await mode.finish(source.linkInfo(LINK), routes);
      expect(info.formats.map((f) => f.id)).toEqual(['400', '136']);
      const id = await mode.finish(source.addLink(LINK, '400'), routes);
      expect(asked).toEqual([{ url: LINK, format: '400' }]);
      expect(source.recordings()[0]).toMatchObject({ id, scenario: 'Air' });
      expect(source.video(id)).toMatchObject({ state: 'downloading', done: 1, total: 2 });
      await untilIn(source, id, routes);
      expect(source.video(id)?.state).toBe('ready');
      expect(source.recordings().filter((r) => r.id === id)).toHaveLength(1);
    });

    it('says why a link cannot be read', async () => {
      const source = setUp(mode, RecordingSource);
      const why = 'yt-dlp cannot read this link: Private video';
      const routes = anywhere({ '/api/link/formats': new Refused(why) });
      await expect(mode.finish(source.linkInfo(LINK), routes)).rejects.toMatchObject({
        error: { error: why },
      });
    });

    it('in the browser, reads a video file itself, or says to start the server for other links', async () => {
      const source = setUp(mode, RecordingSource);
      if (mode.name !== 'browser') {
        expect(source.linkServer).toBeNull();
        return;
      }
      expect(source.linkServer?.()).toBe(DEFAULT_LINK_SERVER);
      const file = 'https://cdn.example.com/clips/Air%20-%201%20-%202026.10.01-16.23.03.mp4';
      const routes: ApiRoutes = {
        [file]: (req: HttpRequest<unknown>) => (req.method === 'HEAD' ? null : new Blob(['video'])),
        ...fakeServer(),
      };
      const info = await mode.finish(source.linkInfo(file), routes);
      expect(info).toEqual({ title: LINK_NAME, duration: null, formats: [] });
      const id = await mode.finish(source.addLink(file, null), routes);
      await untilIn(source, id, routes);
      expect(source.video(id)?.state).toBe('ready');
      expect(source.recordings()[0]).toMatchObject({ id, scenario: 'Air', score: 1 });
      const none = anywhere({ '/api/link/formats': NO_SERVER });
      await expect(mode.finish(source.linkInfo(LINK), none)).rejects.toThrow(NO_LINK_SERVER);
    });
  });
}
