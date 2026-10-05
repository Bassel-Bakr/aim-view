import { TestBed } from '@angular/core/testing';
import { Playback } from './playback';

describe('the playback speed', () => {
  it('stays the chosen one when another video loads, and follows a change made elsewhere', () => {
    const playback = TestBed.inject(Playback);
    const video = document.createElement('video');
    playback.attach(video);
    playback.setRate(0.5);
    // what a new video's loading does: its speed back to the default
    video.playbackRate = video.defaultPlaybackRate;
    expect(video.playbackRate).toBe(0.5);
    video.playbackRate = 2;
    playback.followRate();
    expect(playback.rate()).toBe(2);
  });
});
