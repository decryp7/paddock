import { TestBed } from '@angular/core/testing';
import { Sparkline, sparkPath } from './sparkline';

describe('sparkPath', () => {
  it('returns empty paths for no data', () => {
    expect(sparkPath([], 100, 20)).toEqual({ line: '', area: '' });
  });

  it('draws a flat point for a single value and closes the area', () => {
    const p = sparkPath([5], 100, 20);
    expect(p.line).toBe('M0.0,1.0');
    expect(p.area).toBe('M0.0,1.0 L0.0,20 L0,20 Z');
  });

  it('scales to the maximum, spreads points over the width and clamps negatives to the baseline', () => {
    const p = sparkPath([0, 10, -4], 20, 12);
    expect(p.line).toBe('M0.0,11.0 L10.0,1.0 L20.0,11.0');
    expect(p.area.endsWith('L20.0,12 L0,12 Z')).toBe(true);
  });

  it('uses 1 as the minimum scale so tiny values stay near the baseline', () => {
    const p = sparkPath([0.1, 0.1], 10, 12);
    expect(p.line).toBe('M0.0,10.0 L10.0,10.0');
  });
});

describe('Sparkline', () => {
  it('renders the line and area with the given colour and size, hidden from assistive tech', () => {
    const fixture = TestBed.createComponent(Sparkline);
    fixture.componentRef.setInput('values', [1, 2, 3]);
    fixture.componentRef.setInput('color', 'red');
    fixture.componentRef.setInput('width', 50);
    fixture.componentRef.setInput('height', 10);
    fixture.detectChanges();
    const el = fixture.nativeElement as HTMLElement;
    const svg = el.querySelector('svg')!;
    expect(svg.getAttribute('viewBox')).toBe('0 0 50 10');
    expect(svg.getAttribute('width')).toBe('50');
    const [area, line] = Array.from(el.querySelectorAll('path'));
    expect(area!.getAttribute('fill')).toBe('red');
    expect(line!.getAttribute('stroke')).toBe('red');
    expect(line!.getAttribute('d')).toMatch(/^M0\.0,/);
    expect(el.getAttribute('aria-hidden')).toBe('true');
  });

  it('defaults to a 96x24 currentColor line and updates when the values change', () => {
    const fixture = TestBed.createComponent(Sparkline);
    fixture.componentRef.setInput('values', []);
    fixture.detectChanges();
    const el = fixture.nativeElement as HTMLElement;
    expect(el.querySelector('svg')!.getAttribute('viewBox')).toBe('0 0 96 24');
    expect(el.querySelectorAll('path')[1]!.getAttribute('stroke')).toBe('currentColor');
    expect(el.querySelectorAll('path')[1]!.getAttribute('d')).toBe('');
    fixture.componentRef.setInput('values', [1, 3]);
    fixture.detectChanges();
    expect(el.querySelectorAll('path')[1]!.getAttribute('d')).toContain('L96.0');
  });
});
