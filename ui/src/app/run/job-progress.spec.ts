import { jobProgress } from './job-progress';

describe('jobProgress', () => {
  it('names the device while it tracks', () => {
    const p = jobProgress({ stage: 'tracking', done: 1200, total: 4000, device: 'DirectML' });
    expect(p?.stage).toBe('Tracking the targets on DirectML');
    expect(p?.count).toBe('1200 / 4000 frames');
    expect(p?.fraction).toBe(0.3);
  });

  it('names the device when it is done', () => {
    const p = jobProgress({ stage: 'done', seconds: 12.7, device: 'DirectML and CPU' });
    expect(p?.stage).toBe('Reviewed on DirectML and CPU');
    expect(p?.count).toBe('in 12.7 s');
  });

  it('shows no device when the job has none (the browser)', () => {
    expect(jobProgress({ stage: 'tracking', done: 5, total: 10 })?.stage).toBe(
      'Tracking the targets',
    );
  });

  it('leaves the device out of the stages that do not run the detector', () => {
    const p = jobProgress({ stage: 'camera', done: 5, total: 10, device: 'DirectML' });
    expect(p?.stage).toBe("Reading the camera's turn");
  });

  it('shows nothing without a job', () => {
    expect(jobProgress({ stage: 'none' })).toBeNull();
  });
});
