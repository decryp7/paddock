import { ChangeDetectionStrategy, Component, provideZonelessChangeDetection, signal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { ContributionRegistry, defineNotificationKind, msg, provideContributions } from '@decryp7/paddock-ui/shell';
import { NotificationsStore } from './notifications.store';
import { noticeTime, notificationsStatusText, titleWithCount } from './notice-text';
import { PdNotificationsPanel } from './notifications-panel';
import { PdNotificationsStatus } from './notifications-status';
import { OPEN_SETTINGS, RAISE_CAP } from './notify-config';
import { provideNoticeAction } from './provide-notify';
import { PdServiceStatus } from './service-status';
import { chipCompactText, chipText, shortName, stateWord } from './service-status-text';
import { ServiceHealthStore } from './service-health.store';
import { all, cause, notifyBed, one, settle } from './spec-helpers';
import { offlineLine, PdServiceError, PdWaitingFor, serviceStatus, waitingKind } from './waiting-for';
import { ServiceHealthBackend, type Unlisten } from './backends';
import type { ServiceHealthSnapshot } from './wire.types';

describe('chip text', () => {
  it('names one service and its state, or counts several', () => {
    expect(shortName('the log store')).toBe('Log store');
    expect((['down', 'degraded', 'quarantined', 'repairing'] as const).map((s) => stateWord(s))).toEqual(['stopped', 'limited', 'switched off', 'repairing']);
    expect(chipText([{ label: 'the log store', state: 'down' }])).toBe('Log store: stopped');
    expect(chipText([{ label: 'a', state: 'down' }, { label: 'b', state: 'degraded' }])).toBe('2 services need attention');
    // The compact form of a narrow status bar.
    expect(chipCompactText([{ label: 'the log store', state: 'down' }])).toBe('Store stopped');
    expect(chipCompactText([{ label: 'index', state: 'degraded' }])).toBe('Index limited');
    expect(chipCompactText([{ label: 'a', state: 'down' }, { label: 'b', state: 'degraded' }])).toBe('2 services');
    expect(chipCompactText([])).toBe('Unknown');
  });

  it("keeps the app's casing: only a label with the article is capitalised, and only when its first word has no capital of its own", () => {
    expect(shortName('the S3 bucket')).toBe('S3 bucket');
    expect(shortName('the iPhone link')).toBe('iPhone link');
    expect(shortName('llama.cpp server')).toBe('llama.cpp server');
    expect(shortName('Pitwall server')).toBe('Pitwall server');
    expect(shortName('The log store')).toBe('Log store');
    expect(chipText([{ label: 'iPhone link', state: 'degraded' }])).toBe('iPhone link: limited');
    expect(chipText([{ label: 'llama.cpp', state: 'down' }])).toBe('llama.cpp: stopped');
    // The compact word starts the text: a plain lower-case one takes a capital, a name keeps its own.
    expect(chipCompactText([{ label: 'the iPhone link', state: 'down' }])).toBe('link stopped'.replace('link', 'Link'));
    expect(chipCompactText([{ label: 'the sync iPhone', state: 'down' }])).toBe('iPhone stopped');
  });
});

describe('chip text, sentence case and accurate state words', () => {
  const cause = (code: string | null) => ({ message: 'm', hint: null, detail: null, code, raiseCap: null, settings: null });

  it('starts with a capital when the label opens with a possessive or determiner', () => {
    expect(shortName('your sources')).toBe('Your sources');
    expect(shortName('my sync')).toBe('My sync');
    expect(chipText([{ label: 'your sources', state: 'down' }])).toBe('Your sources: stopped');
    expect(chipCompactText([{ label: 'your sources', state: 'down' }])).toBe('Sources stopped');
  });

  it('says what is wrong by the cause code, in lower case after the colon', () => {
    const svc = (code: string) => ({ label: 'your sources', state: 'down' as const, cause: cause(code) });
    expect(chipText([svc('service.notInstalled')])).toBe('Your sources: not installed');
    expect(chipText([svc('service.unavailable')])).toBe('Your sources: not available');
    expect(chipCompactText([svc('service.needsApproval')])).toBe('Sources needs approval');
    expect(chipText([svc('service.repairFailed')])).toBe('Your sources: stopped');
    expect(chipText([{ label: 'the AI', state: 'degraded', cause: cause('pitwall.needsPairing') }])).toBe('AI: needs pairing');
    // A code nobody listed (or none) is plain "stopped".
    expect(chipText([svc('x.unknown')])).toBe('Your sources: stopped');
    expect(stateWord('down', cause(null))).toBe('stopped');
  });
});

describe('pd-service-status', () => {
  async function open(f: { nativeElement: HTMLElement; whenStable(): Promise<unknown> }) {
    one(f.nativeElement, '[data-testid=service-health-chip]').click();
    await f.whenStable();
  }

  it('appears only when a service is not working, and repairs from the shell', async () => {
    const { health } = notifyBed();
    const f = TestBed.createComponent(PdServiceStatus);
    await settle();
    await f.whenStable();
    const el: HTMLElement = f.nativeElement;
    expect(el.querySelector('[data-testid=service-health-chip]')).toBeNull();
    health.fail('store', cause('The log store stopped.', { hint: 'Choose Restart.', detail: 'exit 6' }));
    await f.whenStable();
    expect(one(el, '[data-testid=service-health-chip]').textContent).toContain('Log store: stopped');
    await open(f);
    const section = one(el, '[data-testid=service-health-store]');
    expect(section.textContent).toContain('The log store stopped.');
    expect(section.textContent).toContain('Choose Restart.');
    expect(section.querySelector('details code')?.textContent).toBe('exit 6');
    one(el, '[data-testid=service-health-repair-store]').click();
    await settle();
    await f.whenStable();
    expect(health.repairs).toEqual(['store']);
    expect(el.textContent).toContain('Repairing…');
    expect(one(el, '[data-testid=service-health-chip]').classList).toContain('pd-repairing');
    health.settle('store');
    await f.whenStable();
    expect(el.querySelector('[data-testid=service-health-chip]')).toBeNull();
    // It does not reopen by itself with the next outage.
    health.fail('ai', cause('The AI worker stopped.'));
    health.fail('store', cause('Again.'), null);
    await f.whenStable();
    expect(one(el, '[data-testid=service-health-chip]').textContent).toContain('2 services need attention');
    expect(el.querySelector('[data-testid=service-health-details]')).toBeNull();
    await open(f);
    expect(el.querySelector('[data-testid=service-health-repair-store]')).toBeNull();
    expect(one(el, '[data-testid=service-health-store]').textContent).toContain('Again.');
  });

  it('shows why a repair was refused, and a cause without a message by name', async () => {
    const { health } = notifyBed();
    const f = TestBed.createComponent(PdServiceStatus);
    await settle();
    TestBed.inject(ServiceHealthStore).apply({ revision: 9, services: [{ service: 'store', label: 'the log store', state: 'down', cause: null, repair: 'restart' }] });
    await f.whenStable();
    await open(f);
    const el: HTMLElement = f.nativeElement;
    expect(el.textContent).toContain('Log store is not working.');
    one(el, '[data-testid=service-health-repair-store]').click();
    await settle();
    await f.whenStable();
    expect(one(el, '[data-testid=service-health-store] [role=alert]').textContent).toContain('nothing to repair');
    expect(health.repairs).toEqual([]);
  });

  it('opens Settings for the openSettings repair and offers Raise the cap when the app can', async () => {
    const opened: string[] = [];
    const raised: unknown[] = [];
    const { health } = notifyBed({ extra: [{ provide: OPEN_SETTINGS, useValue: (s: string) => void opened.push(s) }, { provide: RAISE_CAP, useValue: (c: unknown) => void raised.push(c) }] });
    const f = TestBed.createComponent(PdServiceStatus);
    await settle();
    health.fail('store', cause('Out of memory.', { settings: 'performance', raiseCap: { target: 'storeWorker', suggestedMb: 2048 } }), 'openSettings');
    await f.whenStable();
    await open(f);
    const el: HTMLElement = f.nativeElement;
    expect(one(el, '[data-testid=service-health-repair-store]').textContent).toContain('Open Settings…');
    one(el, '[data-testid=service-health-repair-store]').click();
    one(el, '[data-testid=service-health-raise-cap-store]').click();
    expect(opened).toEqual(['performance']);
    expect(raised).toEqual([{ target: 'storeWorker', suggestedMb: 2048 }]);
    expect(health.repairs).toEqual([]);
  });

  it('without the app’s openers there is no button that does nothing', async () => {
    const { health } = notifyBed();
    const f = TestBed.createComponent(PdServiceStatus);
    await settle();
    health.fail('store', cause('Out of memory.', { settings: 'performance', raiseCap: { target: 'storeWorker', suggestedMb: 2048 } }), 'openSettings');
    await f.whenStable();
    await open(f);
    expect(f.nativeElement.querySelector('[data-testid=service-health-repair-store]')).toBeNull();
    expect(f.nativeElement.querySelector('[data-testid=service-health-raise-cap-store]')).toBeNull();
  });

  it('says so when the state is not known, with a Try again that works', async () => {
    class Flaky extends ServiceHealthBackend {
      fail = true;
      snapshot(): Promise<ServiceHealthSnapshot> {
        return this.fail ? Promise.reject(new Error('The shell is starting.')) : Promise.resolve({ revision: 1, services: [] });
      }
      repair(): Promise<ServiceHealthSnapshot> {
        return this.snapshot();
      }
      listen(): Promise<Unlisten> {
        return Promise.resolve(() => undefined);
      }
    }
    const flaky = new Flaky();
    notifyBed({ health: flaky });
    const f = TestBed.createComponent(PdServiceStatus);
    await settle();
    await f.whenStable();
    const el: HTMLElement = f.nativeElement;
    expect(one(el, '[data-testid=service-health-chip]').textContent).toContain('Services: unknown');
    await open(f);
    expect(el.textContent).toContain('The shell is starting.');
    flaky.fail = false;
    one(el, '[data-testid=service-health-reload]').click();
    await settle();
    await f.whenStable();
    expect(el.querySelector('[data-testid=service-health-chip]')).toBeNull();
  });

  it('closes on Escape (focus back on the chip) and on a click outside; a waiting panel opens it', async () => {
    const { health } = notifyBed();
    const f = TestBed.createComponent(PdServiceStatus);
    f.componentRef.setInput('placement', 'below');
    await settle();
    health.fail('store', cause('x'));
    await f.whenStable();
    const el: HTMLElement = f.nativeElement;
    document.body.append(el);
    await open(f);
    expect(el.classList).toContain('pd-below');
    one(el, '[data-testid=service-health-details]').dispatchEvent(new Event('pointerdown', { bubbles: true }));
    await f.whenStable();
    expect(el.querySelector('[data-testid=service-health-details]')).not.toBeNull();
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }));
    await f.whenStable();
    expect(el.querySelector('[data-testid=service-health-details]')).toBeNull();
    expect(document.activeElement).toBe(one(el, '[data-testid=service-health-chip]'));
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }));
    TestBed.inject(ServiceHealthStore).showDetails();
    await f.whenStable();
    expect(el.querySelector('[data-testid=service-health-details]')).not.toBeNull();
    document.body.dispatchEvent(new Event('pointerdown', { bubbles: true }));
    await f.whenStable();
    expect(el.querySelector('[data-testid=service-health-details]')).toBeNull();
    document.body.dispatchEvent(new Event('pointerdown', { bubbles: true }));
    el.remove();
  });
});

