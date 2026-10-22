import { TestBed } from '@angular/core/testing';
import { UsageChart } from './usage-chart';

describe('UsageChart', () => {
  function mount(inputs: Record<string, unknown>) {
    const f = TestBed.createComponent(UsageChart);
    for (const [k, v] of Object.entries(inputs)) f.componentRef.setInput(k, v);
    f.detectChanges();
    return f;
  }

  it('draws a peak line at the right height with its label, and the scale labels', () => {
    const f = mount({ values: [100, 520, 200], max: 1000, peak: 520, peakLabel: 'peak 52%', topLabel: '100%', height: 72 });
    const el = f.nativeElement as HTMLElement;
    expect(el.querySelector('.peak-label')?.textContent).toContain('peak 52%');
    const top = parseFloat((el.querySelector('.peak-line') as HTMLElement).style.top);
    expect(top).toBeCloseTo(((2 + 68 * 0.48) / 72) * 100, 1);
    expect(el.querySelector('.axis-top')?.textContent).toBe('100%');
    expect(el.querySelector('.axis-bottom')?.textContent).toBe('0');
    expect(el.querySelector('path.line')?.getAttribute('d')).toMatch(/^M/);
  });

  it('draws no peak line without a peak or with an empty scale', () => {
    expect((mount({ values: [], max: 1000 }).nativeElement as HTMLElement).querySelector('.peak-line')).toBeNull();
    expect((mount({ values: [1], max: 0, peak: 1 }).nativeElement as HTMLElement).querySelector('.peak-line')).toBeNull();
  });
});
