import { describe, expect, it } from 'vitest';
import { formatBytes, formatEta, formatRate, percent } from './ai-format';

describe('ai-format', () => {
  it('shares the common unit formatters', () => {
    expect(formatBytes(812 * 1024 ** 2)).toBe('812 MB');
    expect(formatRate(48 * 1024 ** 2)).toBe('48 MB/s');
    expect(formatEta(130)).toBe('about 2 min left');
    expect(percent(5, 10)).toBe(50);
  });
});
