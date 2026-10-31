import { LiveAnnouncer } from '@angular/cdk/a11y';
import { CdkDropList } from '@angular/cdk/drag-drop';
import { ChangeDetectionStrategy, Component, provideZonelessChangeDetection, signal, viewChild } from '@angular/core';
import { TestBed, type ComponentFixture } from '@angular/core/testing';
import { MatIconRegistry } from '@angular/material/icon';
import { MatSnackBar } from '@angular/material/snack-bar';
import { DomSanitizer, By } from '@angular/platform-browser';
import { registerPaddockIcons } from '@decryp7/paddock-ui/icons';
import { KEY_PLATFORM } from '@decryp7/paddock-ui/shell/menu';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { CatalogueInfo, Edit, KindInfo, LayoutView, Placement, Preset, Progress, SurfaceValues, WidgetState } from '../generated/widgets';
import { PdWidgetArea } from './area';
import { PdWidgetChooser } from './chooser';
import { progressText, PdWidgetFrame } from './frame';
import { relativeAgo } from './time';
import { defaultKindOf, PdSkeleton, PdWidget, PdWidgetOption } from './widget';

type W = { kind: 'cpu' | 'alarms' | 'disk' | 'net'; volume?: string };
type V = { text: string };
type A = 'retry' | 'open';

const kind = (k: string, title: string, spans: number[], maxInstances = 1, icon = 'pd:activity', refresh: KindInfo<string>['refresh'] = 'live'): KindInfo<string> => ({
  kind: k,
  title,
  summary: `${title} summary`,
  icon,
  spans,
  maxInstances,
  refresh,
});
const CATALOGUE: CatalogueInfo<string, string> = {
  surface: { id: 'popup', noun: 'popup', columns: 2, maxPlacements: 4, minPlacements: 1, maxUserPresets: 0 },
  kinds: [kind('cpu', 'CPU and memory', [2]), kind('alarms', 'Active alarms', [2], 1, 'pd:bell', 'onChange'), kind('disk', 'Disk space', [2], 2, 'pd:folder', { every: { secs: 60 } }), kind('net', 'Network', [1, 2])],
};
const place = (id: string, k: W['kind'], span = 2): Placement<W> => ({ id, span, widget: { kind: k } });
const PLACEMENTS = [place('alarms', 'alarms'), place('disk', 'disk'), place('cpu', 'cpu'), place('net', 'net', 1)];
const view = (over: Partial<LayoutView<string, W, string>> = {}, placements = PLACEMENTS, catalogue = CATALOGUE): LayoutView<string, W, string> => ({
  layout: { version: 1, placements, presets: [] },
  catalogue,
  builtinPresets: [],
  notes: [],
  canUndo: false,
  editable: true,
  ...over,
});
const ready = (text: string, staleSince: number | null = null): WidgetState<V, A> => ({ state: 'ready', value: { text }, asOf: 1, staleSince });
const act = (action: A, label: string) => ({ action, label });
const values = (...p: [string, WidgetState<V, A>][]): SurfaceValues<V, A> => ({ notice: null, placements: p.map(([id, state]) => ({ id, state })) });

@Component({
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [PdWidgetArea, PdWidget, PdSkeleton],
  template: `
    <pd-widget-area [placements]="placements()" [catalogue]="catalogue" [values]="vals()" [now]="now" [meta]="meta" (action)="actions.push($event)" (customize)="customized = customized + 1">
      <ng-template pdWidget="cpu" [pdWidgetOf]="w" let-v>
        <div class="big" pdSkeleton>{{ v?.text ?? '00' }}</div>
        <div class="line"><span pdSkeleton>{{ v?.text ?? 'xxxx' }}</span></div>
      </ng-template>
      <ng-template pdWidget="alarms" [pdWidgetCalm]="true" [pdWidgetOf]="w" let-v><div pdSkeleton>{{ v?.text ?? 'none' }}</div></ng-template>
      <ng-template pdWidget="disk" [pdWidgetOf]="w" let-v let-stale="stale" let-loading="loading"><div pdSkeleton [attr.data-stale]="stale" [attr.data-loading]="loading">{{ v?.text ?? 'disk' }}</div></ng-template>
    </pd-widget-area>
  `,
})
class AreaHost {
  readonly w: V | null = null;
  readonly placements = signal<Placement<W>[]>(PLACEMENTS);
  readonly vals = signal<SurfaceValues<V, A> | null>(null);
  readonly catalogue = CATALOGUE;
  readonly now = 10 * 60_000;
  readonly meta = (id: string) => (id === 'cpu' ? '3 volumes' : null);
  actions: A[] = [];
  customized = 0;
}

@Component({
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [PdWidgetFrame, PdWidget],
  template: `
    <pd-widget-frame title="Disk space" icon="pd:folder" [state]="state()" [template]="dir()?.template ?? null" [span]="span()" [live]="live()" [calm]="calm()" [now]="now" metaText="3 volumes" (action)="actions.push($event)" />
    <ng-template pdWidget="disk" [pdWidgetOf]="w" let-v><p class="body">{{ v?.text ?? 'none' }}</p></ng-template>
  `,
})
class FrameHost {
  readonly w: V | null = null;
  readonly state = signal<WidgetState<V, A> | null | undefined>(undefined);
  readonly span = signal(2);
  readonly live = signal(false);
  readonly calm = signal(false);
  readonly now = 10 * 60_000;
  readonly dir = viewChild(PdWidget);
  actions: A[] = [];
}

async function settle(f: { whenStable(): Promise<unknown> }): Promise<void> {
  await f.whenStable();
  TestBed.tick();
  await f.whenStable();
}

function setupIcons(): void {
  registerPaddockIcons(TestBed.inject(MatIconRegistry), TestBed.inject(DomSanitizer));
}

const q = (f: ComponentFixture<unknown>, sel: string) => f.nativeElement.querySelector(sel) as HTMLElement | null;
const qa = (f: ComponentFixture<unknown>, sel: string) => [...(f.nativeElement as HTMLElement).querySelectorAll<HTMLElement>(sel)];
const frame = (f: ComponentFixture<unknown>, i: number) => qa(f, 'pd-widget-frame')[i]!;

