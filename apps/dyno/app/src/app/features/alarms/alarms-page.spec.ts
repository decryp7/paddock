import { TestBed } from '@angular/core/testing';
import { DialogService } from '@decryp7/paddock-ui/shell/dialogs';
import { provideRouter } from '@angular/router';
import { BACKEND } from '../../core/backend';
import { MockBackend } from '../../core/mock-backend';
import { SettingsStore } from '../../state/settings.store';
import { AlarmsPage } from './alarms-page';

const NOW = 1_700_000_000_000;

function render(fail: string[] = []) {
  const mock = new MockBackend({ latencyMs: 0, now: () => NOW, fail: new Set(fail) });
  TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: mock }, provideRouter([])] });
  const f = TestBed.createComponent(AlarmsPage);
  f.detectChanges();
  return { f, el: f.nativeElement as HTMLElement, mock };
}
const settle = async (f: { detectChanges(): void; whenStable(): Promise<unknown> }) => {
  for (let i = 0; i < 6; i++) {
    await f.whenStable();
    await new Promise((r) => setTimeout(r));
    f.detectChanges();
  }
};
const text = (el: Element) => (el.textContent ?? '').replace(/\s+/g, ' ').trim();
const button = (root: ParentNode, label: string | RegExp) =>
  [...root.querySelectorAll<HTMLButtonElement>('button')].find((b) => (typeof label === 'string' ? text(b) === label || b.getAttribute('aria-label') === label : label.test(text(b)) || label.test(b.getAttribute('aria-label') ?? '')));
const menuItem = (label: string | RegExp) =>
  [...document.querySelectorAll<HTMLElement>('[cdkMenuItem], .menu-item')].find((b) => (typeof label === 'string' ? text(b) === label : label.test(text(b))));
const tab = async (f: Parameters<typeof settle>[0], el: HTMLElement, name: string) => {
  button(el, name)!.click();
  await settle(f);
};
const rows = (el: ParentNode) => [...el.querySelectorAll<HTMLElement>('.row')];

beforeEach(() => vi.spyOn(Date, 'now').mockReturnValue(NOW));
afterEach(() => {
  vi.restoreAllMocks();
  document.querySelectorAll('.cdk-overlay-container').forEach((c) => (c.innerHTML = ''));
});

