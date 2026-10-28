import { TestBed } from '@angular/core/testing';
import type { ShellContext } from '@decryp7/paddock-ui/shell';
import { describe, expect, it } from 'vitest';
import { provideShellUndo } from './provide-undo';
import { bed, settle } from './spec-helpers';
import { PdUndoToast } from './undo-toast';
import { REDO_COMMAND, stepHistory, UNDO_COMMAND, UNDO_CONTRIBUTIONS, type UndoContext } from './undo.contributions';
import { UndoStore } from './undo.store';

describe('pd-undo-toast', () => {
  function mount() {
    const b = bed([provideShellUndo()]);
    const store = TestBed.inject(UndoStore);
    const f = TestBed.createComponent(PdUndoToast);
    const el: HTMLElement = f.nativeElement;
    return { ...b, store, f, el, toast: () => el.querySelector<HTMLElement>('[data-testid=undo-toast]') };
  }

  it('shows what was done with an Undo button that takes it back, and a close button', async () => {
    const t = mount();
    let value = 1;
    await settle();
    expect(t.toast()).toBeNull();
    await t.store.execute({ label: 'rename to b', announce: true, do: () => void (value = 2), undo: () => void (value = 1) });
    await settle();
    expect(t.toast()?.getAttribute('role')).toBe('status');
    expect(t.toast()?.textContent).toContain('Done: rename to b');
    t.toast()!.querySelector<HTMLButtonElement>('button.pd-btn:not(.pd-btn--icon)')!.click();
    await settle();
    expect(value).toBe(1);
    expect(t.toast()).toBeNull();
    t.store.record({ label: 'again', announce: true, do: () => undefined, undo: () => undefined });
    await settle();
    const close = t.toast()!.querySelector<HTMLButtonElement>('button[aria-label="Dismiss"]')!;
    close.click();
    await settle();
    expect(t.toast()).toBeNull();
    expect(t.store.canUndo()).toBe(true);
  });

  it('draws its close icon at 14 px or more', async () => {
    const t = mount();
    t.store.record({ label: 'x', announce: true, do: () => undefined, undo: () => undefined });
    await settle();
    expect(parseFloat((t.el.querySelector('pd-glyph mat-icon') as HTMLElement).style.width)).toBeGreaterThanOrEqual(14);
  });
});

describe('Edit > Undo and Redo contributions', () => {
  function ctx() {
    const b = bed([provideShellUndo()]);
    const store = TestBed.inject(UndoStore);
    const notices: string[] = [];
    const context = { undo: store, notify: (n: { message: string }) => void notices.push(n.message) } as unknown as ShellContext<UndoContext>;
    return { ...b, store, notices, context };
  }

  it('registers the commands, the keys and the Edit menu items', () => {
    const b = UNDO_CONTRIBUTIONS;
    expect(b.commands?.map((c) => c.id)).toEqual([UNDO_COMMAND, REDO_COMMAND]);
    expect(b.keybindings?.map((k) => [k.command, k.key])).toEqual([
      [UNDO_COMMAND, 'Mod+Z'],
      [REDO_COMMAND, 'Mod+Shift+Z'],
    ]);
    expect(b.menus?.map((m) => [m.menu, m.command])).toEqual([
      ['edit', UNDO_COMMAND],
      ['edit', REDO_COMMAND],
    ]);
    expect(b.notificationKinds?.map((k) => k.id)).toEqual(['pd.undo.failed']);
  });

  it('are enabled only when there is something to undo or redo', async () => {
    const c = ctx();
    const [undo, redo] = UNDO_CONTRIBUTIONS.commands as readonly { enabled: (x: ShellContext<UndoContext>) => boolean }[];
    expect([undo!.enabled(c.context), redo!.enabled(c.context)]).toEqual([false, false]);
    c.store.record({ label: 'a', do: () => undefined, undo: () => undefined });
    expect([undo!.enabled(c.context), redo!.enabled(c.context)]).toEqual([true, false]);
    await c.store.undo();
    expect([undo!.enabled(c.context), redo!.enabled(c.context)]).toEqual([false, true]);
  });

  it('say what happened, or that there was nothing to do', async () => {
    const c = ctx();
    await stepHistory(c.context, 'undo');
    await stepHistory(c.context, 'redo');
    c.store.record({ label: 'filter change', do: () => undefined, undo: () => undefined });
    await stepHistory(c.context, 'undo');
    await stepHistory(c.context, 'redo');
    expect(c.notices).toEqual(['Nothing to undo', 'Nothing to redo', 'Undone: filter change', 'Redone: filter change']);
    // A failing undo says so through the store and adds no "Undone".
    c.store.record({ label: 'bad', do: () => undefined, undo: () => Promise.reject(new Error('no')) });
    await stepHistory(c.context, 'undo');
    expect(c.notices).toHaveLength(4);
    expect(c.notices).not.toContain('Undone: bad');
    const run = (UNDO_CONTRIBUTIONS.commands![0] as { run: (x: ShellContext<UndoContext>) => Promise<void> }).run;
    await run(c.context);
  });
});
