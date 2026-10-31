import { ChangeDetectionStrategy, Component, signal } from '@angular/core';
import { MatTooltip } from '@angular/material/tooltip';
import { By } from '@angular/platform-browser';
import { TestBed } from '@angular/core/testing';
import { PdWidgetArea } from '@decryp7/paddock-ui/widgets';
import type { PopoverValue, WidgetActionId } from '../../core/generated/widgets';
import type { PopoverState, PopoverValues, PopoverView } from '../../core/widgets.types';
import { MockPopover } from '../../core/mock/mock-popover';
import type { PopoverWidget } from '../../core/generated/widgets';
import { appProviders, makeMock, NOW, settle, text } from '../../../testing/test-helpers';
import { ClockService } from '../../core/clock.service';
import { TrayWidgetsStore } from '../../state/tray-widgets.store';
import { WidgetSet } from './widget-set';

const ready = (value: PopoverValue): PopoverState => ({ state: 'ready', value, asOf: NOW, staleSince: null });
/** A popup showing every kind this spec draws, in this order, whatever the defaults of a fresh install are. */
const SHOWN: PopoverWidget[] = ['recentChanges', 'watchedSites', 'failures', 'nextCheck', 'notifications'];
const view: PopoverView = new MockPopover(SHOWN).view();
const ids = view.layout.placements.map((p) => p.id);

/** One state per placement, in layout order: recentChanges, watchedSites, failures, nextCheck, notifications. */
function values(states: PopoverState[]): PopoverValues {
  return { notice: null, placements: ids.map((id, i) => ({ id, state: states[i]! })) };
}

const FULL = values([
  ready({ kind: 'recentChanges', rows: [{ id: 'w1', name: 'Nightly build', version: 'R3.10.00T32', firstSeenMs: NOW - 3_600_000, unseen: true }, { id: 'w2', name: 'Tool', version: '1.2.0', firstSeenMs: NOW - 86_400_000, unseen: false }], more: 2, unseen: 1 }),
  ready({
    kind: 'watchedSites',
    total: 6,
    rows: [
      { id: 'w1', name: 'Nightly build', state: 'attention', version: null },
      { id: 'w2', name: 'Tool', state: 'new', version: '1.2.0' },
      { id: 'w3', name: 'Other', state: 'paused', version: '2.0' },
      { id: 'w4', name: 'Running', state: 'checking', version: null },
    ],
    more: 2,
  }),
  ready({ kind: 'failures', rows: [{ id: 'w1', name: 'Nightly build', kind: 'unreachable', message: 'The share is offline.', failures: 3 }], more: 0 }),
  ready({ kind: 'nextCheck', id: 'w2', name: 'Tool', atMs: NOW + 12 * 60_000, alsoDue: 1, due: false }),
  ready({ kind: 'notifications', mode: { kind: 'paused', untilMs: NOW + 3_600_000 } }),
]);

@Component({
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [PdWidgetArea, WidgetSet],
  template: `
    <sp-widget-set #set (run)="ran.push($event)" />
    <pd-widget-area [placements]="view().layout.placements" [catalogue]="view().catalogue" [values]="vals()" [templates]="set.widgets()" [meta]="set.meta" [now]="now" (action)="ran.push($event)" />
  `,
})
class Host {
  readonly view = signal<PopoverView>(view);
  readonly now = NOW;
  readonly vals = signal<PopoverValues | null>(FULL);
  readonly ran: WidgetActionId[] = [];
}

async function make(vals: PopoverValues | null = FULL) {
  TestBed.configureTestingModule({ providers: appProviders(makeMock()) });
  TestBed.inject(ClockService).set(NOW);
  await TestBed.inject(TrayWidgetsStore).load(); // the loading twins' rows come from the engine's shape
  const f = TestBed.createComponent(Host);
  f.componentInstance.vals.set(vals);
  await settle(f);
  const frames = () => Array.from((f.nativeElement as HTMLElement).querySelectorAll<HTMLElement>('pd-widget-frame'));
  return { f, frames, ran: f.componentInstance.ran };
}