@Component({
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [PdServiceError, PdWaitingFor],
  template: `
    <pd-service-error summary="Could not load events." service="store" detail="engine said no" (retry)="retries.update((n) => n + 1)" />
    <pd-service-error summary="Could not draw." [retryable]="false" />
    <pd-waiting-for service="nobody" />
  `,
})
class Panels {
  readonly retries = signal(0);
}

describe('a stopped service, in words and severity', () => {
  it('says what is missing and what brings it back, from the subject and the repair', () => {
    expect(offlineLine('Engine data', 'the demo engine', 'restart')).toBe('Engine data is unavailable until the demo engine restarts.');
    expect(offlineLine('Search', 'the search index', 'reset')).toBe('Search is unavailable until the search index is reset.');
    expect(offlineLine('Charts', 'the model', 'install')).toBe('Charts is unavailable until the model is installed.');
    expect(offlineLine('Events', 'the log store', null)).toBe('Events is unavailable until the log store works again.');
    expect(offlineLine(null, 'the demo engine', 'restart')).toBe('Demo engine is unavailable until it restarts.');
  });

  it('has one severity everywhere: not working is danger, under way neutral', () => {
    expect(['down', 'degraded', 'quarantined', 'repairing', 'starting', 'ok', null].map(serviceStatus)).toEqual([
      'danger',
      'danger',
      'danger',
      'neutral',
      'neutral',
      'success',
      'neutral',
    ]);
  });
});