describe('dy-alarms-page, Active', () => {
  it('lists the open alarms newest first with calm severity and the state as dot and word', async () => {
    const { f, el } = render();
    await settle(f);
    expect(el.querySelector('h1')?.textContent).toBe('Alarms');
    const list = rows(el);
    expect(list).toHaveLength(5);
    expect(text(list[0]!)).toContain('GPU limit is past its limit');
    expect(text(list[0]!)).toContain('Active');
    expect(text(list[0]!)).not.toContain('Warning');
    expect(el.textContent).toContain('Returned, not acknowledged');
    expect(el.textContent).toContain('Acknowledged');
    expect(list.filter((r) => text(r).includes('Critical'))).toHaveLength(1);
    expect(text(list[0]!)).toContain('×14');
    expect(el.querySelectorAll('.unread')).toHaveLength(4);
  });

  it('shows shelved alarms with one muted suffix instead of a pill', async () => {
    const { f, el } = render();
    await settle(f);
    const shelved = rows(el).find((r) => text(r).includes('rustc has used'))!;
    expect(shelved.querySelector('.sfx')?.textContent).toMatch(/^Shelved until \d\d:\d\d$/);
    expect(el.querySelectorAll('.badge, .pill')).toHaveLength(0);
  });

  it('acknowledges everything that waits with the section action, the only primary button', async () => {
    const { f, el } = render();
    await settle(f);
    const primaries = [...el.querySelectorAll('button[matButton="filled"]')];
    expect(primaries).toHaveLength(1);
    expect(text(primaries[0]!)).toBe('Acknowledge all (4)');
    (primaries[0] as HTMLButtonElement).click();
    await settle(f);
    expect(el.querySelector('button[matButton="filled"]')).toBeNull();
    expect(el.querySelectorAll('.unread')).toHaveLength(0);
    expect(text(el)).toContain('0 not acknowledged');
  });

  it('acknowledges one alarm from its hover button', async () => {
    const { f, el } = render();
    await settle(f);
    button(rows(el)[1]!, /^Acknowledge node/)!.click();
    await settle(f);
    expect(text(rows(el)[1]!)).toContain('Acknowledged');
    expect(button(rows(el)[1]!, /^Acknowledge node/)!.disabled).toBe(true);
  });

  it('shelves from the clock button menu', async () => {
    const { f, el, mock } = render();
    await settle(f);
    button(rows(el)[1]!, /^Shelve node/)!.click();
    await settle(f);
    expect(['1 hour', 'Until tomorrow', 'Until restart'].every((l) => menuItem(l))).toBe(true);
    menuItem('Until restart')!.click();
    await settle(f);
    expect(text(rows(el)[1]!)).toContain('Shelved until restart');
    expect(mock.isShelved((await mock.alarms({ states: [], minSeverity: null, rule: null, includeShelved: true, limit: 10 })).find((a) => a.id === 7)!)).toBe(true);
  });

  it('opens the row menu on right click with every action and applies one', async () => {
    const { f, el } = render();
    await settle(f);
    rows(el)[1]!.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, clientX: 50, clientY: 50, button: 2 }));
    await settle(f);
    for (const l of ['Acknowledge', /^Shelve/, 'Add note…', 'Mute this rule', 'Show history']) expect(menuItem(l), String(l)).toBeTruthy();
    expect(menuItem('Unshelve')).toBeUndefined();
    menuItem('Mute this rule')!.click();
    await settle(f);
    expect(text(rows(el)[1]!)).toContain('Muted');
    button(rows(el)[1]!, /^More actions/)!.click();
    await settle(f);
    expect(menuItem('Unmute this rule')).toBeTruthy();
  });

  it('offers Unshelve only for a shelved alarm and applies it', async () => {
    const { f, el } = render();
    await settle(f);
    const row = rows(el).find((r) => text(r).includes('rustc has used'))!;
    button(row, /^More actions/)!.click();
    await settle(f);
    menuItem('Unshelve')!.click();
    await settle(f);
    expect(text(rows(el).find((r) => text(r).includes('rustc has used'))!)).not.toContain('Shelved');
  });

  it('opens the detail panel with full values, notes and history, and closes it with Esc', async () => {
    const { f, el } = render();
    await settle(f);
    rows(el)[3]!.click();
    await settle(f);
    const panel = el.querySelector('aside')!;
    expect(text(panel)).toContain('Macintosh HD is 78% full');
    expect(text(panel)).toContain('Waiting for the Xcode caches');
    expect(text(panel)).toContain('Raised');
    expect(panel.querySelector('a[href="/recorder"]')).toBeTruthy();
    panel.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    await settle(f);
    expect(el.querySelector('aside')).toBeNull();
  });

  it('adds a note from the panel and clears the field', async () => {
    const { f, el } = render();
    await settle(f);
    rows(el)[0]!.click();
    await settle(f);
    const input = el.querySelector<HTMLInputElement>('aside input')!;
    const add = button(el.querySelector('aside')!, 'Add')!;
    expect(add.disabled).toBe(true);
    input.value = 'Rebooting later';
    input.dispatchEvent(new Event('input'));
    await settle(f);
    add.click();
    await settle(f);
    expect(text(el.querySelector('aside')!)).toContain('Rebooting later');
    expect(el.querySelector<HTMLInputElement>('aside input')!.value).toBe('');
  });

  it('"Add note…" from the menu focuses the note field; the panel buttons act on the alarm', async () => {
    const { f, el } = render();
    await settle(f);
    button(rows(el)[0]!, /^More actions/)!.click();
    await settle(f);
    menuItem('Add note…')!.click();
    await settle(f);
    expect(document.activeElement).toBe(el.querySelector('aside input'));
    const aside = el.querySelector('aside')!;
    button(aside, 'Mute rule')!.click();
    await settle(f);
    expect(button(el.querySelector('aside')!, 'Unmute rule')).toBeTruthy();
    button(aside, /Acknowledge/)!.click();
    await settle(f);
    expect(text(el.querySelector('aside')!)).toContain('Acknowledged');
    button(el.querySelector('aside')!, /Shelve/)!.click();
    await settle(f);
    menuItem('1 hour')!.click();
    await settle(f);
    expect(text(el.querySelector('aside')!)).toContain('Shelved until');
    button(el.querySelector('aside')!, /Unshelve/)!.click();
    await settle(f);
    expect(text(el.querySelector('aside')!)).not.toContain('Shelved until');
  });

  it('moves between rows with the arrow keys and opens the context menu from the keyboard', async () => {
    const { f, el } = render();
    await settle(f);
    const grid = el.querySelector('.grid')!;
    rows(el)[0]!.focus();
    grid.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true }));
    expect(document.activeElement).toBe(rows(el)[1]);
    grid.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowUp', bubbles: true }));
    expect(document.activeElement).toBe(rows(el)[0]);
    rows(el)[1]!.focus();
    grid.dispatchEvent(new KeyboardEvent('keydown', { key: 'F10', shiftKey: true, bubbles: true }));
    await settle(f);
    expect(menuItem('Acknowledge')).toBeTruthy();
    rows(el)[0]!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    await settle(f);
    expect(el.querySelector('aside')).toBeTruthy();
    grid.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    await settle(f);
    expect(el.querySelector('aside')).toBeNull();
    grid.dispatchEvent(new KeyboardEvent('keydown', { key: 'x', bubbles: true }));
  });

  it('shows a live change pushed by the agent', async () => {
    const { f, el, mock } = render();
    await settle(f);
    await mock.acknowledgeAlarms([6]);
    await settle(f);
    expect(text(rows(el)[0]!)).toContain('Acknowledged');
    expect(el.querySelectorAll('.unread')).toHaveLength(3);
  });

  it('shows the empty state when nothing is open', async () => {
    vi.spyOn(MockBackend.prototype, 'alarms').mockResolvedValue([]);
    const { f, el } = render();
    await settle(f);
    expect(el.textContent).toContain('No open alarms');
    expect(el.querySelector('button[matButton="filled"]')).toBeNull();
  });

  it('shows an inline error with Retry when the alarms cannot be read', async () => {
    const { f, el } = render(['alarms']);
    await settle(f);
    expect(el.querySelector('[role="alert"]')).toBeTruthy();
    expect(button(el, /Retry/)).toBeTruthy();
    expect(rows(el)).toHaveLength(0);
  });
});

