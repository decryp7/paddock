import { Component, input, output } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { recorderSamples } from '../../core/mock/alarms';
import { Chart } from '../../shared/chart';
import { TimelinePanel } from './timeline-panel';


/** jsdom has no canvas: the chart is replaced by an element with the same selector, inputs and outputs. */
@Component({ selector: 'dy-chart', template: '', host: { '[attr.aria-label]': 'label()' } })
class StubChart {
  readonly options = input<unknown>(null);
  readonly label = input('');
  readonly itemClick = output<unknown>();
}
const stubChart = () => TestBed.overrideComponent(TimelinePanel, { remove: { imports: [Chart] }, add: { imports: [StubChart] } });

function make(moment = 3) {
  const f = TestBed.createComponent(TimelinePanel);
  f.componentRef.setInput('snapshot', { version: 1, at: 1_700_000_000_000, reason: 'manual', samples: recorderSamples(1_700_000_000_000, 20) });
  f.componentRef.setInput('moment', moment);
  f.detectChanges();
  return f;
}

describe('dy-timeline-panel', () => {
  beforeEach(() => stubChart());
  it('reads out the picked moment and its top processes', () => {
    const el = make().nativeElement as HTMLElement;
    expect(el.querySelectorAll('.heading')).toHaveLength(5);
    expect(el.querySelectorAll('tbody tr')).toHaveLength(3);
    expect(el.querySelector('.at')?.textContent).toContain('CPU');
  });

  it('turns a chart click into a picked moment and ignores clicks without one', () => {
    const f = make();
    const picked: number[] = [];
    f.componentInstance.picked.subscribe((i) => picked.push(i));
    const c = f.componentInstance as unknown as { onClick(p: { name?: string }): void };
    c.onClick({ name: '12' });
    c.onClick({ name: 'x' });
    c.onClick({});
    expect(picked).toEqual([12]);
  });

  it('says so when a second has no process detail', () => {
    const f = TestBed.createComponent(TimelinePanel);
    const samples = recorderSamples(1_700_000_000_000, 5).map((s) => ({ ...s, top: [] }));
    f.componentRef.setInput('snapshot', { version: 1, at: 1, reason: 'manual', samples });
    f.componentRef.setInput('moment', 0);
    f.detectChanges();
    expect((f.nativeElement as HTMLElement).textContent).toContain('No process detail');
  });
});
