import { TestBed } from '@angular/core/testing';
import { describe, expect, it, vi } from 'vitest';
import { GridMetrics } from './grid-metrics';
import { DEFAULT_CHAR_PX } from './message-column';

describe('GridMetrics', () => {
  it('starts at the default and keeps the width of one character measured on the cell\'s font', () => {
    const metrics = TestBed.inject(GridMetrics);
    expect(metrics.charPx()).toBe(DEFAULT_CHAR_PX);
    const cell = document.createElement('span');
    document.body.append(cell);
    const seen: string[] = [];
    const canvas = { getContext: () => ({ set font(f: string) { seen.push(f); }, measureText: (t: string) => ({ width: t.length * 7.8 }) }) } as unknown as HTMLCanvasElement;
    cell.style.fontSize = '13px';
    metrics.measure(cell, canvas);
    expect(metrics.charPx()).toBe(7.8);
    expect(seen[0]).toContain('13px');
    cell.remove();
  });

  it('ignores a measurement that is not a width', () => {
    const metrics = TestBed.inject(GridMetrics);
    const cell = document.createElement('span');
    document.body.append(cell);
    const canvas = { getContext: () => ({ font: '', measureText: () => ({ width: Number.NaN }) }) } as unknown as HTMLCanvasElement;
    metrics.measure(cell, canvas);
    expect(metrics.charPx()).toBe(DEFAULT_CHAR_PX);
    cell.remove();
    vi.restoreAllMocks();
  });
});