/** The structure of a frame, text and the skeleton marks removed: loading and ready must have the same one. */
function shape(el: Element): string {
  const c = el.cloneNode(true) as HTMLElement;
  c.querySelectorAll('.pd-w-meta').forEach((n) => n.remove());
  for (const n of c.querySelectorAll('[data-stale], [data-loading]')) { n.removeAttribute('data-stale'); n.removeAttribute('data-loading'); }
  for (const n of c.querySelectorAll('.pd-sk')) n.classList.remove('pd-sk');
  for (const n of c.querySelectorAll('[aria-hidden]')) n.removeAttribute('aria-hidden');
  c.querySelectorAll('[class=""]').forEach((n) => n.removeAttribute('class'));
  return c
    .querySelector('.pd-w')!
    .outerHTML.replace(/<!--[^>]*-->/g, '')
    .replace(/ aria-busy="true"/, '')
    .replace(/ pd-w--loading/, '')
    .replace(/>[^<]+</g, '><');
}

describe('helpers', () => {
  it('reads a widget kind from a string, a kind field or the single key of a tagged object', () => {
    expect(defaultKindOf('cpu')).toBe('cpu');
    expect(defaultKindOf({ kind: 'disk', volume: 'x' })).toBe('disk');
    expect(defaultKindOf({ disk: { volume: 'x' } })).toBe('disk');
    expect(defaultKindOf({ a: 1, b: 2 })).toBe('');
    expect(defaultKindOf(null)).toBe('');
    expect(defaultKindOf(3)).toBe('');
    expect(defaultKindOf({})).toBe('');
  });

  it('says how long ago a time was, from the clock it is given', () => {
    expect(relativeAgo(100_000, 100_000)).toBe('just now');
    expect(relativeAgo(100_000, 200_000)).toBe('just now');
    expect(relativeAgo(10 * 60_000, 6 * 60_000)).toBe('4 min ago');
    expect(relativeAgo(10 * 3_600_000, 7 * 3_600_000)).toBe('3 h ago');
    expect(relativeAgo(5 * 86_400_000, 3 * 86_400_000)).toBe('2 d ago');
  });

  it('writes progress as a growing count or a fraction', () => {
    expect(progressText({ count: { done: 12400, unit: 'events' } }, 'en')).toBe('12,400 events so far');
    expect(progressText({ count: { done: 3, unit: '' } }, 'en')).toBe('3 so far');
    expect(progressText({ fraction: { done: 2, total: 3, unit: '' } }, 'en')).toBe('2 of 3');
  });

  it('has context guards that accept any context', () => {
    expect(PdWidget.ngTemplateContextGuard(null as never, {})).toBe(true);
    expect(PdWidgetOption.ngTemplateContextGuard(null as never, {})).toBe(true);
  });

  it('draws no skeleton outside a frame', () => {
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection()] });
    @Component({ changeDetection: ChangeDetectionStrategy.OnPush, imports: [PdSkeleton], template: `<span pdSkeleton>x</span>` })
    class Bare {}
    const f = TestBed.createComponent(Bare);
    f.detectChanges();
    expect(q(f, 'span')!.classList.contains('pd-sk')).toBe(false);
    expect(q(f, 'span')!.hasAttribute('aria-hidden')).toBe(false);
  });
});

