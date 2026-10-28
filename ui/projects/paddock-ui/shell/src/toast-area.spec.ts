import { describe, expect, it } from 'vitest';
import { ToastArea } from './toast-area';

describe('ToastArea', () => {
  it('holds the height the notification host keeps free, and counts nonsense as none', () => {
    const area = new ToastArea();
    expect(area.lift()).toBe(0);
    area.setLift(48);
    expect(area.lift()).toBe(48);
    for (const bad of [-1, Number.NaN, Number.POSITIVE_INFINITY]) {
      area.setLift(bad);
      expect(area.lift()).toBe(0);
    }
  });
});