describe('dy-alarms-page, History', () => {
  it('lists the changes with sentence-case kinds', async () => {
    const { f, el } = render();
    await settle(f);
    await tab(f, el, 'History');
    const list = rows(el);
    expect(list.length).toBeGreaterThan(5);
    expect(list.map((r) => text(r)).join('|')).toContain('Returned to normal');
    expect(list.map((r) => text(r)).join('|')).toContain('Acknowledged');
    expect(el.querySelector('[role="tab"][aria-selected="true"]')?.textContent).toBe('History');
  });

  it('shows an error with Retry, then the list', async () => {
    const { f, el, mock } = render();
    await settle(f);
    vi.spyOn(mock, 'alarmHistory').mockRejectedValueOnce({ code: 'timeout', message: 'Too slow.' });
    await tab(f, el, 'History');
    expect(text(el)).toContain('Too slow.');
    button(el, /Retry/)!.click();
    await settle(f);
    expect(rows(el).length).toBeGreaterThan(5);
  });

  it('shows the empty state', async () => {
    const { f, el, mock } = render();
    await settle(f);
    vi.spyOn(mock, 'alarmHistory').mockResolvedValue([]);
    await tab(f, el, 'History');
    expect(el.textContent).toContain('No alarm history yet');
  });

  it('does not fetch the history twice when the tab is revisited', async () => {
    const { f, el, mock } = render();
    await settle(f);
    const spy = vi.spyOn(mock, 'alarmHistory');
    await tab(f, el, 'History');
    await tab(f, el, 'Active');
    await tab(f, el, 'History');
    expect(spy.mock.calls.filter((c) => c[0] === null)).toHaveLength(1);
  });
});