describe('pd-widget-frame', () => {
  beforeEach(() => TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection()] }));

  async function setup(): Promise<ComponentFixture<FrameHost>> {
    setupIcons();
    const f = TestBed.createComponent(FrameHost);
    document.body.appendChild(f.nativeElement);
    await settle(f);
    return f;
  }
  const frameCmp = (f: ComponentFixture<FrameHost>) => f.debugElement.query(By.directive(PdWidgetFrame));

  it('is a section named by its title and busy while loading, drawing the template with no value', async () => {
    const f = await setup();
    const s = q(f, 'section')!;
    expect(s.getAttribute('aria-busy')).toBe('true');
    expect(s.getAttribute('aria-labelledby')).toBe(q(f, '.pd-w-title')!.id);
    expect(q(f, '.pd-w-title')!.textContent).toBe('Disk space');
    expect(q(f, '.body')!.textContent).toBe('none');
    f.componentInstance.state.set(null);
    await settle(f);
    expect(s.getAttribute('aria-busy')).toBe('true');
  });

  it('draws the ready template with a value, a fresh reading with its meta and the span as a grid column', async () => {
    const f = await setup();
    const cmp = frameCmp(f).componentInstance as PdWidgetFrame<V, A>;
    expect(cmp.span()).toBe(2);
    expect(frame(f, 0).style.gridColumn).toContain('span 2');
    f.componentInstance.state.set(ready('182 GB'));
    await settle(f);
    expect(q(f, 'section')!.getAttribute('aria-busy')).toBeNull();
    expect(q(f, '.pd-w-meta')!.textContent).toBe('3 volumes');
    expect(q(f, '.pd-w-meta--stale')).toBeNull();
    expect(q(f, '.body')!.textContent).toBe('182 GB');
  });

  it('says "Updated 4 min ago" when stale, "Paused at" for a live kind, and the clock with the full wording in the name at half width', async () => {
    const f = await setup();
    const fifty = Date.UTC(2020, 0, 1, 14, 28);
    const fx = f.componentInstance;
    fx.state.set(ready('x', 6 * 60_000));
    await settle(f);
    expect(q(f, '.pd-w-meta--stale')!.textContent).toContain('Updated 4 min ago');
    expect(q(f, '.pd-w-meta--stale svg')).not.toBeNull();
    fx.span.set(1);
    await settle(f);
    const meta = q(f, '.pd-w-meta--stale')!;
    expect(meta.textContent!.trim()).toBe('4 min');
    expect(meta.getAttribute('title')).toBe('Updated 4 min ago');
    expect(meta.getAttribute('aria-label')).toBe('Updated 4 min ago');
    fx.live.set(true);
    fx.state.set(ready('x', fifty));
    await settle(f);
    const text = q(f, '.pd-w-meta--stale')!.getAttribute('aria-label')!;
    expect(text).toMatch(/^Paused at \d\d:\d\d$/);
    expect(q(f, '.pd-w-meta--stale')!.textContent!.trim()).toBe(text.replace('Paused at ', ''));
    fx.span.set(2);
    await settle(f);
    expect(q(f, '.pd-w-meta--stale')!.textContent).toContain('Paused at');
  });

  it('draws an empty state with the benefit, the detail and the next step, which emits the app action', async () => {
    const f = await setup();
    f.componentInstance.state.set({ state: 'empty', title: 'See disk space', detail: 'No disks.', next: act('open', 'Choose another volume') });
    await settle(f);
    expect(q(f, '.pd-w-t')!.textContent).toBe('See disk space');
    expect(q(f, '.pd-w-d')!.textContent).toBe('No disks.');
    q(f, '.pd-w-act')!.click();
    expect(f.componentInstance.actions).toEqual(['open']);
    f.componentInstance.state.set({ state: 'empty', title: 'T', detail: 'D', next: null });
    await settle(f);
    expect(q(f, '.pd-w-act')).toBeNull();
  });

  it('draws a calm empty state as one real button named with its text and next step', async () => {
    const f = await setup();
    f.componentInstance.calm.set(true);
    f.componentInstance.state.set({ state: 'empty', title: 'No active alarms', detail: 'checked 2 min ago', next: act('open', 'Open alarms') });
    await settle(f);
    const b = q(f, 'button.pd-w-calm')!;
    expect(b.getAttribute('aria-label')).toBe('No active alarms, checked 2 min ago. Open alarms');
    b.click();
    expect(f.componentInstance.actions).toEqual(['open']);
    f.componentInstance.state.set({ state: 'empty', title: 'No alarms', detail: 'none', next: null });
    await settle(f);
    expect(q(f, 'button.pd-w-calm')).toBeNull();
    expect(q(f, '.pd-w-st')).not.toBeNull();
  });

  it('draws an error with its cause and Retry, never as the whole surface', async () => {
    const f = await setup();
    f.componentInstance.state.set({ state: 'error', title: "Couldn't read volumes", cause: 'The volume list did not answer.', next: act('retry', 'Retry') });
    await settle(f);
    expect(q(f, '.pd-w-st--error .pd-w-d')!.textContent).toBe('The volume list did not answer.');
    q(f, '.pd-w-act')!.click();
    expect(f.componentInstance.actions).toEqual(['retry']);
    f.componentInstance.state.set({ state: 'error', title: 'E', cause: null, next: null });
    await settle(f);
    expect(q(f, '.pd-w-d')).toBeNull();
    expect(q(f, '.pd-w-act')).toBeNull();
  });

  it('draws working with the phase, a growing count on an indeterminate bar, then a fraction on a determinate one, and cancel', async () => {
    const f = await setup();
    const working = (progress: Progress, cancel = true): WidgetState<V, A> => ({
      state: 'working',
      title: 'Reading system logs',
      phase: 'Indexing',
      progress,
      last: null,
      cancel: cancel ? act('retry', 'Cancel') : null,
    });
    f.componentInstance.state.set(working({ count: { done: 12400, unit: 'events' } }));
    await settle(f);
    expect(q(f, '.pd-w-d')!.textContent).toBe('Indexing · 12,400 events so far');
    expect(q(f, 'mat-progress-bar')!.getAttribute('mode')).toBe('indeterminate');
    q(f, '.pd-w-act')!.click();
    expect(f.componentInstance.actions).toEqual(['retry']);
    f.componentInstance.state.set(working({ fraction: { done: 2, total: 4, unit: '' } }, false));
    await settle(f);
    expect(q(f, 'mat-progress-bar')!.getAttribute('mode')).toBe('determinate');
    expect(q(f, 'mat-progress-bar')!.getAttribute('aria-valuenow')).toBe('50');
    expect(q(f, '.pd-w-act')).toBeNull();
    f.componentInstance.state.set(working({ fraction: { done: 0, total: 0, unit: '' } }, false));
    await settle(f);
    expect(q(f, 'mat-progress-bar')!.getAttribute('aria-valuenow')).toBe('0');
  });

  it('shows the last value under the working bar when there is one, and the system clock when none is given', async () => {
    const f = await setup();
    f.componentInstance.state.set({ state: 'working', title: 'Checking', phase: 'Time Machine', progress: { fraction: { done: 2, total: 3, unit: '' } }, last: { text: 'last 182 GB' }, cancel: null });
    await settle(f);
    expect(q(f, '.body')!.textContent).toBe('last 182 GB');
    @Component({ changeDetection: ChangeDetectionStrategy.OnPush, imports: [PdWidgetFrame], template: `<pd-widget-frame title="T" icon="pd:bell" [state]="s" />` })
    class NoClock {
      s: WidgetState<V, A> = ready('x', Date.now() - 5 * 60_000);
    }
    const g = TestBed.createComponent(NoClock);
    await settle(g);
    expect(q(g, '.pd-w-meta--stale')!.textContent).toContain('Updated 5 min ago');
    g.componentInstance.s = { state: 'working', title: 'T', phase: 'p', progress: { count: { done: 1, unit: 'x' } }, last: null, cancel: null };
    g.changeDetectorRef.markForCheck();
    expect(q(g, '.body')).toBeNull();
  });
});