describe('waitingKind', () => {
  it('spins only while something is under way; a stopped service is an offline state', () => {
    expect([null, 'starting', 'ok', 'repairing', 'down', 'degraded', 'quarantined'].map(waitingKind)).toEqual([
      'waiting',
      'waiting',
      'waiting',
      'repairing',
      'offline',
      'offline',
      'offline',
    ]);
  });
});

describe('pd-service-error and pd-waiting-for', () => {
  it('a panel waits calmly while its service is down and loads again by itself when it is back', async () => {
    const { health } = notifyBed();
    const f = TestBed.createComponent(Panels);
    const el: HTMLElement = f.nativeElement;
    f.detectChanges();
    // Before the first snapshot: waiting, but nothing was lost.
    expect(all(el, '[data-testid=waiting-for-store]')).toHaveLength(1);
    await settle();
    await f.whenStable();
    expect(f.componentInstance.retries()).toBe(0);
    expect(el.textContent).toContain('Could not load events.');
    expect(el.querySelector('details code')?.textContent).toBe('engine said no');
    expect(el.textContent).toContain('Could not draw.');
    expect(all(el, 'button').filter((b) => b.textContent?.trim() === 'Retry')).toHaveLength(1);
    expect(el.textContent).toContain('Waiting for nobody');
    health.fail('store', cause('stopped'));
    await f.whenStable();
    // Stopped: the offline state (no spinner) with the chip's repair, never "Waiting" (nothing happens by itself).
    const panel = one(el, '[data-testid=waiting-for-store]');
    expect(panel.dataset['kind']).toBe('offline');
    expect(panel.textContent).toContain('Log store stopped');
    expect(panel.textContent).toContain('Log store is unavailable until it restarts.');
    expect(panel.querySelector('svg.pd-icon')?.getAttribute('data-status')).toBe('danger');
    expect(panel.querySelector('.pd-spinner')).toBeNull();
    expect(el.textContent).not.toContain('Could not load events.');
    const store = TestBed.inject(ServiceHealthStore);
    const repair = vi.spyOn(store, 'repair').mockResolvedValue(undefined);
    one(el, '[data-testid=waiting-for-store] [data-testid=waiting-for-repair]').click();
    expect(one(el, '[data-testid=waiting-for-store] [data-testid=waiting-for-repair]').textContent?.trim()).toBe('Restart log store');
    expect(repair).toHaveBeenCalledWith('store');
    repair.mockRestore();
    // While the repair runs: the spinner, saying so.
    health.fail('store', cause('stopped'), 'restart', 'repairing');
    await f.whenStable();
    expect(one(el, '[data-testid=waiting-for-store]').dataset['kind']).toBe('repairing');
    expect(one(el, '[data-testid=waiting-for-store]').textContent).toContain('Repairing the log store…');
    expect(one(el, '[data-testid=waiting-for-store] .pd-spinner')).not.toBeNull();
    one(el, '[data-testid=waiting-for-store] [data-testid=waiting-for-details]').click();
    expect(TestBed.inject(ServiceHealthStore).detailsRequested()).toBe(1);
    health.ok('store');
    await f.whenStable();
    expect(f.componentInstance.retries()).toBe(1);
    all(el, 'button').find((b) => b.textContent?.trim() === 'Retry')?.click();
    expect(f.componentInstance.retries()).toBe(2);
  });
});

