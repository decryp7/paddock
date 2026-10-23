import { TestBed } from '@angular/core/testing';
import { provideRouter } from '@angular/router';
import { BACKEND } from '../../core/backend';
import { MockBackend } from '../../core/mock-backend';
import { Dialogs } from '../../shared/dialog';
import { provideIcons } from '../../shared/icons';
import { RulesStore } from '../../state/rules.store';
import { RuleDialog } from './rule-dialog';
import { RulesPage } from './rules-page';

function render(fail = new Set<string>()) {
  const mock = new MockBackend({ latencyMs: 0, now: () => 1_700_000_000_000, fail });
  TestBed.configureTestingModule({ providers: [provideIcons(), { provide: BACKEND, useValue: mock }, provideRouter([])] });
  const f = TestBed.createComponent(RulesPage);
  f.detectChanges();
  const dialogs = TestBed.inject(Dialogs);
  return { f, el: f.nativeElement as HTMLElement, mock, dialogs, store: TestBed.inject(RulesStore) };
}
const settle = async (f: { detectChanges(): void; whenStable(): Promise<unknown> }) => {
  for (let i = 0; i < 5; i++) {
    await f.whenStable();
    f.detectChanges();
  }
};
const byLabel = (el: HTMLElement, label: string) => el.querySelector<HTMLButtonElement>(`[aria-label="${label}"]`)!;