describe('pd-widget-area', () => {
  beforeEach(() => TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection()] }));
  afterEach(() => document.body.replaceChildren());

  async function setup(): Promise<ComponentFixture<AreaHost>> {
    setupIcons();
    const f = TestBed.createComponent(AreaHost);
    document.body.appendChild(f.nativeElement);
    await settle(f);
    return f;
  }

  it('draws the placements in order, each loading until it has a state, on the catalogue grid', async () => {
    const f = await setup();
    expect(qa(f, '.pd-w-title').map((t) => t.textContent)).toEqual(['Active alarms', 'Disk space', 'CPU and memory', 'Network']);
    expect(qa(f, '.pd-w').every((s) => s.getAttribute('aria-busy') === 'true')).toBe(true);
    expect(q(f, '.pd-wa-grid')!.style.gridTemplateColumns).toContain('repeat(2');
    expect(frame(f, 3).style.gridColumn).toContain('span 1');
    expect(qa(f, 'section')).toHaveLength(4);
  });

  it('clamps a span wider than the surface to its columns', async () => {
    const f = await setup();
    f.componentInstance.placements.set([place('w', 'cpu', 5), place('z', 'net', 0)]);
    await settle(f);
    expect(frame(f, 0).style.gridColumn).toContain('span 2');
    expect(frame(f, 1).style.gridColumn).toContain('span 1');
  });

  it('draws each ready value through its kind template and marks stale readings', async () => {
    const f = await setup();
    f.componentInstance.vals.set(values(['disk', ready('182 GB', 9 * 60_000)], ['cpu', ready('76 %')], ['alarms', ready('none')]));
    await settle(f);
    const disk = frame(f, 1);
    expect(disk.querySelector('[data-stale="true"]')!.textContent).toBe('182 GB');
    expect(disk.querySelector('.pd-w-meta--stale')!.textContent).toContain('Updated 1 min ago');
    expect(frame(f, 2).querySelector('.big')!.textContent).toBe('76 %');
    expect(frame(f, 2).querySelector('.pd-w-meta')!.textContent).toBe('3 volumes');
    // a kind with no template draws just its head
    expect(frame(f, 3).querySelector('.pd-w-title')!.textContent).toBe('Network');
  });

  it('draws a placement of an unknown kind with its kind name and a fallback icon', async () => {
    const f = await setup();
    f.componentInstance.placements.set([{ id: 'x', span: 2, widget: { kind: 'cpu', ...{} } as W }, { id: 'y', span: 2, widget: { kind: 'mystery' as W['kind'] } }]);
    await settle(f);
    expect(qa(f, '.pd-w-title').map((t) => t.textContent)).toEqual(['CPU and memory', 'mystery']);
  });

  it('keeps the loading height: the loading twin of a template has the structure of its ready one', async () => {
    const f = await setup();
    const loading = [0, 1, 2].map((i) => shape(frame(f, i)));
    f.componentInstance.vals.set(values(['alarms', ready('none')], ['disk', ready('182 GB')], ['cpu', ready('76 %')]));
    await settle(f);
    expect([0, 1, 2].map((i) => shape(frame(f, i)))).toEqual(loading);
    expect(qa(f, '.pd-sk')).toHaveLength(0);
    f.componentInstance.vals.set(null);
    await settle(f);
    expect(frame(f, 2).querySelectorAll('.pd-sk')).toHaveLength(2);
    expect(frame(f, 2).querySelector('.pd-sk')!.getAttribute('aria-hidden')).toBe('true');
    expect(frame(f, 1).querySelector('[data-loading="true"]')).not.toBeNull();
  });

  it('draws the notice once above the widgets with its action, which keep their stale values', async () => {
    const f = await setup();
    f.componentInstance.vals.set({
      notice: { tone: 'danger', title: 'Agent stopped', detail: 'Readings paused at 14:28.', action: act('retry', 'Restart agent') },
      placements: [{ id: 'cpu', state: ready('76 %', 9 * 60_000) }],
    });
    await settle(f);
    const n = q(f, '.pd-wa-notice')!;
    expect(n.getAttribute('role')).toBe('alert');
    expect(n.textContent).toContain('Agent stopped');
    expect(qa(f, '.pd-wa-notice')).toHaveLength(1);
    expect(n.nextElementSibling!.classList.contains('pd-wa-grid')).toBe(true);
    q(f, '.pd-wa-na')!.click();
    expect(f.componentInstance.actions).toEqual(['retry']);
    expect(frame(f, 2).querySelector('.pd-w-meta--stale')).not.toBeNull();
    f.componentInstance.vals.set({ notice: { tone: 'info', title: 'Hello', detail: 'd', action: null }, placements: [] });
    await settle(f);
    expect(q(f, '.pd-wa-notice')!.getAttribute('role')).toBe('status');
    expect(q(f, '.pd-wa-na')).toBeNull();
    f.componentInstance.vals.set({ notice: { tone: 'warning', title: 'W', detail: 'd', action: null }, placements: [] });
    await settle(f);
    expect(q(f, '.pd-wa-notice--warning')).not.toBeNull();
  });

  it('shows "Choose what the popup shows" on an empty layout and emits customize', async () => {
    const f = await setup();
    f.componentInstance.placements.set([]);
    await settle(f);
    expect(q(f, '.pd-wa-et')!.textContent).toBe('Choose what the popup shows');
    expect(q(f, '.pd-wa-grid')).toBeNull();
    q(f, '.pd-wa-ea')!.click();
    expect(f.componentInstance.customized).toBe(1);
  });

  it('draws kinds from templates passed in, a content template of the same kind winning', async () => {
    @Component({
      changeDetection: ChangeDetectionStrategy.OnPush,
      imports: [PdWidgetArea, PdWidget],
      template: `
        <ng-template pdWidget="net" #net="pdWidget"><i class="outer-net">outer net</i></ng-template>
        <ng-template pdWidget="cpu" #cpu="pdWidget"><i class="outer-cpu">outer cpu</i></ng-template>
        <pd-widget-area [placements]="p" [catalogue]="cat" [templates]="[net, cpu]" [values]="vals">
          <ng-template pdWidget="cpu"><i class="inner-cpu">inner cpu</i></ng-template>
        </pd-widget-area>
      `,
    })
    class Shared {
      readonly p = [place('c', 'cpu'), place('n', 'net', 1)];
      readonly cat = CATALOGUE;
      readonly vals = values(['c', ready('x')], ['n', ready('y')]);
    }
    setupIcons();
    const g = TestBed.createComponent(Shared);
    await settle(g);
    expect(q(g, '.inner-cpu')).not.toBeNull();
    expect(q(g, '.outer-cpu')).toBeNull();
    expect(q(g, '.outer-net')).not.toBeNull();
  });

  it('forwards an empty state action of a calm kind', async () => {
    const f = await setup();
    f.componentInstance.vals.set(values(['alarms', { state: 'empty', title: 'No active alarms', detail: 'checked now', next: act('open', 'Open alarms') }]));
    await settle(f);
    q(f, 'button.pd-w-calm')!.click();
    expect(f.componentInstance.actions).toEqual(['open']);
  });
});

@Component({
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [PdWidgetChooser, PdWidgetOption],
  template: `
    <pd-widget-chooser [view]="v()" [error]="err()" [widgetFor]="widgetFor" [again]="{ disk: 'Add volume' }" [preview]="preview()" (edit)="edits.push($event)">
      <ng-template pdWidgetOption="disk" let-p><span class="opt">volume of {{ p.id }}</span></ng-template>
      <div slot="preview" class="pv">preview</div>
    </pd-widget-chooser>
  `,
})
class ChooserHost {
  readonly v = signal(view());
  readonly preview = signal(false);
  readonly err = signal<string | null>(null);
  readonly widgetFor = (k: string): W => ({ kind: k as W['kind'] });
  edits: Edit<W>[] = [];
}

