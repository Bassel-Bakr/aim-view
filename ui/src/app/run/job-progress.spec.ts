import { jobProgress } from './job-progress';

describe('jobProgress', () => {
  it('names the device while it tracks', () => {
    const progress = jobProgress({
      stage: 'tracking',
      done: 1200,
      total: 4000,
      device: 'DirectML',
    });
    expect(progress?.stage).toBe('Tracking the targets on DirectML');
    expect(progress?.count).toBe('1200 / 4000 frames');
    expect(progress?.fraction).toBe(0.3);
  });

  it('names the device when it is done', () => {
    const progress = jobProgress({ stage: 'done', seconds: 12.7, device: 'DirectML and CPU' });
    expect(progress?.stage).toBe('Reviewed on DirectML and CPU');
    expect(progress?.count).toBe('in 12.7 s');
  });

  it('shows no device when the job has none (the browser)', () => {
    expect(jobProgress({ stage: 'tracking', done: 5, total: 10 })?.stage).toBe(
      'Tracking the targets',
    );
  });

  it('leaves the device out of the stages that do not run the detector', () => {
    const progress = jobProgress({ stage: 'camera', done: 5, total: 10, device: 'DirectML' });
    expect(progress?.stage).toBe("Reading the camera's turn");
  });

  it('names the kill check and keeps the bar full once the frames are read', () => {
    const progress = jobProgress({ stage: 'checking', done: 0, total: 10 });
    expect(progress?.stage).toBe('Checking the kills in the frames');
    expect(progress?.fraction).toBe(1);
    expect(jobProgress({ stage: 'linking', done: 0, total: 0 })?.fraction).toBe(1);
  });

  it('shows nothing without a job', () => {
    expect(jobProgress({ stage: 'none' })).toBeNull();
  });
});
