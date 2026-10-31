import { Component, signal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import type { CommandError, UpdatePhase } from '../../core/api.types';
import { appProviders, makeMock, settle, text } from '../../../testing/test-helpers';
import { BrandLogo } from './brand-logo';
import { EmptyState } from './empty-state';
import { LoadState } from './load-state';
import { ServiceStore } from '../../state/service.store';
import { Sparkline } from './sparkline';
import { StatusBadge } from './status-badge';
import { Timeline, type TimelineRow } from './timeline';
import { UpdateProgress } from './update-progress';

function configure(platform: 'browser' | 'macos' = 'browser') {
  TestBed.configureTestingModule({ providers: appProviders(makeMock({ platform })) });
}

describe('BrandLogo', () => {
  it('uses the Windows tile by default and the macOS tile on macOS', () => {
    configure();
    const f = TestBed.createComponent(BrandLogo);
    f.componentRef.setInput('size', 40);
    f.detectChanges();
    const img = f.nativeElement.querySelector('img') as HTMLImageElement;
    expect(img.getAttribute('src')).toBe('brand/pitcrew-windows.svg');
    TestBed.resetTestingModule();
    configure('macos');
    const g = TestBed.createComponent(BrandLogo);
    g.detectChanges();
    expect((g.nativeElement.querySelector('img') as HTMLImageElement).getAttribute('src')).toBe('brand/pitcrew-macos.svg');
  });
});

@Component({
  imports: [EmptyState],
  template: `<pc-empty-state [title]="title" [text]="body" [brand]="brand" icon="logs"><button type="button">Do it</button></pc-empty-state>`,
})
class EmptyHost {
  title = 'Nothing here';
  body: string | null = 'Add something.';
  brand = false;
}

describe('EmptyState', () => {
  it('shows title, text and projected actions', () => {
    configure();
    const f = TestBed.createComponent(EmptyHost);
    f.detectChanges();
    const el = f.nativeElement as HTMLElement;
    expect(text(el.querySelector('h3'))).toBe('Nothing here');
    expect(text(el.querySelector('p'))).toBe('Add something.');
    expect(text(el.querySelector('button'))).toBe('Do it');
    expect(el.querySelector('pc-brand-logo')).toBeNull();
    expect(el.querySelector('mat-icon')).not.toBeNull();
  });

  it('shows the brand logo and omits missing text', () => {
    configure();
    const f = TestBed.createComponent(EmptyHost);
    f.componentInstance.brand = true;
    f.componentInstance.body = null;
    f.detectChanges();
    const el = f.nativeElement as HTMLElement;
    expect(el.querySelector('pc-brand-logo')).not.toBeNull();
    expect(el.querySelector('p')).toBeNull();
  });
});

@Component({
  imports: [LoadState],
  template: `<pc-load-state what="Activity" [hasValue]="has()" [error]="error()" [skeleton]="[10, 20]" (retry)="retried = retried + 1"
    ><p class="content">Loaded</p></pc-load-state
  >`,
})
class LoadHost {
  readonly has = signal(false);
  readonly error = signal<CommandError | null>(null);
  retried = 0;
}

describe('LoadState', () => {
  function mount() {
    configure();
    const f = TestBed.createComponent(LoadHost);
    f.detectChanges();
    return f;
  }

  it('shows a labelled skeleton before the first value', () => {
    const f = mount();
    const el = f.nativeElement as HTMLElement;
    expect(el.querySelector('.content')).toBeNull();
    expect(el.querySelector('[aria-busy="true"]')?.getAttribute('aria-label')).toBe('Loading activity');
    expect(el.querySelectorAll('.skeleton')).toHaveLength(2);
  });

  it('shows the content once there is a value', () => {
    const f = mount();
    f.componentInstance.has.set(true);
    f.detectChanges();
    expect(text(f.nativeElement.querySelector('.content'))).toBe('Loaded');
    expect(f.nativeElement.querySelector('[role="alert"]')).toBeNull();
  });

  it('shows the error with Retry, and keeps content above a failed refresh', () => {
    const f = mount();
    f.componentInstance.error.set({ code: 'internal', message: 'It broke' });
    f.detectChanges();
    const el = f.nativeElement as HTMLElement;
    expect(text(el.querySelector('[role="alert"]'))).toContain('Activity could not be loaded');
    expect(text(el)).toContain('It broke');
    expect(el.querySelector('.content')).toBeNull();
    (el.querySelector('[role="alert"] button') as HTMLButtonElement).click();
    expect(f.componentInstance.retried).toBe(1);
    f.componentInstance.has.set(true);
    f.detectChanges();
    expect(el.querySelector('.content')).not.toBeNull();
    expect(el.querySelector('[role="alert"]')).not.toBeNull();
  });
});

describe('LoadState with the service down', () => {
  it('offers Repair service on a serviceUnavailable error on macOS, and the button calls the app command', async () => {
    const mock = makeMock({ serverDown: true, startFailed: true });
    TestBed.configureTestingModule({ providers: appProviders(mock) });
    const f = TestBed.createComponent(LoadHost);
    f.componentInstance.error.set({ code: 'serviceUnavailable', message: 'It is not running.' });
    f.detectChanges();
    await TestBed.inject(ServiceStore).load();
    f.detectChanges();
    const el = f.nativeElement as HTMLElement;
    const repair = el.querySelector<HTMLButtonElement>('[data-testid="repair-service"]');
    expect(text(repair)).toBe('Repair service');
    repair!.click();
    await settle(f, 6);
    expect(mock.repairs).toBe(1);
    expect(el.querySelector('[data-testid="repair-service"]')).toBeNull();
    mock.dispose();
  });

  it('offers no Repair for other errors or off macOS', async () => {
    const mock = makeMock({ serverDown: true });
    TestBed.configureTestingModule({ providers: appProviders(mock) });
    const f = TestBed.createComponent(LoadHost);
    f.componentInstance.error.set({ code: 'serviceUnavailable', message: 'It is not running.' });
    f.detectChanges();
    await TestBed.inject(ServiceStore).load();
    f.detectChanges();
    expect(f.nativeElement.querySelector('[data-testid="repair-service"]')).toBeNull();
    mock.dispose();
  });
});

describe('Sparkline', () => {
  it('draws a line and area for the values', () => {
    configure();
    const f = TestBed.createComponent(Sparkline);
    f.componentRef.setInput('values', [1, 5, 3]);
    f.componentRef.setInput('width', 100);
    f.componentRef.setInput('height', 20);
    f.detectChanges();
    const el = f.nativeElement as HTMLElement;
    expect(el.querySelector('svg')?.getAttribute('viewBox')).toBe('0 0 100 20');
    expect(el.querySelector('path.line')?.getAttribute('d')).toMatch(/^M0\.0,/);
    expect(el.querySelector('path.area')?.getAttribute('d')).toContain('Z');
    f.componentRef.setInput('values', []);
    f.detectChanges();
    expect(el.querySelector('path.line')?.getAttribute('d')).toBe('');
  });
});

describe('StatusBadge', () => {
  it('shows the label, hint and tone of the status', () => {
    configure();
    const f = TestBed.createComponent(StatusBadge);
    f.componentRef.setInput('status', 'crashLoop');
    f.detectChanges();
    const badge = f.nativeElement.querySelector('.badge') as HTMLElement;
    expect(text(badge)).toBe('Crash loop');
    expect(badge.classList.contains('danger')).toBe(true);
    expect(badge.getAttribute('title')).toContain('Exited repeatedly');
    expect(badge.querySelector('mat-icon')?.classList.contains('spin')).toBe(false);
    f.componentRef.setInput('status', 'starting');
    f.detectChanges();
    expect(badge.querySelector('mat-icon')?.classList.contains('spin')).toBe(true);
  });
});

describe('Timeline', () => {
  const from = new Date(2026, 9, 20).getTime();
  const to = from + 48 * 3_600_000;
  const rows: TimelineRow[] = [
    {
      name: 'build-win',
      uptimePermille: 995,
      up: [{ from, to }],
      busy: [{ from: from + 3_600_000, to: from + 7_200_000 }],
      marks: [
        { at: from + 10_000_000, kind: 'crash' },
        { at: from + 20_000_000, kind: 'updated' },
        { at: from + 30_000_000, kind: 'crashLoop' },
        { at: from + 40_000_000, kind: 'cleanup' },
        { at: to + 1000, kind: 'stall' },
      ],
    },
    { name: 'empty', uptimePermille: null, up: [], busy: [], marks: [] },
  ];

  it('lays out rows with accessible summaries, marks and ticks', async () => {
    configure();
    const f = TestBed.createComponent(Timeline);
    f.componentRef.setInput('rows', rows);
    f.componentRef.setInput('from', from);
    f.componentRef.setInput('to', to);
    await settle(f);
    const el = f.nativeElement as HTMLElement;
    const tracks = el.querySelectorAll('.track');
    expect(tracks).toHaveLength(2);
    expect(tracks[0]!.getAttribute('aria-label')).toBe('build-win: up 99.5% of the time, 2 restarts or problems, 1 updates.');
    expect(tracks[1]!.getAttribute('aria-label')).toContain('up — of the time, 0 restarts');
    expect(tracks[0]!.querySelectorAll('.mark')).toHaveLength(3);
    expect(tracks[0]!.querySelectorAll('.mark.update')).toHaveLength(1);
    expect(tracks[0]!.querySelectorAll('.mark.problem')).toHaveLength(1);
    expect(tracks[0]!.querySelectorAll('.up')).toHaveLength(1);
    expect(tracks[0]!.querySelectorAll('.busy')).toHaveLength(1);
    expect(text(el.querySelectorAll('.uptime')[0])).toBe('99.5%');
    expect(el.querySelectorAll('.ticks span').length).toBeGreaterThan(1);
    expect(text(el.querySelector('.legend'))).toContain('Crash loop');
  });
});

describe('UpdateProgress', () => {
  const plan = { from: '5.1.1', to: '5.2.0' };
  function mount(phase: UpdatePhase, now = 100_000) {
    configure();
    const f = TestBed.createComponent(UpdateProgress);
    f.componentRef.setInput('phase', phase);
    f.componentRef.setInput('now', now);
    f.detectChanges();
    return f.nativeElement as HTMLElement;
  }

  it('marks done, current and pending steps with the plan and elapsed time', () => {
    const el = mount({ phase: 'draining', plan, since: 40_000 });
    expect(text(el.querySelector('.what'))).toBe('Waiting for the current job to end · 1m 00s');
    expect(text(el.querySelector('.faint'))).toBe('5.1.1 → 5.2.0');
    const steps = Array.from(el.querySelectorAll('li'));
    expect(steps.map((s) => s.classList.contains('done'))).toEqual([true, true, false, false, false, false]);
    expect(steps[2]!.classList.contains('current')).toBe(true);
    expect(el.querySelector('ol')?.getAttribute('aria-label')).toBe('Update progress: step 3 of 6');
    expect(steps[2]!.getAttribute('aria-current')).toBe('step');
  });

  it('has no elapsed text for phases without a start time', () => {
    const el = mount({ phase: 'downloading', plan });
    expect(text(el.querySelector('.what'))).toBe('Downloading');
  });

  it('describes a roll-back', () => {
    const el = mount({ phase: 'rollingBack', plan, reason: 'exitedEarly' });
    expect(text(el.querySelector('.what'))).toContain('Rolling back: the new runner exited');
    expect(el.querySelector('.what')?.classList.contains('tone-warning')).toBe(true);
  });

  it('shows nothing about a plan when idle', () => {
    const el = mount({ phase: 'idle' });
    expect(el.querySelector('.faint')).toBeNull();
    expect(Array.from(el.querySelectorAll('li')).some((li) => li.classList.contains('current'))).toBe(false);
  });
});
