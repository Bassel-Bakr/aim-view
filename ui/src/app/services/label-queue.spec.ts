import { HttpRequest } from '@angular/common/http';
import { TestBed } from '@angular/core/testing';
import { ApiRoutes, recording, served, serverMode } from '../fake-api';
import { Playback } from '../run/playback';
import { LabelQueue } from './label-queue';
import { Library } from './library';

const IDS = [
  'uploads/Air - 1 - 2026.10.01-16.23.03.mp4',
  'uploads/Bounce - 2 - 2026.10.01-17.00.00.mp4',
];

/**
 * A review server with two recordings that keeps skips and other games, the way the service does: its queue is the
 * recordings neither skipped nor another game, in the list's order.
 */
function fakeServer(): ApiRoutes {
  const skipped = new Set<string>();
  const other = new Set<string>();
  const list = IDS.map((id) =>
    recording({ id, scenario: id.slice(8).split(' - ')[0], uploaded: true }),
  );
  const id = (req: HttpRequest<unknown>) => req.params.get('id') ?? '';
  return {
    '/api/vods': () => list.map((r) => ({ ...r, not_aim: other.has(r.id) })),
    '/api/label_queue': () => IDS.filter((r) => !skipped.has(r) && !other.has(r)),
    '/api/label_skip': (req: HttpRequest<unknown>) => {
      skipped.add(id(req));
      return { id: id(req), skipped: true };
    },
    '/api/not_aim': (req: HttpRequest<unknown>) => {
      const on = req.params.get('on') === '1';
      if (on) other.add(id(req));
      else other.delete(id(req));
      return { id: id(req), not_aim: on };
    },
  };
}

describe('LabelQueue', () => {
  let routes: ApiRoutes;
  beforeEach(() => {
    routes = fakeServer();
    TestBed.configureTestingModule({ providers: serverMode() });
  });
  afterEach(() => history.replaceState(null, '', '/'));

  it('opens the recordings one by one at a frame from the run, and says when it went through them', async () => {
    const queue = TestBed.inject(LabelQueue);
    const library = TestBed.inject(Library);
    await served(queue.start(), routes);
    expect(queue.current()).toBe(IDS[0]);
    expect(queue.upcoming()).toBe(IDS[1]);
    expect(library.selectedId()).toBe(IDS[0]);
    expect(TestBed.inject(Playback).startAt).toBe(20);
    queue.next();
    TestBed.tick();
    expect(library.selectedId()).toBe(IDS[1]);
    expect(queue.position()).toBe(1);
    queue.next();
    expect(queue.active()).toBe(false);
    expect(queue.note()).toEqual({ text: 'Went through all 2 recordings', failed: false });
  });

  it('skips a recording for good, and moves on when the open one is marked as another game', async () => {
    const queue = TestBed.inject(LabelQueue);
    await served(queue.start(), routes);
    await served(queue.skip(), routes);
    expect(queue.current()).toBe(IDS[1]);
    await served(queue.setNotAim(IDS[1], true), routes);
    expect(queue.active()).toBe(false);
    await served(queue.start(), routes);
    expect(queue.active()).toBe(false);
    expect(queue.note()?.text).toMatch(/^Every recording has saved areas already/);
  });

  it('ends when another recording is opened', async () => {
    const queue = TestBed.inject(LabelQueue);
    await served(queue.start(), routes);
    TestBed.tick();
    TestBed.inject(Library).selectedId.set(IDS[1]);
    TestBed.tick();
    expect(queue.active()).toBe(false);
  });
});
