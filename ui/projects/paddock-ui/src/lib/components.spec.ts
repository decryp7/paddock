import { Component, signal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { SuCard } from './card';
import { SuMetric, deltaTone, formatNumber } from './metric';
import { SuSegmentedControl, nextEnabled, type SuSegment } from './segmented-control';
import { SuSkeleton } from './skeleton';
import { SuSparkline, sparklineGeometry } from './sparkline';
import { SU_STATUSES } from './status';
import { SuStatusPill } from './status-pill';
import { SuEmptyState, SuErrorState } from './states';
import { SuTrayPopover } from './tray-popover';

function text(el: Element): string {
  return (el.textContent ?? '').replace(/\s+/g, ' ').trim();
}

describe('SuStatusPill', () => {
  it.each(SU_STATUSES)('shows word, icon and the %s class', (status) => {
    const f = TestBed.createComponent(SuStatusPill);
    f.componentRef.setInput('status', status);
    f.componentRef.setInput('label', 'Word');
    f.detectChanges();
    const el = f.nativeElement as HTMLElement;
    expect(el.className).toContain(`pd-pill--${status}`);
    expect(text(el)).toBe('Word');
    expect(el.querySelector('svg')?.getAttribute('aria-hidden')).toBe('true');
    expect(el.querySelector('path')?.getAttribute('d')).toBeTruthy();
  });

  it('uses a different icon shape for every status (never colour alone)', () => {
    const shapes = SU_STATUSES.map((status) => {
      const f = TestBed.createComponent(SuStatusPill);
      f.componentRef.setInput('status', status);
      f.componentRef.setInput('label', 'x');
      f.detectChanges();
      return (f.nativeElement as HTMLElement).querySelector('path')?.getAttribute('d');
    });
    expect(new Set(shapes).size).toBe(SU_STATUSES.length);
  });

  it('defaults to neutral', () => {
    const f = TestBed.createComponent(SuStatusPill);
    f.componentRef.setInput('label', 'Paused');
    f.detectChanges();
    expect((f.nativeElement as HTMLElement).className).toContain('pd-pill--neutral');
  });
});

describe('metric helpers', () => {
  it('deltaTone is neutral without a good direction or without change', () => {
    expect(deltaTone(5, 'none')).toBe('neutral');
    expect(deltaTone(0, 'up')).toBe('neutral');
  });
  it('deltaTone is good when the change goes the good way and bad otherwise', () => {
    expect(deltaTone(2, 'up')).toBe('good');
    expect(deltaTone(-2, 'down')).toBe('good');
    expect(deltaTone(2, 'down')).toBe('bad');
    expect(deltaTone(-2, 'up')).toBe('bad');
  });
  it('formatNumber groups thousands, fixes decimals and shows a dash for non-finite values', () => {
    expect(formatNumber(1234567, undefined, 'en-US')).toBe('1,234,567');
    expect(formatNumber(1.5, 2, 'en-US')).toBe('1.50');
    expect(formatNumber(1.23456, undefined, 'en-US')).toBe('1.23');
    expect(formatNumber(Number.NaN, 1)).toBe('—');
  });
});

describe('SuMetric', () => {
  function make(inputs: Record<string, unknown>): HTMLElement {
    const f = TestBed.createComponent(SuMetric);
    for (const [k, v] of Object.entries(inputs)) f.componentRef.setInput(k, v);
    f.detectChanges();
    return f.nativeElement as HTMLElement;
  }

  it('renders value and unit as siblings, tabular', () => {
    const el = make({ value: 17.1, unit: 'GB', decimals: 1, locale: 'en-US' });
    expect(el.querySelector('.pd-value')?.textContent).toBe('17.1');
    expect(el.querySelector('.pd-unit')?.textContent).toBe('GB');
    expect(el.className).toContain('pd-metric--display');
    expect(el.querySelector('.pd-delta')).toBeNull();
  });

  it('shows a dash for null and passes strings through', () => {
    expect(make({ value: null }).querySelector('.pd-value')?.textContent).toBe('—');
    expect(make({ value: 'n/a' }).querySelector('.pd-value')?.textContent).toBe('n/a');
  });

  it('renders a positive delta with arrow, sign, label and a spoken description', () => {
    const el = make({ value: 3, delta: 1.5, deltaUnit: '%', deltaLabel: 'vs last week', goodDirection: 'up', locale: 'en-US' });
    const delta = el.querySelector('.pd-delta') as HTMLElement;
    expect(delta.className).toContain('pd-delta--good');
    expect(text(delta)).toContain('+1.5%');
    expect(text(delta)).toContain('vs last week');
    expect(el.querySelector('.pd-visually-hidden')?.textContent).toBe('Up 1.5% vs last week');
  });

  it('renders a negative delta with a minus sign and a bad tone when down is not good', () => {
    const el = make({ value: 3, delta: -2, goodDirection: 'up', locale: 'en-US' });
    expect(el.querySelector('.pd-delta')?.className).toContain('pd-delta--bad');
    expect(text(el.querySelector('.pd-delta') as Element)).toContain('−2');
    expect(el.querySelector('.pd-visually-hidden')?.textContent).toBe('Down 2');
  });

  it('renders no change without a sign', () => {
    const el = make({ value: 3, delta: 0, locale: 'en-US' });
    expect(el.querySelector('.pd-visually-hidden')?.textContent).toBe('No change');
    expect(el.querySelector('.pd-delta')?.className).toContain('pd-delta--neutral');
  });
});

describe('sparklineGeometry', () => {
  it('returns nothing for no finite values', () => {
    expect(sparklineGeometry([], 100, 30)).toEqual({ line: '', area: '', end: null });
    expect(sparklineGeometry([Number.NaN], 100, 30).line).toBe('');
  });
  it('scales to the box with padding and puts the end point at the last value', () => {
    const g = sparklineGeometry([0, 10], 100, 30, 3);
    expect(g.line).toBe('M3.0 27.0L97.0 3.0');
    expect(g.end).toEqual({ x: 97, y: 3 });
    expect(g.area.endsWith('Z')).toBe(true);
  });
  it('honours a fixed scale shared across sparklines', () => {
    const g = sparklineGeometry([5, 5], 100, 30, 3, 0, 10);
    expect(g.end?.y).toBeCloseTo(15, 5);
  });
  it('a single value and a flat series do not divide by zero', () => {
    expect(sparklineGeometry([4], 100, 30).line).toBe('M3.0 15.0');
    expect(sparklineGeometry([4, 4, 4], 100, 30).line).toContain('M');
  });
  it('breaks the line at a gap instead of connecting across it', () => {
    const g = sparklineGeometry([1, Number.NaN, 3], 100, 30);
    expect(g.line.match(/M/g)?.length).toBe(2);
  });
  it('a trailing gap has no end dot', () => {
    expect(sparklineGeometry([1, 2, Number.NaN], 100, 30).end).toBeNull();
  });
});

describe('SuSparkline', () => {
  function make(inputs: Record<string, unknown>): HTMLElement {
    const f = TestBed.createComponent(SuSparkline);
    for (const [k, v] of Object.entries(inputs)) f.componentRef.setInput(k, v);
    f.detectChanges();
    return f.nativeElement as HTMLElement;
  }
  it('is an image with latest and peak in its name', () => {
    const el = make({ values: [1, 9, 4], label: 'CPU' });
    expect(el.querySelector('svg')?.getAttribute('aria-label')).toBe('CPU: latest 4, peak 9');
    expect(el.querySelector('svg')?.getAttribute('role')).toBe('img');
    expect(el.querySelector('.pd-dot')).not.toBeNull();
    expect(el.querySelector('.pd-area')).toBeNull();
  });
  it('draws the area when asked', () => {
    expect(make({ values: [1, 2], area: true }).querySelector('.pd-area')).not.toBeNull();
  });
  it('says there is no data for an empty series', () => {
    const el = make({ values: [], label: 'CPU' });
    expect(el.querySelector('svg')?.getAttribute('aria-label')).toBe('CPU: no data');
    expect(el.querySelector('.pd-dot')).toBeNull();
    expect(el.querySelector('.pd-area')).toBeNull();
  });
});

@Component({
  imports: [SuCard],
  template: `
    <pd-card heading="CPU" [interactive]="interactive()" [flat]="flat()" (activated)="count.set(count() + 1)">
      <span class="hero">45</span>
      <span slot="caption">peak 61%</span>
      <span slot="status">ok</span>
    </pd-card>
  `,
})
class CardHost {
  readonly interactive = signal(false);
  readonly flat = signal(false);
  readonly count = signal(0);
}

describe('SuCard', () => {
  function make(): { f: ReturnType<typeof TestBed.createComponent<CardHost>>; card: HTMLElement } {
    const f = TestBed.createComponent(CardHost);
    f.detectChanges();
    return { f, card: (f.nativeElement as HTMLElement).querySelector('pd-card') as HTMLElement };
  }

  it('is a labelled group with title, hero, caption and status slots', () => {
    const { card } = make();
    expect(card.getAttribute('role')).toBe('group');
    const title = card.querySelector('h3') as HTMLElement;
    expect(card.getAttribute('aria-labelledby')).toBe(title.id);
    expect(title.textContent).toBe('CPU');
    expect(card.querySelector('.pd-hero')?.textContent).toBe('45');
    expect(card.querySelector('.pd-caption')?.textContent).toContain('peak 61%');
    expect(card.querySelector('.pd-status')?.textContent).toContain('ok');
    expect(card.getAttribute('tabindex')).toBeNull();
    expect(card.querySelector('.pd-chevron')).toBeNull();
  });

  it('does not emit when it is not interactive', () => {
    const { f, card } = make();
    card.click();
    expect(f.componentInstance.count()).toBe(0);
  });

  it('becomes a focusable button that activates by click, Enter and Space', () => {
    const { f, card } = make();
    f.componentInstance.interactive.set(true);
    f.detectChanges();
    expect(card.getAttribute('role')).toBe('button');
    expect(card.getAttribute('tabindex')).toBe('0');
    expect(card.getAttribute('aria-label')).toBe('CPU');
    expect(card.querySelector('.pd-chevron')).not.toBeNull();
    card.click();
    card.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter' }));
    card.dispatchEvent(new KeyboardEvent('keydown', { key: ' ' }));
    expect(f.componentInstance.count()).toBe(3);
  });

  it('flat drops the border', () => {
    const { f, card } = make();
    f.componentInstance.flat.set(true);
    f.detectChanges();
    expect(card.classList).toContain('pd-card--flat');
  });

  it('gives every card a unique title id', () => {
    const a = make().card.querySelector('h3')?.id;
    const b = make().card.querySelector('h3')?.id;
    expect(a).not.toBe(b);
  });
});

describe('SuEmptyState', () => {
  it('shows the heading, the description and a default icon', () => {
    const f = TestBed.createComponent(SuEmptyState);
    f.componentRef.setInput('heading', 'No runners yet');
    f.componentRef.setInput('description', 'Register one.');
    f.detectChanges();
    const el = f.nativeElement as HTMLElement;
    expect(el.getAttribute('role')).toBe('status');
    expect(el.querySelector('h3')?.textContent).toBe('No runners yet');
    expect(el.querySelector('.pd-description')?.textContent).toBe('Register one.');
    expect(el.querySelector('svg')).not.toBeNull();
  });
  it('omits the description when empty', () => {
    const f = TestBed.createComponent(SuEmptyState);
    f.componentRef.setInput('heading', 'Nothing');
    f.detectChanges();
    expect((f.nativeElement as HTMLElement).querySelector('.pd-description')).toBeNull();
  });
});

describe('SuErrorState', () => {
  function make(inputs: Record<string, unknown>) {
    const f = TestBed.createComponent(SuErrorState);
    for (const [k, v] of Object.entries(inputs)) f.componentRef.setInput(k, v);
    f.detectChanges();
    return f;
  }
  it('is an alert with what failed, the cause and Retry', () => {
    const f = make({ heading: "Can't reach the server", cause: 'Check the URL.' });
    const el = f.nativeElement as HTMLElement;
    expect(el.getAttribute('role')).toBe('alert');
    expect(el.querySelector('h3')?.textContent).toBe("Can't reach the server");
    expect(el.querySelector('.pd-cause')?.textContent).toBe('Check the URL.');
    expect(el.querySelector('.pd-retry')?.textContent).toBe('Retry');
    expect(el.querySelector('.pd-secondary')).toBeNull();
  });
  it('emits retry and secondary, and disables Retry while busy', () => {
    const f = make({ heading: 'x', secondaryLabel: 'Open logs' });
    const el = f.nativeElement as HTMLElement;
    let retries = 0;
    let secondaries = 0;
    f.componentInstance.retry.subscribe(() => retries++);
    f.componentInstance.secondary.subscribe(() => secondaries++);
    (el.querySelector('.pd-retry') as HTMLButtonElement).click();
    (el.querySelector('.pd-secondary') as HTMLButtonElement).click();
    expect([retries, secondaries]).toEqual([1, 1]);
    f.componentRef.setInput('busy', true);
    f.detectChanges();
    expect((el.querySelector('.pd-retry') as HTMLButtonElement).disabled).toBe(true);
  });
  it('omits the cause when empty and honours a custom retry label', () => {
    const f = make({ heading: 'x', retryLabel: 'Try again' });
    const el = f.nativeElement as HTMLElement;
    expect(el.querySelector('.pd-cause')).toBeNull();
    expect(el.querySelector('.pd-retry')?.textContent).toBe('Try again');
  });
});

describe('SuSkeleton', () => {
  function make(inputs: Record<string, unknown>): HTMLElement {
    const f = TestBed.createComponent(SuSkeleton);
    for (const [k, v] of Object.entries(inputs)) f.componentRef.setInput(k, v);
    f.detectChanges();
    return f.nativeElement as HTMLElement;
  }
  it('is hidden from assistive tech and has a shape class', () => {
    const el = make({});
    expect(el.getAttribute('aria-hidden')).toBe('true');
    expect(el.className).toContain('pd-skeleton--text');
    expect(el.style.width).toBe('100%');
    expect(el.style.height).toBe('12px');
  });
  it('takes numbers as px and strings as CSS lengths; blocks default to 32px high', () => {
    const el = make({ shape: 'block', width: 120, height: '3rem' });
    expect(el.style.width).toBe('120px');
    expect(el.style.height).toBe('3rem');
    expect(make({ shape: 'circle' }).style.height).toBe('32px');
  });
});

describe('nextEnabled', () => {
  const o: SuSegment[] = [{ value: 'a', label: 'A' }, { value: 'b', label: 'B', disabled: true }, { value: 'c', label: 'C' }];
  it('skips disabled segments and wraps', () => {
    expect(nextEnabled(o, 0, 1)).toBe(2);
    expect(nextEnabled(o, 2, 1)).toBe(0);
    expect(nextEnabled(o, 0, -1)).toBe(2);
    expect(nextEnabled(o, 2, -1)).toBe(0);
  });
  it('returns -1 when nothing is enabled', () => {
    expect(nextEnabled([{ value: 'a', label: 'A', disabled: true }], 0, 1)).toBe(-1);
    expect(nextEnabled([], 0, 1)).toBe(-1);
  });
});

describe('SuSegmentedControl', () => {
  const options: SuSegment[] = [
    { value: '1h', label: '1 h' },
    { value: '24h', label: '24 h', count: 7 },
    { value: 'x', label: 'Off', disabled: true },
    { value: '7d', label: '7 days' },
  ];
  function make(value = '24h') {
    const f = TestBed.createComponent(SuSegmentedControl);
    f.componentRef.setInput('options', options);
    f.componentRef.setInput('label', 'Time range');
    f.componentRef.setInput('value', value);
    document.body.appendChild(f.nativeElement as HTMLElement);
    f.detectChanges();
    const el = f.nativeElement as HTMLElement;
    const radios = (): HTMLButtonElement[] => Array.from(el.querySelectorAll('button'));
    const key = (k: string): void => {
      el.dispatchEvent(new KeyboardEvent('keydown', { key: k, bubbles: true, cancelable: true }));
      f.detectChanges();
    };
    return { f, el, radios, key };
  }

  it('is a named radio group with one tab stop on the checked option', () => {
    const { el, radios } = make();
    expect(el.getAttribute('role')).toBe('radiogroup');
    expect(el.getAttribute('aria-label')).toBe('Time range');
    expect(radios().map((r) => r.getAttribute('aria-checked'))).toEqual(['false', 'true', 'false', 'false']);
    expect(radios().map((r) => r.tabIndex)).toEqual([-1, 0, -1, -1]);
    expect(radios()[1]?.textContent).toContain('7');
  });

  it('selects on click, but not a disabled option', () => {
    const { f, radios } = make();
    radios()[0]?.click();
    f.detectChanges();
    expect(f.componentInstance.value()).toBe('1h');
    radios()[2]?.click();
    expect(f.componentInstance.value()).toBe('1h');
  });

  it('moves with arrows (skipping disabled), Home and End, and focuses the new option', () => {
    const { f, radios, key } = make();
    key('ArrowRight');
    expect(f.componentInstance.value()).toBe('7d');
    expect(document.activeElement).toBe(radios()[3]);
    key('ArrowDown');
    expect(f.componentInstance.value()).toBe('1h');
    key('ArrowLeft');
    expect(f.componentInstance.value()).toBe('7d');
    key('ArrowUp');
    expect(f.componentInstance.value()).toBe('24h');
    key('Home');
    expect(f.componentInstance.value()).toBe('1h');
    key('End');
    expect(f.componentInstance.value()).toBe('7d');
  });

  it('ignores other keys', () => {
    const { f, key } = make();
    key('a');
    expect(f.componentInstance.value()).toBe('24h');
  });

  it('with no enabled option arrows do nothing', () => {
    const f = TestBed.createComponent(SuSegmentedControl);
    f.componentRef.setInput('options', [{ value: 'a', label: 'A', disabled: true }]);
    f.componentRef.setInput('label', 'x');
    f.detectChanges();
    (f.nativeElement as HTMLElement).dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight', bubbles: true }));
    expect(f.componentInstance.value()).toBe('');
  });
});

describe('SuTrayPopover', () => {
  function make(inputs: Record<string, unknown>) {
    const f = TestBed.createComponent(SuTrayPopover);
    for (const [k, v] of Object.entries(inputs)) f.componentRef.setInput(k, v);
    f.detectChanges();
    return f;
  }
  it('is a named dialog with platform class and slots', () => {
    const f = make({ label: 'Dyno', platform: 'windows', translucent: true });
    const el = f.nativeElement as HTMLElement;
    expect(el.getAttribute('role')).toBe('dialog');
    expect(el.getAttribute('aria-label')).toBe('Dyno');
    expect(el.classList).toContain('pd-tray--windows');
    expect(el.classList).toContain('pd-tray--translucent');
    expect(el.querySelector('.pd-header')).not.toBeNull();
    expect(el.querySelector('.pd-content')).not.toBeNull();
    expect(el.querySelector('.pd-footer')).not.toBeNull();
  });
  it('defaults to macOS and solid', () => {
    const el = make({ label: 'x' }).nativeElement as HTMLElement;
    expect(el.classList).toContain('pd-tray--macos');
    expect(el.classList).not.toContain('pd-tray--translucent');
  });
  it('Escape asks to dismiss', () => {
    const f = make({ label: 'x' });
    let n = 0;
    f.componentInstance.dismissed.subscribe(() => n++);
    (f.nativeElement as HTMLElement).dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }));
    expect(n).toBe(1);
  });
});