describe('dy-alarms-page, Rules', () => {
  it('shows time to acknowledge as hero numbers, the noisiest rules and the engine suggestions', async () => {
    const { f, el } = render();
    await settle(f);
    await tab(f, el, 'Rules');
    expect(text(el)).toContain('18 min');
    expect(text(el)).toContain('11 min');
    expect(text(el)).toContain('threshold:gpu-limit');
    expect(text(el)).toContain('GPU limit fired 14 times in a day.');
  });

  it('mutes and unmutes a noisy rule from its row', async () => {
    const { f, el } = render();
    await settle(f);
    await tab(f, el, 'Rules');
    button(el, 'Mute alert:lowDisk')!.click();
    await settle(f);
    expect(button(el, 'Unmute alert:lowDisk')).toBeTruthy();
    expect(text(el)).toContain('Muted');
    button(el, 'Unmute alert:lowDisk')!.click();
    await settle(f);
    expect(button(el, 'Mute alert:lowDisk')).toBeTruthy();
  });

  it('shows a statistics error with Retry', async () => {
    const { f, el } = render(['alarmStats']);
    await settle(f);
    await tab(f, el, 'Rules');
    expect(el.querySelector('dy-alarm-stats [role="alert"]')).toBeTruthy();
  });

  it('says so when no rule fired and nothing needs adjusting', async () => {
    const { f, el, mock } = render();
    await settle(f);
    vi.spyOn(mock, 'alarmStats').mockResolvedValue({ open: 0, unacknowledged: 0, chattering: [], noisiest: [], meanTimeToAcknowledgeMs: null, medianTimeToAcknowledgeMs: null, suggestions: [] });
    await tab(f, el, 'Rules');
    expect(text(el)).toContain('No rule has fired yet.');
    expect(text(el)).toContain('Nothing to adjust.');
    expect(text(el)).toContain('—');
  });

  it('lists the custom rules as a sentence and switches one off', async () => {
    const { f, el } = render();
    await settle(f);
    await tab(f, el, 'Rules');
    const row = rows(el).find((r) => text(r).includes('GPU limit'))!;
    expect(text(row)).toContain('GPU is above 90% for 2 min');
    expect(text(row)).toContain('Warning');
    (row.querySelector('button[role="switch"]') as HTMLButtonElement).click();
    await settle(f);
    expect(TestBed.inject(SettingsStore).settings()?.customAlerts?.[0]?.enabled).toBe(false);
  });

  it('shows a toast when a switch cannot be saved', async () => {
    const { f, el, mock } = render();
    await settle(f);
    await tab(f, el, 'Rules');
    vi.spyOn(mock, 'saveSettings').mockRejectedValueOnce({ code: 'badRequest', message: 'No way.' });
    (rows(el).find((r) => text(r).includes('GPU limit'))!.querySelector('button[role="switch"]') as HTMLButtonElement).click();
    await settle(f);
    expect(TestBed.inject(SettingsStore).settings()?.customAlerts?.[0]?.enabled).toBe(true);
  });

  it('opens the rule window for a new rule and for an edit, and reads the settings again once a rule was stored', async () => {
    const { f, el, mock } = render();
    await settle(f);
    await tab(f, el, 'Rules');
    const dialogs = TestBed.inject(DialogService);
    const opened = vi.spyOn(dialogs, 'open').mockReturnValue({ closed: Promise.resolve({ kind: 'dismissed' }), dismiss() {} } as never);
    button(el, /Add rule/)!.click();
    await settle(f);
    expect(opened.mock.calls[0]![0]).toBe('dy.alert-rule');
    expect(opened.mock.calls[0]![1]).toMatchObject({ inputs: { rule: null, takenIds: ['gpu-limit'] }, title: 'Add alert rule' });
    const load = vi.spyOn(mock, 'settings');
    opened.mockReturnValue({ closed: Promise.resolve({ kind: 'answered', value: true }), dismiss() {} } as never);
    button(el, 'Edit GPU limit')!.click();
    await settle(f);
    expect(opened.mock.calls[1]![1]).toMatchObject({ inputs: { rule: expect.objectContaining({ id: 'gpu-limit' }) }, title: 'Edit alert rule' });
    expect(load).toHaveBeenCalled();
  });

  it('deletes a rule only after the shared confirmation that names it', async () => {
    const { f, el } = render();
    await settle(f);
    await tab(f, el, 'Rules');
    const opened = vi.spyOn(TestBed.inject(DialogService), 'open').mockReturnValue({ closed: Promise.resolve({ kind: 'dismissed' }), dismiss() {} } as never);
    button(el, /^More actions for GPU limit/)!.click();
    await settle(f);
    menuItem('Delete…')!.click();
    await settle(f);
    expect(opened.mock.calls[0]![0]).toBe('pd.confirm');
    expect(opened.mock.calls[0]![1]).toMatchObject({ inputs: { heading: 'Delete “GPU limit”?', confirm: { label: 'Delete rule', destructive: true } } });
    expect(TestBed.inject(SettingsStore).settings()!.customAlerts).toHaveLength(1);
    opened.mockReturnValue({ closed: Promise.resolve({ kind: 'answered', value: true }), dismiss() {} } as never);
    button(el, /^More actions for GPU limit/)!.click();
    await settle(f);
    menuItem('Delete…')!.click();
    await settle(f);
    expect(TestBed.inject(SettingsStore).settings()!.customAlerts).toHaveLength(0);
    expect(text(el)).toContain('No custom rules');
  });

  it('turns a rule on or off from its menu', async () => {
    const { f, el } = render();
    await settle(f);
    await tab(f, el, 'Rules');
    rows(el).find((r) => text(r).includes('GPU limit'))!.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, clientX: 40, clientY: 40, button: 2 }));
    await settle(f);
    menuItem('Turn off')!.click();
    await settle(f);
    expect(TestBed.inject(SettingsStore).settings()!.customAlerts![0]!.enabled).toBe(false);
  });

  it('says so when deleting fails', async () => {
    const { f, el, mock } = render();
    await settle(f);
    await tab(f, el, 'Rules');
    vi.spyOn(TestBed.inject(DialogService), 'open').mockReturnValue({ closed: Promise.resolve({ kind: 'answered', value: true }), dismiss() {} } as never);
    vi.spyOn(mock, 'saveSettings').mockRejectedValueOnce({ code: 'internal', message: 'Disk full.' });
    button(el, /^More actions for GPU limit/)!.click();
    await settle(f);
    menuItem('Delete…')!.click();
    await settle(f);
    expect(TestBed.inject(SettingsStore).settings()!.customAlerts).toHaveLength(1);
  });

  it('binds the network switches to the settings', async () => {
    const { f, el } = render();
    await settle(f);
    await tab(f, el, 'Rules');
    const sw = (l: string) => el.querySelector<HTMLButtonElement>(`button[role="switch"][aria-label="${l}"]`)!;
    expect(sw('New exposed listener').getAttribute('aria-checked')).toBe('false');
    sw('New exposed listener').click();
    await settle(f);
    expect(TestBed.inject(SettingsStore).settings()!.networkAlerts).toEqual({ newExposedListener: true, newRemoteHost: false });
    sw('New remote host').click();
    await settle(f);
    expect(TestBed.inject(SettingsStore).settings()!.networkAlerts).toEqual({ newExposedListener: true, newRemoteHost: true });
  });

  it('edits notifications and quiet hours, converting HH:MM to minutes', async () => {
    const { f, el } = render();
    await settle(f);
    await tab(f, el, 'Rules');
    const sw = (l: string) => el.querySelector<HTMLButtonElement>(`button[role="switch"][aria-label="${l}"]`)!;
    const settings = TestBed.inject(SettingsStore);
    const before = settings.settings()!.notifications ?? false;
    sw('Desktop notifications').click();
    await settle(f);
    expect(settings.settings()!.notifications).toBe(!before);
    const start = el.querySelector<HTMLInputElement>('[aria-label="Quiet hours start"]')!;
    expect(start.disabled).toBe(true);
    sw('Quiet hours').click();
    await settle(f);
    expect(settings.settings()!.alarms!.quiet.enabled).toBe(true);
    start.value = '23:15';
    start.dispatchEvent(new Event('input'));
    await settle(f);
    const end = el.querySelector<HTMLInputElement>('[aria-label="Quiet hours end"]')!;
    end.value = '06:30';
    end.dispatchEvent(new Event('input'));
    await settle(f);
    expect(settings.settings()!.alarms!.quiet).toMatchObject({ startMinute: 23 * 60 + 15, endMinute: 6 * 60 + 30 });
    end.value = '';
    end.dispatchEvent(new Event('input'));
    await settle(f);
    expect(settings.settings()!.alarms!.quiet.endMinute).toBe(6 * 60 + 30);
    sw('Critical alarms still notify').click();
    await settle(f);
    expect(settings.settings()!.alarms!.quiet.criticalBypass).toBe(false);
  });

  it('falls back to the engine defaults when the settings carry no alarm configuration', async () => {
    const { f, el, mock } = render();
    const s = await mock.settings();
    delete s.alarms;
    delete s.networkAlerts;
    delete s.customAlerts;
    delete s.notifications;
    vi.spyOn(mock, 'settings').mockResolvedValue(s);
    await settle(f);
    await tab(f, el, 'Rules');
    expect(text(el)).toContain('No custom rules');
    expect(el.querySelector<HTMLInputElement>('[aria-label="Quiet hours start"]')!.value).toBe('22:00');
    expect(el.querySelector('button[role="switch"][aria-label="Desktop notifications"]')!.getAttribute('aria-checked')).toBe('false');
  });

  it('shows a settings error with Retry', async () => {
    const { f, el } = render(['settings']);
    await settle(f);
    await tab(f, el, 'Rules');
    expect(el.querySelector('dy-custom-rules [role="alert"]')).toBeTruthy();
    expect(el.querySelector('dy-alarm-settings [role="alert"]')).toBeTruthy();
    expect(button(el.querySelector('dy-custom-rules')!, /Add rule/)!.disabled).toBe(true);
  });
});
