import { TestBed } from '@angular/core/testing';
import { MODE as BROWSER } from '../modes/mode.browser';
import { RecordingSource } from '../platform/recording-source';
import { Playback } from '../run/playback';
import { LabelQueue } from './label-queue';
import { Library } from './library';

const NAMES = ['Air - 1 - 2026.10.01-16.23.03.mp4', 'Bounce - 2 - 2026.10.01-17.00.00.mp4'];

/** The queue over two recordings added in the browser, and their ids in the queue's order. */
async function twoRecordings(): Promise<string[]> {
  TestBed.configureTestingModule({ providers: BROWSER.providers });
  const { ids } = await TestBed.inject(RecordingSource).add(NAMES.map((n) => new File(['v'], n)));
  return [...ids].reverse();
}

describe('LabelQueue', () => {
  afterEach(() => history.replaceState(null, '', '/'));

  it('opens the recordings one by one at a frame from the run, and says when it went through them', async () => {
    const ids = await twoRecordings();
    const queue = TestBed.inject(LabelQueue);
    const library = TestBed.inject(Library);
    await queue.start();
    expect(queue.current()).toBe(ids[0]);
    expect(queue.upcoming()).toBe(ids[1]);
    expect(library.selectedId()).toBe(ids[0]);
    expect(TestBed.inject(Playback).startAt).toBe(20);
    queue.next();
    TestBed.tick();
    expect(library.selectedId()).toBe(ids[1]);
    expect(queue.position()).toBe(1);
    queue.next();
    expect(queue.active()).toBe(false);
    expect(queue.note()).toEqual({ text: 'Went through all 2 recordings', failed: false });
  });

  it('skips a recording for good, and moves on when the open one is marked as another game', async () => {
    const ids = await twoRecordings();
    const queue = TestBed.inject(LabelQueue);
    await queue.start();
    await queue.skip();
    expect(queue.current()).toBe(ids[1]);
    await queue.setNotAim(ids[1], true);
    expect(queue.active()).toBe(false);
    await queue.start();
    expect(queue.active()).toBe(false);
    expect(queue.note()?.text).toMatch(/^Every recording has saved areas already/);
  });

  it('ends when another recording is opened', async () => {
    const ids = await twoRecordings();
    const queue = TestBed.inject(LabelQueue);
    await queue.start();
    TestBed.tick();
    TestBed.inject(Library).selectedId.set(ids[1]);
    TestBed.tick();
    expect(queue.active()).toBe(false);
  });
});