describe('Spotter widget set', () => {
  afterEach(() => vi.restoreAllMocks());

  it('draws every placed kind in the shared frame, in layout order', async () => {
    const { frames } = await make();
    expect(frames().map((x) => text(x.querySelector('.pd-w-title, h3, [id^=pd-widget]')).split(' ')[0])).toHaveLength(SHOWN.length);
    expect(frames().map((x) => text(x))).toEqual([
      expect.stringContaining('Recent changes'),
      expect.stringContaining('Watched sites'),
      expect.stringContaining('Failures'),
      expect.stringContaining('Next check'),
      expect.stringContaining('Notifications'),
    ]);
  });

  it('says each site state in words next to its icon, never by colour alone', async () => {
    const { frames } = await make();
    const sites = text(frames()[1]!);
    for (const word of ['Needs attention', 'New release', 'Paused', 'Checking']) expect(sites, word).toContain(word);
    expect(sites).toContain('+2 more in Spotter');
    for (const row of frames()[1]!.querySelectorAll('button.row')) expect(row.querySelector('mat-icon')).not.toBeNull();
  });

  it('shows versions, new marks and how long ago a release was found', async () => {
    const { frames } = await make();
    const changes = text(frames()[0]!);
    expect(changes).toContain('R3.10.00');
    expect(changes).toContain('New');
    expect(changes).toContain('1 h');
    expect(changes, 'the head says how many are new').toContain('1 new');
    expect(text(frames()[2]!), 'and how many are failing').toContain('1 failing');
  });

  it('opens the watch of a row, and retries a failing watch', async () => {
    const { frames, ran, f } = await make();
    (frames()[0]!.querySelector('button.row') as HTMLButtonElement).click();
    (frames()[2]!.querySelector('button.retry') as HTMLButtonElement).click();
    (frames()[3]!.querySelector('button.block') as HTMLButtonElement).click();
    (frames()[4]!.querySelector('button.link') as HTMLButtonElement).click();
    await settle(f);
    expect(ran).toEqual([{ type: 'openWatch', id: 'w1' }, { type: 'retryCheck', id: 'w1' }, { type: 'openWatch', id: 'w2' }, { type: 'resumeNotifications' }]);
    expect(frames()[2]!.querySelector('button.retry')?.getAttribute('aria-label')).toBe('Retry check of Nightly build');
    expect(text(frames()[2]!)).toContain('Unreachable: The share is offline.');
  });

  it('says when the next check is and how many watches are due with it', async () => {
    const { frames } = await make();
    const next = text(frames()[3]!);
    expect(next).toContain('Tool');
    expect(next).toContain('+1 more');
    expect(frames()[3]!.querySelector('button.block')?.getAttribute('aria-label')).toMatch(/^Next check: Tool in .+, open$/);
  });

  it('says "Due now" for a check that is due or overdue, never "In now", also in the label', async () => {
    for (const atMs of [NOW, NOW - 90_000]) {
      const due = values(FULL.placements.map((p, i) => (i === 3 ? ready({ kind: 'nextCheck', id: 'w2', name: 'Tool', atMs, alsoDue: 0, due: true }) : p.state)));
      const { frames } = await make(due);
      expect(text(frames()[3]!)).toContain('Due now');
      expect(text(frames()[3]!)).not.toMatch(/\bIn (now|\d)/);
      expect(frames()[3]!.querySelector('button.block')?.getAttribute('aria-label')).toBe('Next check: Tool is due now, open');
      TestBed.resetTestingModule();
    }
    const { frames } = await make();
    expect(text(frames()[3]!)).toMatch(/In \d+ min/);
  });

  it('puts the failing watch\'s whole message in a tooltip and draws no chevron on a failure row', async () => {
    const { frames, f } = await make();
    const row = frames()[2]!.querySelector('button.row')!;
    expect(row.querySelector('.meta')?.hasAttribute('title'), 'a Material tooltip, not a title attribute').toBe(false);
    expect(row.getAttribute('aria-label'), 'the keyboard and screen reader get the whole message').toContain('The share is offline.');
    expect(f.debugElement.queryAll(By.css('button.row')).some((d) => d.injector.get(MatTooltip, null) !== null), 'the tooltip is on the focusable row').toBe(true);
    expect(f.debugElement.queryAll(By.directive(MatTooltip)).map((d) => (d.injector.get(MatTooltip) as MatTooltip).message)).toContain('Unreachable: The share is offline.');
    expect(row.querySelector('.chev')).toBeNull();
  });

  it('words the notifications as on, paused until a time, or off with a way to Settings', async () => {
    const { frames, f, ran } = await make();
    expect(text(frames()[4]!)).toMatch(/Paused until/);
    expect(text(frames()[4]!)).toContain('Resume notifications');
    f.componentInstance.vals.set(values([FULL.placements[0]!.state, FULL.placements[1]!.state, FULL.placements[2]!.state, FULL.placements[3]!.state, ready({ kind: 'notifications', mode: { kind: 'on' } })]));
    await settle(f);
    expect(text(frames()[4]!)).toContain('On');
    expect(text(frames()[4]!)).toContain('You will hear about new releases');
    f.componentInstance.vals.set(values([FULL.placements[0]!.state, FULL.placements[1]!.state, FULL.placements[2]!.state, FULL.placements[3]!.state, ready({ kind: 'notifications', mode: { kind: 'off' } })]));
    await settle(f);
    expect(text(frames()[4]!)).toContain('Off');
    (frames()[4]!.querySelector('button.link') as HTMLButtonElement).click();
    expect(ran).toEqual([{ type: 'openSettings' }]);
  });

  it('shows quiet hours with whether they are in force', async () => {
    const mock = new MockPopover(SHOWN);
    mock.edit({ add: { widget: 'quietHours', span: null, at: 0 } });
    const rest = FULL.placements.map((p) => p.state);
    const quiet = (active: boolean): PopoverValues => ({
      notice: null,
      placements: mock.view().layout.placements.map((p, i) => ({ id: p.id, state: i === 0 ? ready({ kind: 'quietHours', start: '22:00', end: '07:00', active }) : rest[i - 1]! })),
    });
    const { f, frames } = await make(quiet(false));
    f.componentInstance.view.set(mock.view());
    await settle(f);
    expect(text(frames()[0]!)).toContain('22:00 – 07:00');
    expect(text(frames()[0]!)).toContain('Not now');
    f.componentInstance.vals.set(quiet(true));
    await settle(f);
    expect(text(frames()[0]!)).toContain('In force now');
  });

  it('draws the loading twin of every kind when there are no values yet (same markup, skeleton parts, busy)', async () => {
    const { frames } = await make(null);
    expect(frames()).toHaveLength(SHOWN.length);
    for (const frame of frames()) {
      expect(frame.querySelector('.pd-w--loading, [aria-busy="true"]')).not.toBeNull();
      expect(frame.querySelectorAll('.pd-sk').length).toBeGreaterThan(0);
      for (const b of frame.querySelectorAll<HTMLButtonElement>('button.row, button.retry')) expect(b.disabled).toBe(true);
    }
  });

  it('keeps empty, working and error states in the frame, with the next step as a button', async () => {
    const empty: PopoverState = { state: 'empty', title: 'See your watches here', detail: 'No watches yet.', next: { action: { type: 'addWatch' }, label: 'Add a watch' }, calm: false };
    const working: PopoverState = { state: 'working', title: 'Checking now', phase: 'Checking watches', progress: { count: { done: 2, unit: 'watches' } }, last: null, cancel: null };
    const error: PopoverState = { state: 'error', title: "Couldn't read this", cause: 'The engine did not answer.', next: { action: { type: 'checkAll' }, label: 'Retry' } };
    const { frames, ran, f } = await make(values([empty, error, working, empty, empty]));
    expect(text(frames()[0]!)).toContain('See your watches here');
    expect(text(frames()[1]!)).toContain("Couldn't read this");
    expect(text(frames()[2]!)).toContain('Checking watches');
    const button = Array.from(frames()[0]!.querySelectorAll('button')).find((b) => text(b) === 'Add a watch')!;
    button.click();
    Array.from(frames()[1]!.querySelectorAll('button')).find((b) => text(b).includes('Retry'))!.click();
    await settle(f);
    expect(ran).toEqual([{ type: 'addWatch' }, { type: 'checkAll' }]);
  });

  it('draws the all-good check row only when the engine says the empty state is calm', async () => {
    const calmState = (calm: boolean): PopoverState => ({ state: 'empty', title: 'No failures', detail: 'Last checked 12 min ago.', next: { action: { type: 'openWatches' }, label: 'Open watches' }, calm });
    const states = (calm: boolean) => {
      const base = FULL.placements.map((p) => p.state);
      base[2] = calmState(calm);
      return values(base);
    };
    const calm = await make(states(true));
    expect(calm.frames()[2]!.querySelector('.pd-w-calm'), 'calm: the check row').not.toBeNull();
    expect(text(calm.frames()[2]!)).toContain('Last checked 12 min ago.');
    TestBed.resetTestingModule();
    const notCalm = await make(states(false));
    expect(notCalm.frames()[2]!.querySelector('.pd-w-calm'), 'not calm: no check row').toBeNull();
  });
});