describe('the notification centre', () => {
  it('titles and times', () => {
    expect(titleWithCount({ title: 'Spike', count: 4 })).toBe('Spike (×4)');
    expect(titleWithCount({ title: 'Spike', count: 1 })).toBe('Spike');
    const at = new Date(2026, 9, 24, 14, 5).getTime();
    expect(noticeTime(at, at)).toBe('14:05');
    expect(noticeTime(at, at + 3 * 86_400_000)).toBe('24 Oct, 14:05');
    expect(notificationsStatusText(0)).toBe('Notifications');
    expect(notificationsStatusText(3)).toBe('3 unread');
  });

  function bed() {
    const installs: unknown[] = [];
    const b = notifyBed({
      extra: [
        provideContributions({ notificationKinds: [defineNotificationKind({ id: 'sd.save', title: msg('sd.notification.save', 'Saving'), actions: [] })] }),
        provideNoticeAction({ type: 'install', label: () => 'Install', run: (a) => void installs.push(a) }),
        provideNoticeAction({ type: 'broken', label: () => 'Break', run: () => Promise.reject(new Error('it broke')) }),
      ],
    });
    return { ...b, installs };
  }

  it('lists messages grouped by source with their actions, read state and Dismiss', async () => {
    const { centre, health, installs } = bed();
    const f = TestBed.createComponent(PdNotificationsPanel);
    await settle();
    const el: HTMLElement = f.nativeElement;
    await f.whenStable();
    expect(one(el, '[data-testid=notices-empty]').textContent).toContain('Nothing needs your attention');
    health.fail('store', cause('The log store stopped.', { hint: 'Choose Restart.' }));
    health.fail('store', cause('The log store stopped.', { hint: 'Choose Restart.' }));
    const update = centre.post({ cause: 'update:2', source: 'update', severity: 'info', title: 'Update ready', primary: { type: 'install', version: '2' }, secondary: { type: 'unknown' } });
    await TestBed.inject(NotificationsStore).post({ level: 'warning', kind: 'sd.save', message: 'Saved with warnings' });
    await f.whenStable();
    // Newest first (same time: the later post first); the window's notice is the engine's message now.
    expect(all(el, 'h3').map((h) => h.textContent)).toEqual(['Saving', 'Update', 'Services']);
    const outage = one(el, '[data-cause="health:store"]');
    expect(outage.textContent).toContain('The log store stopped. (×2)');
    expect(outage.textContent).toContain('Choose Restart.');
    expect(outage.querySelector('button.pd-btn')?.textContent).toContain('Restart');
    (outage.querySelector('button.pd-btn') as HTMLButtonElement).click();
    await settle();
    expect(health.repairs).toEqual(['store']);
    const row = one(el, `[data-testid=notice-${update.id}]`);
    expect(all(row, '.pd-actions button').map((b) => b.textContent?.trim())).toEqual(['Install']);
    (row.querySelector('.pd-actions button') as HTMLButtonElement).click();
    await settle();
    expect(installs).toEqual([{ type: 'install', version: '2' }]);
    one(el, `[data-testid=notice-read-${update.id}]`).click();
    await settle();
    await f.whenStable();
    expect(el.querySelector(`[data-testid=notice-read-${update.id}]`)).toBeNull();
    one(el, `[data-testid=notice-dismiss-${update.id}]`).click();
    await settle();
    await f.whenStable();
    expect(el.querySelector(`[data-testid=notice-${update.id}]`)).toBeNull();
    one(el, '[data-testid=notices-mark-all]').click();
    await settle();
    await f.whenStable();
    expect((one(el, '[data-testid=notices-mark-all]') as HTMLButtonElement).disabled).toBe(true);
  });

  it('a long title uses the width it has: it wraps (two lines at most) and the hidden row buttons reserve no room', async () => {
    const { centre } = bed();
    const f = TestBed.createComponent(PdNotificationsPanel);
    const title = 'Some of your places in the log sources could not be read because the folder moved';
    const n = centre.post({ cause: 'long:1', source: 'update', severity: 'warning', title });
    await settle();
    await f.whenStable();
    const el: HTMLElement = f.nativeElement;
    document.body.appendChild(el);
    try {
      const heading = one(el, `[data-testid=notice-${n.id}] .pd-title`);
      const style = getComputedStyle(heading);
      expect(style.whiteSpace).not.toBe('nowrap');
      expect(heading.getAttribute('title')).toBe(title);
      expect(getComputedStyle(one(el, `[data-testid=notice-${n.id}] .pd-row-actions`)).position).toBe('absolute');
    } finally {
      el.remove();
    }
  });

  it('a failed action says why; the Unread filter; an error with Retry', async () => {
    const { centre } = bed();
    const f = TestBed.createComponent(PdNotificationsPanel);
    await settle();
    const el: HTMLElement = f.nativeElement;
    const broken = centre.post({ cause: 'x', source: 'job', severity: 'error', title: 'Broken', primary: { type: 'broken' } });
    const read = centre.post({ cause: 'y', source: 'job', severity: 'success', title: 'Done already' });
    await centre.markRead(read.id);
    await f.whenStable();
    one(el, `[data-testid=notice-${broken.id}] .pd-actions button`).click();
    await settle();
    await f.whenStable();
    expect(one(el, 'p.pd-err').textContent).toContain('it broke');
    expect(all(el, 'h3').map((h) => h.textContent)).toEqual(['Jobs']);
    (all(el, '[role=radio]')[1] as HTMLButtonElement).click();
    await f.whenStable();
    expect(el.textContent).not.toContain('Done already');
    const store = TestBed.inject(NotificationsStore);
    vi.spyOn(centre, 'list').mockRejectedValueOnce(new Error('The centre is starting.'));
    await store.load();
    await f.whenStable();
    expect(one(el, 'div.pd-err').textContent).toContain('The centre is starting.');
    one(el, 'div.pd-err button').click();
    await settle();
    await f.whenStable();
    expect(el.querySelector('div.pd-err')).toBeNull();
  });

  it('reveals a focused message; works without a registry', async () => {
    const { centre } = notifyBed();
    TestBed.overrideProvider(ContributionRegistry, { useValue: null });
    const f = TestBed.createComponent(PdNotificationsPanel);
    await settle();
    const n = centre.post({ cause: 'z', source: 'custom', severity: 'warning', title: 'Look' });
    await f.whenStable();
    TestBed.inject(NotificationsStore).setFocus(n.id);
    await f.whenStable();
    await Promise.resolve();
    const row = one(f.nativeElement, `[data-testid=notice-${n.id}]`);
    expect(row.classList).toContain('pd-focus');
    expect(document.activeElement === row.querySelector('button') || !row.isConnected).toBe(true);
    expect(one(f.nativeElement, 'h3').textContent).toBe('Custom');
    TestBed.inject(NotificationsStore).setFocus(9999);
    await f.whenStable();
    await Promise.resolve();
  });
});

