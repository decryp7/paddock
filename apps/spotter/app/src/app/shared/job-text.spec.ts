import type { JobProgress } from '../core/api.types';
import { jobFraction, jobNumbers } from './job-text';

const p = (over: Partial<JobProgress>): JobProgress => ({ phase: 'downloading', label: 'Downloading', unit: 'bytes', done: 0, total: null, ...over });

describe('job text', () => {
  it('says bytes on their own scale, with the total when it is known', () => {
    expect(jobNumbers(p({ done: 5 * 1024 * 1024, total: 20 * 1024 * 1024 }))).toBe('5.0 MiB of 20.0 MiB');
    expect(jobNumbers(p({ done: 2048, total: null }))).toBe('2.0 KiB');
  });

  it('says items as "n of m" or as a growing count when the total is not known', () => {
    expect(jobNumbers(p({ phase: 'checking', label: 'Checking', unit: 'count', done: 3, total: 12 }))).toBe('3 of 12');
    expect(jobNumbers(p({ phase: 'finding', label: 'Finding files', unit: 'count', done: 12345, total: null }))).toBe(`${(12345).toLocaleString()} found`);
    expect(jobNumbers(p({ phase: 'selfTest', label: 'Testing', unit: 'count', done: 0, total: null }))).toBe('');
    expect(jobNumbers(p({ phase: 'settling', label: 'Waiting for files to settle', unit: 'count', done: 2, total: 5 }))).toBe('2 of 5 s');
  });

  it('has a fraction only when the total is known and not zero', () => {
    expect(jobFraction(p({ done: 5, total: 20 }))).toBe(0.25);
    expect(jobFraction(p({ done: 50, total: 20 }))).toBe(1);
    expect(jobFraction(p({ done: 5, total: null }))).toBeNull();
    expect(jobFraction(p({ done: 0, total: 0 }))).toBeNull();
    expect(jobFraction(null)).toBeNull();
  });
});