describe('dy-rules-page', () => {
  it('shows the heading, one primary button and the rules table', async () => {
    const { f, el, store } = render();
    await settle(f);
    expect(el.querySelector('h1')?.textContent).toBe('CPU taming');
    expect(el.querySelectorAll('.page-head .btn.primary').length).toBe(1);
    expect(el.querySelectorAll('table[aria-label="Rules"] tbody tr').length).toBe(store.ruleSet()!.rules.length);
    expect(el.textContent).toContain('Lower priority > Efficiency mode > Cap 50%');
    expect(el.textContent).toContain('All profiles');
    expect(el.querySelectorAll('table[aria-label="Rules"] thead').length).toBe(1);
  });

  it('switches profile and pauses all rules', async () => {
    const { f, el, store } = render();
    await settle(f);
    const gaming = [...el.querySelectorAll<HTMLButtonElement>('[aria-label="Profile"] button')].find((b) => b.textContent === 'Gaming')!;
    gaming.click();
    await settle(f);
    expect(store.profile()).toBe('gaming');
    byLabel(el, 'Pause all rules').click();
    await settle(f);
    expect(store.paused()).toBe(true);
    // choosing the profile already on does nothing
    gaming.click();
    await settle(f);
    expect(store.profile()).toBe('gaming');
  });

  it('asks what a rule will do the first time it is enabled, and enables only on confirm', async () => {
    const { f, el, store, dialogs } = render();
    await settle(f);
    const confirm = vi.spyOn(dialogs, 'confirm').mockResolvedValueOnce(false).mockResolvedValue(true);
    const sw = byLabel(el, 'Enable Indexing only when idle');
    sw.click();
    await settle(f);
    expect(confirm.mock.calls[0]![0].details!.at(-1)).toContain('never touches the foreground app');
    expect(confirm.mock.calls[0]![0].details![0]).toContain('Step 1');
    expect(store.ruleSet()!.rules.find((r) => r.id === 'indexer-idle')!.enabled).toBe(false);
    sw.click();
    await settle(f);
    expect(store.ruleSet()!.rules.find((r) => r.id === 'indexer-idle')!.enabled).toBe(true);
    // off needs no confirmation; on again is not asked a second time
    byLabel(el, 'Enable Indexing only when idle').click();
    await settle(f);
    byLabel(el, 'Enable Indexing only when idle').click();
    await settle(f);
    expect(confirm).toHaveBeenCalledTimes(2);
  });

  it('opens the dialog to add and to edit (Edit button, Enter)', async () => {
    const { f, el, dialogs } = render();
    await settle(f);
    const open = vi.spyOn(dialogs, 'open').mockReturnValue({} as never);
    el.querySelector<HTMLButtonElement>('.btn.primary')!.click();
    expect(open).toHaveBeenLastCalledWith(RuleDialog, { rule: null });
    byLabel(el, 'Edit Save battery').click();
    expect((open.mock.calls.at(-1)![1] as { rule: { id: string } }).rule.id).toBe('battery-saver');
    const row = el.querySelector<HTMLElement>('tbody tr')!;
    row.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    expect(open).toHaveBeenCalledTimes(3);
  });

  it('duplicates a rule as a disabled copy', async () => {
    const { f, el, store } = render();
    await settle(f);
    const n = store.ruleSet()!.rules.length;
    byLabel(el, 'Duplicate Save battery').click();
    await settle(f);
    const rules = store.ruleSet()!.rules;
    expect(rules.length).toBe(n + 1);
    expect(rules.at(-1)).toMatchObject({ name: 'Save battery copy', enabled: false });
  });

  it('deletes a rule only after a confirmation naming it', async () => {
    const { f, el, store, dialogs } = render();
    await settle(f);
    const confirm = vi.spyOn(dialogs, 'confirm').mockResolvedValueOnce(false).mockResolvedValue(true);
    const n = store.ruleSet()!.rules.length;
    const del = async () => {
      byLabel(el, 'More actions for Save battery').click();
      await settle(f);
      document.querySelector<HTMLButtonElement>('.menu .danger')!.click();
      await settle(f);
    };
    await del();
    expect(confirm.mock.calls[0]![0]).toMatchObject({ title: 'Delete “Save battery”?', danger: true });
    expect(store.ruleSet()!.rules.length).toBe(n);
    await del();
    expect(store.ruleSet()!.rules.length).toBe(n - 1);
  });

  it('moves between rows with the arrows and opens the menu with Shift+F10 or Menu', async () => {
    const { f, el } = render();
    await settle(f);
    const rows = [...el.querySelectorAll<HTMLElement>('tbody tr')];
    const press = (target: HTMLElement, key: string, shiftKey = false) => target.dispatchEvent(new KeyboardEvent('keydown', { key, shiftKey, bubbles: true, cancelable: true }));
    rows[0]!.focus();
    press(rows[0]!, 'ArrowDown');
    expect(document.activeElement).toBe(rows[1]);
    press(rows[1]!, 'ArrowUp');
    press(rows[0]!, 'ArrowUp');
    expect(document.activeElement).toBe(rows[0]);
    press(rows[0]!, 'F10');
    await settle(f);
    expect(document.querySelector('.menu')).toBeNull();
    press(rows[0]!, 'F10', true);
    await settle(f);
    expect(document.querySelector('.menu')?.textContent).toContain('Duplicate');
    // keys from inside a control are left alone
    press(rows[0]!.querySelector('button')!, 'ArrowDown');
    press(el.querySelector('thead th')!, 'ArrowDown');
    document.body.click();
  });

  it('opens the same menu with a right click', async () => {
    const { f, el } = render();
    await settle(f);
    el.querySelector('tbody tr')!.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: 10, clientY: 10 }));
    await settle(f);
    expect(document.querySelector('.menu')?.textContent).toContain('Edit');
    expect(document.querySelector('.menu')?.textContent).toContain('Delete');
  });

  it('shows an empty state without rules and an error with Retry when loading fails', async () => {
    const a = render();
    await settle(a.f);
    for (const r of [...a.store.ruleSet()!.rules]) await a.store.deleteRule(r.id);
    await settle(a.f);
    expect(a.el.textContent).toContain('No rules yet');
  });

  it('shows an error with Retry when the rules fail to load', async () => {
    const fail = new Set(['rules']);
    const { f, el } = render(fail);
    await settle(f);
    expect(el.querySelector('[role="alert"]')).not.toBeNull();
    fail.clear();
    el.querySelector<HTMLButtonElement>('[role="alert"] button')!.click();
    await settle(f);
    expect(el.querySelector('[role="alert"]')).toBeNull();
    expect(el.querySelectorAll('table[aria-label="Rules"] tbody tr').length).toBeGreaterThan(0);
  });

  describe('temporary rules', () => {
    const durButton = (el: HTMLElement, name: string) => byLabel(el, `How long ${name} stays on`);
    const choose = async (f: Parameters<typeof settle>[0], label: string) => {
      const item = [...document.querySelectorAll<HTMLButtonElement>('.menu button')].find((b) => b.textContent!.trim() === label)!;
      item.click();
      await settle(f);
    };

    it('says in one place that the app in front is never touched', async () => {
      const { f, el } = render();
      await settle(f);
      const notes = [...el.querySelectorAll('.fixed-note')];
      expect(notes).toHaveLength(1);
      expect(notes[0]!.textContent).toContain('Never touches the app in front; gives it back its CPU the moment you switch to it.');
    });

    it('shows how long a temporary rule has left, and "until turned off" for the others', async () => {
      const { f, el } = render();
      await settle(f);
      expect(durButton(el, 'Browsers in the background').textContent).toMatch(/Ending now|Ends in/);
      expect(durButton(el, 'Tame long CPU hogs').textContent).toContain('Until turned off');
      expect(durButton(el, 'Save battery').disabled).toBe(true);
      expect(durButton(el, 'Tame long CPU hogs').disabled).toBe(false);
    });

    it('sets an end time of an hour, four hours or tomorrow, and clears it again', async () => {
      const { f, el, store } = render();
      await settle(f);
      const rule = () => store.ruleSet()!.rules.find((r) => r.id === 'tame-long-hogs')!;
      const before = Date.now();
      durButton(el, 'Tame long CPU hogs').click();
      await settle(f);
      await choose(f, '1 hour');
      expect(rule().expiresAt! - before).toBeGreaterThanOrEqual(3_600_000);
      expect(rule().expiresAt! - before).toBeLessThan(3_600_000 + 5_000);
      expect(durButton(el, 'Tame long CPU hogs').textContent).toContain('Ends in 1 h');
      durButton(el, 'Tame long CPU hogs').click();
      await settle(f);
      await choose(f, '4 hours');
      expect(rule().expiresAt! - before).toBeGreaterThanOrEqual(4 * 3_600_000);
      durButton(el, 'Tame long CPU hogs').click();
      await settle(f);
      await choose(f, 'Until tomorrow');
      expect(new Date(rule().expiresAt!).getHours()).toBe(6);
      durButton(el, 'Tame long CPU hogs').click();
      await settle(f);
      await choose(f, 'Until I turn it off');
      expect(rule().expiresAt).toBeNull();
    });

    it('asks for a time of day for a custom end, and a cancelled question changes nothing', async () => {
      const { f, el, store } = render();
      await settle(f);
      durButton(el, 'Tame long CPU hogs').click();
      await settle(f);
      await choose(f, 'Until a time…');
      const dialog = document.querySelector('.dy-dialog-panel')!;
      expect(dialog.textContent).toContain('The rule ends');
      [...dialog.querySelectorAll<HTMLButtonElement>('button')].find((b) => b.textContent === 'Cancel')!.click();
      await settle(f);
      expect(store.ruleSet()!.rules.find((r) => r.id === 'tame-long-hogs')!.expiresAt).toBeNull();
      durButton(el, 'Tame long CPU hogs').click();
      await settle(f);
      await choose(f, 'Until a time…');
      const input = document.querySelector<HTMLInputElement>('#tmp-time')!;
      input.value = '23:15';
      input.dispatchEvent(new Event('input'));
      await settle(f);
      const set = [...document.querySelectorAll<HTMLButtonElement>('.dy-dialog-panel button')].find((b) => b.textContent === 'Set end time')!;
      set.click();
      await settle(f);
      expect(new Date(store.ruleSet()!.rules.find((r) => r.id === 'tame-long-hogs')!.expiresAt!).getMinutes()).toBe(15);
    });

    it('turning a rule off clears its end time', async () => {
      const { f, el, store } = render();
      await settle(f);
      const rule = () => store.ruleSet()!.rules.find((r) => r.id === 'browser-background')!;
      expect(rule().expiresAt).not.toBeNull();
      el.querySelector<HTMLButtonElement>('tr[data-id="browser-background"] button[role="switch"]')!.click();
      await settle(f);
      expect(rule()).toMatchObject({ enabled: false, expiresAt: null });
      expect(durButton(el, 'Browsers in the background').disabled).toBe(true);
    });
  });
});