describe('pd-notifications-status', () => {
  it('draws the count in the danger colour while an error is unread, and neutral otherwise', async () => {
    const { centre } = notifyBed();
    const f = TestBed.createComponent(PdNotificationsStatus);
    await settle();
    const el: HTMLElement = f.nativeElement;
    document.body.append(el);
    centre.post({ cause: 'a', source: 'job', severity: 'info', title: 'A' });
    await f.whenStable();
    expect(one(el, '.pd-count').classList.contains('pd-error')).toBe(false);
    const bad = centre.post({ cause: 'b', source: 'job', severity: 'error', title: 'B' });
    await f.whenStable();
    expect(one(el, '.pd-count').classList.contains('pd-error')).toBe(true);
    expect(one(el, '.pd-count').textContent).toBe('2');
    await centre.markRead(bad.id);
    await f.whenStable();
    expect(one(el, '.pd-count').classList.contains('pd-error')).toBe(false);
    el.remove();
  });

  it('shows the unread count, toggles the centre and opens by itself for a focused message', async () => {
    const { centre } = notifyBed();
    const f = TestBed.createComponent(PdNotificationsStatus);
    f.componentRef.setInput('placement', 'below');
    await settle();
    const el: HTMLElement = f.nativeElement;
    document.body.append(el);
    await f.whenStable();
    const pill = one(el, '[data-testid=notifications-status]');
    expect(pill.getAttribute('aria-label')).toBe('Notifications');
    const n = centre.post({ cause: 'a', source: 'job', severity: 'info', title: 'A' });
    await f.whenStable();
    expect(pill.getAttribute('aria-label')).toBe('1 unread');
    expect(one(el, '.pd-count').textContent).toBe('1');
    pill.click();
    await f.whenStable();
    expect(el.querySelector('pd-notifications-panel')).not.toBeNull();
    pill.click();
    await f.whenStable();
    expect(el.querySelector('pd-notifications-panel')).toBeNull();
    TestBed.inject(NotificationsStore).setFocus(n.id);
    await f.whenStable();
    expect(el.querySelector('pd-notifications-panel')).not.toBeNull();
    el.querySelector('pd-notifications-panel')?.dispatchEvent(new Event('pointerdown', { bubbles: true }));
    await f.whenStable();
    expect(el.querySelector('pd-notifications-panel')).not.toBeNull();
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }));
    await f.whenStable();
    expect(el.querySelector('pd-notifications-panel')).toBeNull();
    expect(TestBed.inject(NotificationsStore).focus()).toBeNull();
    expect(document.activeElement).toBe(pill);
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }));
    pill.click();
    await f.whenStable();
    document.body.dispatchEvent(new Event('pointerdown', { bubbles: true }));
    await f.whenStable();
    expect(el.querySelector('pd-notifications-panel')).toBeNull();
    document.body.dispatchEvent(new Event('pointerdown', { bubbles: true }));
    el.remove();
  });
});

void provideZonelessChangeDetection;