describe('pd-widget-chooser', () => {
  beforeEach(() => TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection()] }));
  afterEach(() => {
    TestBed.inject(MatSnackBar).dismiss();
    document.body.replaceChildren();
  });

  async function setup(): Promise<ComponentFixture<ChooserHost>> {
    setupIcons();
    const f = TestBed.createComponent(ChooserHost);
    document.body.appendChild(f.nativeElement);
    await settle(f);
    return f;
  }
  const rows = (f: ComponentFixture<unknown>) => qa(f, '.pd-wc-list:first-of-type > li, ul[role=list] > li.pd-wc-item:not(.pd-wc-add)');
  const grip = (f: ComponentFixture<unknown>, i: number) => qa(f, '.pd-wc-grip')[i]!;
  const key = (el: HTMLElement, k: string, init: KeyboardEventInit = { altKey: true }) => {
    const e = new KeyboardEvent('keydown', { key: k, bubbles: true, cancelable: true, ...init });
    el.dispatchEvent(e);
    return e;
  };
  const menuItems = () => [...document.querySelectorAll<HTMLElement>('.cdk-overlay-container [role=menuitem]')];
  const addButtons = (f: ComponentFixture<unknown>) => qa(f, '.pd-wc-add button');

  it('lists the shown widgets with a labelled grip, summary, option control, Half / Full toggle and menu, and counts them', async () => {
    const f = await setup();
    expect(qa(f, '.pd-wc-label')[0]!.textContent).toContain('Shown in the popup');
    expect(qa(f, '.pd-wc-count')[0]!.textContent).toBe('4 of 4');
    expect(qa(f, 'ul[role=list]')[0]!.getAttribute('aria-labelledby')).toBe(qa(f, '.pd-wc-label span')[0]!.id);
    expect(grip(f, 1).getAttribute('aria-label')).toBe('Reorder Disk space, position 2 of 4');
    expect(grip(f, 1).getAttribute('aria-describedby')).toBe(q(f, '.pd-wc-hint')!.id);
    expect(qa(f, '.pd-wc-item .pd-wc-txt b').slice(0, 4).map((b) => b.textContent)).toEqual(['Active alarms', 'Disk space', 'CPU and memory', 'Network']);
    expect(qa(f, '.opt').map((o) => o.textContent)).toEqual(['volume of disk']);
    const toggles = qa(f, 'mat-button-toggle');
    expect(toggles.map((t) => t.textContent!.trim())).toEqual(['Half', 'Full']);
    expect(qa(f, '.pd-wc-more').map((b) => b.getAttribute('aria-label'))[1]).toBe('More actions for Disk space');
    expect(rows(f).length).toBeGreaterThan(0);
  });

  it('emits the Edit for a width change', async () => {
    const f = await setup();
    const buttons = qa(f, 'mat-button-toggle button');
    buttons[1]!.click();
    await settle(f);
    expect(f.componentInstance.edits).toEqual([{ resize: { id: 'net', span: 2 } }]);
  });

  it('moves a row with Alt+Up, Down, Home and End, keeps focus on the moved handle and announces it', async () => {
    const f = await setup();
    const say = vi.spyOn(TestBed.inject(LiveAnnouncer), 'announce');
    grip(f, 1).focus();
    expect(key(grip(f, 1), 'ArrowUp').defaultPrevented).toBe(true);
    expect(f.componentInstance.edits.pop()).toEqual({ move: { id: 'disk', to: 0 } });
    // nothing is announced until the engine's answer shows the move
    expect(say).not.toHaveBeenCalled();
    // the app feeds the reordered view back
    f.componentInstance.v.set(view({}, [PLACEMENTS[1]!, PLACEMENTS[0]!, PLACEMENTS[2]!, PLACEMENTS[3]!]));
    await settle(f);
    expect(say).toHaveBeenLastCalledWith('Disk space moved to position 1 of 4');
    expect(document.activeElement).toBe(grip(f, 0));
    expect(grip(f, 0).getAttribute('aria-label')).toBe('Reorder Disk space, position 1 of 4');
    key(grip(f, 0), 'ArrowDown');
    expect(f.componentInstance.edits.pop()).toEqual({ move: { id: 'disk', to: 1 } });
    key(grip(f, 0), 'End');
    expect(f.componentInstance.edits.pop()).toEqual({ move: { id: 'disk', to: 3 } });
    key(grip(f, 3), 'Home');
    expect(f.componentInstance.edits.pop()).toEqual({ move: { id: 'net', to: 0 } });
  });

  it('says so at the ends instead of moving, and ignores other keys, other modifiers and a read-only list', async () => {
    const f = await setup();
    const say = vi.spyOn(TestBed.inject(LiveAnnouncer), 'announce');
    key(grip(f, 0), 'ArrowUp');
    expect(say).toHaveBeenLastCalledWith('Active alarms is already first.');
    key(grip(f, 3), 'ArrowDown');
    expect(say).toHaveBeenLastCalledWith('Network is already last.');
    key(grip(f, 0), 'Home');
    expect(say).toHaveBeenLastCalledWith('Active alarms is already first.');
    key(grip(f, 1), 'ArrowUp', {});
    expect(key(grip(f, 1), 'ArrowUp', { altKey: true, ctrlKey: true }).defaultPrevented).toBe(false);
    expect(key(grip(f, 1), 'ArrowUp', { altKey: true, metaKey: true }).defaultPrevented).toBe(false);
    expect(key(grip(f, 1), 'a').defaultPrevented).toBe(false);
    expect(f.componentInstance.edits).toEqual([]);
    f.componentInstance.v.set(view({ editable: false }));
    await settle(f);
    key(grip(f, 1), 'ArrowUp');
    expect(f.componentInstance.edits).toEqual([]);
    expect(grip(f, 1).getAttribute('aria-disabled')).toBe('true');
  });

  it('emits a move for a drop on a new position and nothing for a drop where it was', async () => {
    const f = await setup();
    const list = f.debugElement.query(By.directive(CdkDropList));
    list.triggerEventHandler('cdkDropListDropped', { previousIndex: 0, currentIndex: 2 });
    list.triggerEventHandler('cdkDropListDropped', { previousIndex: 1, currentIndex: 1 });
    list.triggerEventHandler('cdkDropListDropped', { previousIndex: 9, currentIndex: 0 });
    expect(f.componentInstance.edits).toEqual([{ move: { id: 'alarms', to: 2 } }]);
  });

  it('moves and removes through the row menu, with the keys shown, disabled at the ends', async () => {
    const f = await setup();
    qa(f, '.pd-wc-more')[1]!.click();
    await settle(f);
    expect(menuItems().map((m) => m.textContent!.trim())).toEqual(['Move upAlt+↑', 'Move downAlt+↓', 'Move to topAlt+Home', 'Move to bottomAlt+End', 'Remove']);
    expect(menuItems()[0]!.getAttribute('aria-keyshortcuts')).toBe('Alt+ArrowUp');
    menuItems()[0]!.click();
    await settle(f);
    expect(f.componentInstance.edits.pop()).toEqual({ move: { id: 'disk', to: 0 } });
    qa(f, '.pd-wc-more')[3]!.click();
    await settle(f);
    expect(menuItems()[1]!.hasAttribute('disabled') || menuItems()[1]!.getAttribute('aria-disabled') === 'true').toBe(true);
    menuItems()[2]!.click();
    await settle(f);
    expect(f.componentInstance.edits.pop()).toEqual({ move: { id: 'net', to: 0 } });
    qa(f, '.pd-wc-more')[0]!.click();
    await settle(f);
    menuItems()[3]!.click();
    await settle(f);
    expect(f.componentInstance.edits.pop()).toEqual({ move: { id: 'alarms', to: 3 } });
    qa(f, '.pd-wc-more')[2]!.click();
    await settle(f);
    menuItems()[1]!.click();
    await settle(f);
    expect(f.componentInstance.edits.pop()).toEqual({ move: { id: 'cpu', to: 3 } });
    qa(f, '.pd-wc-more')[2]!.click();
    await settle(f);
    menuItems()[4]!.click();
    await settle(f);
    expect(f.componentInstance.edits.pop()).toEqual({ remove: { id: 'cpu' } });
  });

  it('keeps the last widget: Remove is disabled at the surface minimum', async () => {
    const f = await setup();
    f.componentInstance.v.set(view({}, [PLACEMENTS[0]!]));
    await settle(f);
    qa(f, '.pd-wc-more')[0]!.click();
    await settle(f);
    expect(menuItems()[4]!.hasAttribute('disabled') || menuItems()[4]!.getAttribute('aria-disabled') === 'true').toBe(true);
  });

  it('adds a kind, says "Added" for a kind at its count, offers the app label to place one again and disables Add at the limit', async () => {
    const f = await setup();
    // 4 of 4: at the limit
    expect(q(f, '.pd-wc-limit')!.textContent).toContain('The popup holds 4 widgets. Remove one to add another.');
    expect(qa(f, '.pd-wc-addedlabel').map((a) => a.textContent!.trim())).toEqual(['Added', 'Added', 'Added']);
    expect(addButtons(f).map((b) => b.textContent!.trim())).toEqual(['Add volume']);
    expect(addButtons(f)[0]!.hasAttribute('disabled')).toBe(true);
    expect(addButtons(f)[0]!.getAttribute('aria-label')).toBe('Add volume Disk space');
    f.componentInstance.v.set(view({}, [PLACEMENTS[1]!]));
    await settle(f);
    expect(q(f, '.pd-wc-limit')).toBeNull();
    expect(addButtons(f).map((b) => b.textContent!.trim())).toEqual(['Add', 'Add', 'Add volume', 'Add']);
    addButtons(f)[0]!.click();
    expect(f.componentInstance.edits).toEqual([{ add: { widget: { kind: 'cpu' }, span: null, at: null } }]);
    // a kind placed again with no label of its own says "Add another"
    f.componentInstance.v.set(view({}, [PLACEMENTS[1]!], { ...CATALOGUE, kinds: CATALOGUE.kinds.map((k) => (k.kind === 'disk' ? { ...k, maxInstances: 3 } : k)) }));
    await settle(f);
    expect(addButtons(f)[2]!.textContent!.trim()).toBe('Add volume');
  });

  it('says "Add another" when the app gives no label', async () => {
    setupIcons();
    @Component({ changeDetection: ChangeDetectionStrategy.OnPush, imports: [PdWidgetChooser], template: `<pd-widget-chooser [view]="v" [widgetFor]="wf" />` })
    class Plain {
      v = view({}, [PLACEMENTS[1]!], { ...CATALOGUE, surface: { ...CATALOGUE.surface, noun: 'dashboard' } });
      wf = (k: string): W => ({ kind: k as W['kind'] });
    }
    const f = TestBed.createComponent(Plain);
    document.body.appendChild(f.nativeElement);
    await settle(f);
    expect(addButtons(f)[2]!.textContent!.trim()).toBe('Add another');
    expect(qa(f, '.pd-wc-label')[0]!.textContent).toContain('Shown in the dashboard');
  });

  it('labels widths of odd spans and resets to defaults', async () => {
    const f = await setup();
    f.componentInstance.v.set(
      view({}, [{ id: 'net', span: 1, widget: { kind: 'net' } }], { ...CATALOGUE, surface: { ...CATALOGUE.surface, columns: 3 }, kinds: [kind('net', 'Network', [1, 2, 3])] }),
    );
    await settle(f);
    expect(qa(f, 'mat-button-toggle').map((t) => t.textContent!.trim())).toEqual(['1 of 3', '2 of 3', 'Full']);
    [...qa(f, 'button')].find((b) => b.textContent!.trim() === 'Reset to defaults')!.click();
    expect(f.componentInstance.edits).toEqual(['resetToDefaults']);
  });

  it('shows the undo toast after a remove while canUndo is true, emits undo from it and dismisses it when canUndo turns false', async () => {
    const f = await setup();
    qa(f, '.pd-wc-more')[3]!.click();
    await settle(f);
    menuItems()[4]!.click();
    await settle(f);
    expect(document.querySelector('.mat-mdc-snack-bar-container')).toBeNull();
    f.componentInstance.v.set(view({ canUndo: true }, PLACEMENTS.slice(0, 3)));
    await settle(f);
    const snack = document.querySelector('.mat-mdc-snack-bar-container')!;
    expect(snack.textContent).toContain('Network removed from the popup.');
    expect(snack.classList.contains('pd-undo-snack') || document.querySelector('.pd-undo-snack') !== null).toBe(true);
    (snack.querySelector('button') as HTMLElement).click();
    await settle(f);
    expect(f.componentInstance.edits.pop()).toBe('undo');
    f.componentInstance.v.set(view({ canUndo: true }, PLACEMENTS.slice(0, 3)));
    f.componentInstance.v.set(view({ canUndo: false }));
    await settle(f);
    TestBed.inject(MatSnackBar)._openedSnackBarRef?.dismiss();
    f.componentInstance.v.set(view({ canUndo: true }));
    await settle(f);
    f.componentInstance.v.set(view({ canUndo: false }));
    await settle(f);
    await new Promise((r) => setTimeout(r, 300));
    expect(TestBed.inject(MatSnackBar)._openedSnackBarRef).toBeNull();
  });

  it('opens no toast for a refused edit even though the old view still says canUndo', async () => {
    const f = await setup();
    qa(f, '.pd-wc-more')[3]!.click();
    await settle(f);
    menuItems()[4]!.click();
    await settle(f);
    f.componentInstance.err.set('At most 4 widgets fit in the popup; remove one first.');
    await settle(f);
    f.componentInstance.v.set(view({ canUndo: true }));
    await settle(f);
    expect(document.querySelector('.mat-mdc-snack-bar-container')).toBeNull();
  });

  it('says the only widget cannot move, and cancels the delete confirm with Escape', async () => {
    const f = await setup();
    f.componentInstance.v.set(view({}, [PLACEMENTS[0]!]));
    await settle(f);
    const say = vi.spyOn(TestBed.inject(LiveAnnouncer), 'announce');
    key(grip(f, 0), 'ArrowDown');
    expect(say).toHaveBeenLastCalledWith('Active alarms is the only widget.');
    key(grip(f, 0), 'ArrowUp');
    expect(say).toHaveBeenLastCalledWith('Active alarms is the only widget.');
    f.componentInstance.v.set(withPresets(2, [preset('mine', 'Work', false, 1)], []));
    await settle(f);
    (q(f, '[data-preset-delete="mine"]') as HTMLElement).click();
    await settle(f);
    q(f, '[data-preset-confirm="mine"]')!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    await settle(f);
    expect(q(f, '.pd-wc-ask')).toBeNull();
    expect(document.activeElement).toBe(q(f, '[data-preset-delete="mine"]'));
  });

  it('writes the Option glyph and name on a Mac', async () => {
    TestBed.overrideProvider(KEY_PLATFORM, { useValue: 'mac' });
    const f = await setup();
    expect(q(f, '.pd-wc-hint')!.textContent).toContain('hold Option with');
    qa(f, '.pd-wc-more')[1]!.click();
    await settle(f);
    expect(menuItems()[0]!.textContent!.trim()).toBe('Move up⌥↑');
  });

  it('does not open an undo toast for a view that arrives with canUndo already true (reopened Settings, a refetch)', async () => {
    const f = await setup();
    f.componentInstance.v.set(view({ canUndo: true }));
    await settle(f);
    expect(document.querySelector('.mat-mdc-snack-bar-container')).toBeNull();
    f.componentInstance.v.set(view({ canUndo: true }, PLACEMENTS.slice(0, 3)));
    await settle(f);
    expect(document.querySelector('.mat-mdc-snack-bar-container')).toBeNull();
  });

  it('opens the toast once per edit and not again when the same state is fetched again', async () => {
    const f = await setup();
    qa(f, '.pd-wc-more')[3]!.click();
    await settle(f);
    menuItems()[4]!.click();
    await settle(f);
    f.componentInstance.v.set(view({ canUndo: true }, PLACEMENTS.slice(0, 3)));
    await settle(f);
    const open = vi.spyOn(TestBed.inject(MatSnackBar), 'open');
    f.componentInstance.v.set(view({ canUndo: true }, PLACEMENTS.slice(0, 3)));
    await settle(f);
    expect(open).not.toHaveBeenCalled();
    expect(document.querySelectorAll('.mat-mdc-snack-bar-container')).toHaveLength(1);
  });

  it('never opens a toast with no text, and drops a waiting edit the engine says cannot be undone', async () => {
    const f = await setup();
    qa(f, '.pd-wc-more')[3]!.click();
    await settle(f);
    menuItems()[4]!.click();
    await settle(f);
    f.componentInstance.v.set(view({ canUndo: false }, PLACEMENTS.slice(0, 3)));
    await settle(f);
    f.componentInstance.v.set(view({ canUndo: true }, PLACEMENTS.slice(0, 3)));
    await settle(f);
    expect(document.querySelector('.mat-mdc-snack-bar-container')).toBeNull();
  });

  it('announces a reset in the toast and closes it when the chooser is destroyed', async () => {
    const f = await setup();
    [...qa(f, 'button')].find((b) => b.textContent!.trim() === 'Reset to defaults')!.click();
    f.componentInstance.v.set(view({ canUndo: true }));
    await settle(f);
    expect(document.querySelector('.mat-mdc-snack-bar-container')!.textContent).toContain('The popup is back to the defaults.');
    f.destroy();
    await new Promise((r) => setTimeout(r, 300));
    expect(TestBed.inject(MatSnackBar)._openedSnackBarRef).toBeNull();
  });

  it('is read-only for a newer layout: no menus, options, toggles, Add or Reset, and the note says why', async () => {
    const f = await setup();
    f.componentInstance.v.set(view({ editable: false, notes: [{ code: { newer: { version: 3 } }, message: 'This layout was saved by a newer version of the app.' }] }, PLACEMENTS, { ...CATALOGUE, kinds: CATALOGUE.kinds.filter((k) => k.kind !== 'disk') }));
    await settle(f);
    expect(q(f, '.pd-wc-note')!.textContent).toContain('newer version');
    expect(qa(f, '.pd-wc-more')).toHaveLength(0);
    expect(qa(f, 'mat-button-toggle')).toHaveLength(0);
    expect(qa(f, '.opt')).toHaveLength(0);
    expect(addButtons(f)).toHaveLength(0);
    expect(qa(f, '.pd-wc-foot')).toHaveLength(0);
    // a placement whose kind is not in the catalogue still has a name
    expect(qa(f, '.pd-wc-txt b')[1]!.textContent).toBe('disk');
  });

  const preset = (id: string, name: string, builtin: boolean, n: number): Preset<W> => ({ id, name, builtin, placements: PLACEMENTS.slice(0, n) });
  const withPresets = (max: number, user: Preset<W>[], builtin: Preset<W>[], over: Partial<LayoutView<string, W, string>> = {}) =>
    view({ layout: { version: 1, placements: PLACEMENTS, presets: user }, builtinPresets: builtin, ...over }, PLACEMENTS, { ...CATALOGUE, surface: { ...CATALOGUE.surface, maxUserPresets: max } });
  const nameInput = (f: ComponentFixture<unknown>) => q(f, '.pd-wc-save input') as HTMLInputElement;
  const type = async (f: ComponentFixture<ChooserHost>, text: string) => {
    nameInput(f).value = text;
    nameInput(f).dispatchEvent(new Event('input', { bubbles: true }));
    await settle(f);
  };
  const saveButton = (f: ComponentFixture<unknown>) => qa(f, '.pd-wc-save button')[0] as HTMLButtonElement;

  it('shows no presets when the surface has none and keeps none', async () => {
    const f = await setup();
    expect(q(f, '.pd-wc-save')).toBeNull();
    expect(qa(f, '.pd-wc-label').map((l) => l.textContent)).not.toContain('Presets');
  });

  it('lists built-in and saved presets, applies one, deletes only a saved one and says it in the undo toast', async () => {
    const f = await setup();
    f.componentInstance.v.set(withPresets(2, [preset('mine', 'Work', false, 1)], [preset('min', 'Minimal', true, 2)]));
    await settle(f);
    const items = qa(f, 'ul[aria-labelledby$="-presets"] li');
    expect(items.map((i) => i.querySelector('b')!.textContent)).toEqual(['Minimal', 'Work']);
    expect(items[0]!.textContent).toContain('2 widgets · Built in');
    expect(items[1]!.textContent).toContain('1 widget');
    expect(items[1]!.textContent).not.toContain('1 widgets');
    expect(qa(f, '.pd-wc-count').map((c) => c.textContent)).toContain('1 of 2 saved');
    expect(items[0]!.querySelector('button[aria-label^="Delete"]')).toBeNull();
    (items[0]!.querySelector('button[aria-label="Apply the Minimal preset"]') as HTMLElement).click();
    expect(f.componentInstance.edits.pop()).toEqual({ applyPreset: { id: 'min' } });
    f.componentInstance.v.set(withPresets(2, [preset('mine', 'Work', false, 1)], [preset('min', 'Minimal', true, 2)], { canUndo: true }));
    await settle(f);
    expect(document.querySelector('.mat-mdc-snack-bar-container')!.textContent).toContain('The popup now uses the Minimal preset.');
    (items[1]!.querySelector('button[aria-label="Delete the Work preset"]') as HTMLElement).click();
    await settle(f);
    expect(f.componentInstance.edits).toEqual([]);
    expect(document.activeElement).toBe(q(f, '[data-preset-confirm="mine"]'));
    expect(q(f, '.pd-wc-ask')!.textContent).toBe('Delete this preset?');
    // Cancel goes back to the row, focus on its delete button
    [...qa(f, 'button')].find((b) => b.textContent!.trim() === 'Cancel')!.click();
    await settle(f);
    expect(q(f, '.pd-wc-ask')).toBeNull();
    expect(document.activeElement).toBe(q(f, '[data-preset-delete="mine"]'));
    (q(f, '[data-preset-delete="mine"]') as HTMLElement).click();
    await settle(f);
    q(f, '[data-preset-confirm="mine"]')!.click();
    expect(f.componentInstance.edits.pop()).toEqual({ deletePreset: { id: 'mine' } });
    // the engine's answer arrives: focus goes to the previous preset's Apply
    f.componentInstance.v.set(withPresets(2, [], [preset('min', 'Minimal', true, 2)]));
    await settle(f);
    expect(document.activeElement).toBe(q(f, '[data-preset-apply="min"]'));
  });

  it('puts focus in the name field when the last preset is deleted', async () => {
    const f = await setup();
    f.componentInstance.v.set(withPresets(2, [preset('mine', 'Work', false, 1)], []));
    await settle(f);
    (q(f, '[data-preset-delete="mine"]') as HTMLElement).click();
    await settle(f);
    q(f, '[data-preset-confirm="mine"]')!.click();
    f.componentInstance.v.set(withPresets(2, [], []));
    await settle(f);
    expect(document.activeElement).toBe(nameInput(f));
  });

  it('shows the engine message for a refused edit', async () => {
    const f = await setup();
    expect(q(f, '.pd-wc-error')).toBeNull();
    f.componentInstance.err.set('At most 4 widgets fit in the popup; remove one first.');
    await settle(f);
    expect(q(f, '.pd-wc-error')!.getAttribute('role')).toBe('alert');
    expect(q(f, '.pd-wc-error')!.textContent).toContain('At most 4 widgets fit');
  });

  it('saves the layout under a trimmed name, clears the field, ignores an empty name and says when the limit is reached', async () => {
    const f = await setup();
    f.componentInstance.v.set(withPresets(1, [], []));
    await settle(f);
    expect(qa(f, 'ul[aria-labelledby$="-presets"]')).toHaveLength(0);
    expect(saveButton(f).disabled).toBe(true);
    nameInput(f).dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    expect(f.componentInstance.edits).toEqual([]);
    await type(f, '  Work  ');
    expect(saveButton(f).disabled).toBe(false);
    saveButton(f).click();
    expect(f.componentInstance.edits.pop()).toEqual({ savePreset: { name: 'Work' } });
    await settle(f);
    expect(nameInput(f).value).toBe('');
    await type(f, 'Again');
    nameInput(f).dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    expect(f.componentInstance.edits.pop()).toEqual({ savePreset: { name: 'Again' } });
    f.componentInstance.v.set(withPresets(1, [preset('mine', 'Work', false, 1)], []));
    await settle(f);
    await type(f, 'Third');
    expect(saveButton(f).disabled).toBe(true);
    expect(q(f, '.pd-wc-save + .pd-wc-limit')!.textContent).toContain('You can keep 1 preset. Delete one to save another.');
    f.componentInstance.v.set(withPresets(2, [preset('a', 'A', false, 1), preset('b', 'B', false, 1)], []));
    await settle(f);
    expect(q(f, '.pd-wc-save + .pd-wc-limit')!.textContent).toContain('You can keep 2 presets.');
  });

  it('shows built-in presets without a save form when the person may not save any, and nothing to change when read-only', async () => {
    const f = await setup();
    f.componentInstance.v.set(withPresets(0, [], [preset('min', 'Minimal', true, 2)]));
    await settle(f);
    expect(qa(f, 'ul[aria-labelledby$="-presets"] li')).toHaveLength(1);
    expect(q(f, '.pd-wc-save')).toBeNull();
    f.componentInstance.v.set(withPresets(2, [preset('mine', 'Work', false, 1)], [preset('min', 'Minimal', true, 2)], { editable: false }));
    await settle(f);
    expect(qa(f, 'ul[aria-labelledby$="-presets"] button')).toHaveLength(0);
    expect(q(f, '.pd-wc-save')).toBeNull();
  });

  it('shows the preview column only when asked', async () => {
    const f = await setup();
    expect(q(f, '.pd-wc-preview')).toBeNull();
    f.componentInstance.preview.set(true);
    await settle(f);
    expect(q(f, '.pd-wc-preview .pv')!.textContent).toBe('preview');
    expect(q(f, '.pd-wc--preview')).not.toBeNull();
  });
});
