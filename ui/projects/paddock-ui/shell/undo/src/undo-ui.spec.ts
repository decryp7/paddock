import { TestBed } from '@angular/core/testing';
import { ToastArea, type ShellContext } from '@decryp7/paddock-ui/shell';
import { MATERIAL_ANIMATIONS } from '@angular/material/core';
import { MatSnackBar } from '@angular/material/snack-bar';
import { providePaddockMaterial } from '@decryp7/paddock-ui/material';
import { describe, expect, it, vi } from 'vitest';
import { UNDO_TOAST_MS } from './undo-config';

// Read from disk (vitest runs in ui/), as template-classes.spec does.
const nodeFs = (): Promise<{ readFileSync(path: string, encoding: 'utf8'): string }> => import(/* @vite-ignore */ ['node', 'fs'].join(':')) as never;
import { provideShellUndo } from './provide-undo';
import { bed, settle } from './spec-helpers';
import { PdUndoToast, UNDO_SNACK_CLASS } from './undo-toast';
import { REDO_COMMAND, stepHistory, UNDO_COMMAND, UNDO_CONTRIBUTIONS, type UndoContext } from './undo.contributions';
import { UndoStore } from './undo.store';

describe('pd-undo-toast (a Material snack bar)', () => {
  function mount() {
    const b = bed([provideShellUndo(), { provide: MATERIAL_ANIMATIONS, useValue: { animationsDisabled: true } }, providePaddockMaterial()]);
    const store = TestBed.inject(UndoStore);
    const f = TestBed.createComponent(PdUndoToast);
    // The snack bar is drawn in the overlay, not inside the component.
    const toast = () => document.querySelector<HTMLElement>('[data-testid=undo-toast]');
    const buttons = () => [...(toast()?.querySelectorAll('button') ?? [])];
    return { ...b, store, f, toast, buttons, container: () => document.querySelector<HTMLElement>('.mat-mdc-snack-bar-container') };
  }
  /** The snack bar leaves the page after its exit step (a microtask or a timer, never a fixed wait): wait for the condition. */
  const gone = (t: { toast: () => HTMLElement | null }) => vi.waitFor(() => expect(t.toast()).toBeNull());
  const announce = (store: InstanceType<typeof UndoStore>, label: string) => store.record({ label, announce: true, do: () => undefined, undo: () => undefined });

  it('shows what was done with an Undo button that takes it back, and a close button', async () => {
    const t = mount();
    let value = 1;
    await settle();
    expect(t.toast()).toBeNull();
    await t.store.execute({ label: 'rename to b', announce: true, do: () => void (value = 2), undo: () => void (value = 1) });
    await settle();
    expect(t.toast()?.textContent).toContain('Done: rename to b');
    expect(t.buttons().map((b) => b.textContent?.trim() || b.getAttribute('aria-label'))).toEqual(['Undo', 'Dismiss']);
    t.buttons()[0]!.click();
    await settle();
    expect(value).toBe(1);
    await gone(t);
    announce(t.store, 'again');
    await settle();
    t.toast()!.querySelector<HTMLButtonElement>('button[aria-label="Dismiss"]')!.click();
    await settle();
    await gone(t);
    expect(t.store.canUndo()).toBe(true);
  });

  it('is announced politely, takes no focus and has no timer of its own', async () => {
    vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] });
    try {
      const t = mount();
      announce(t.store, 'x');
      await settle();
      expect(t.container()?.querySelector('[aria-live="polite"]')).not.toBeNull();
      // The snack bar moves the content into its live region after a short delay, which is what screen readers read.
      await vi.advanceTimersByTimeAsync(500);
      expect(t.container()?.querySelector('[aria-live="polite"] [data-testid=undo-toast]')).not.toBeNull();
      expect(t.toast()?.contains(document.activeElement)).toBe(false);
      // The store's clock (UNDO_TOAST_MS) is the only one: the snack bar's own would end the toast far earlier (its default is
      // none, and a configured one would show here).
      await vi.advanceTimersByTimeAsync(UNDO_TOAST_MS - 1);
      expect(t.toast()).not.toBeNull();
      t.clock.advance(UNDO_TOAST_MS);
      await settle();
      await vi.advanceTimersByTimeAsync(500);
      expect(t.toast()).toBeNull();
    } finally {
      vi.useRealTimers();
    }
  });

  it('is inert (not focusable) while the snack bar holds it in its aria-hidden announce step, and live afterwards', async () => {
    vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] });
    try {
      const t = mount();
      announce(t.store, 'x');
      await settle();
      // Before the announce delay the content sits in an aria-hidden wrapper: nothing in it may be focusable (axe aria-hidden-focus).
      expect(t.toast()?.closest('[aria-hidden="true"]')).not.toBeNull();
      expect(t.toast()?.hasAttribute('inert')).toBe(true);
      await vi.advanceTimersByTimeAsync(500);
      expect(t.toast()?.closest('[aria-hidden="true"]')).toBeNull();
      await vi.waitFor(() => expect(t.toast()?.hasAttribute('inert')).toBe(false));
    } finally {
      vi.useRealTimers();
    }
  });

  it('goes by itself when the store hides it, and a newer toast replaces the older one', async () => {
    const t = mount();
    announce(t.store, 'one');
    await settle();
    announce(t.store, 'two');
    await settle();
    await vi.waitFor(() => expect(document.querySelectorAll('[data-testid=undo-toast]')).toHaveLength(1));
    expect(t.toast()?.textContent).toContain('Done: two');
    t.clock.advance(UNDO_TOAST_MS);
    await settle();
    await gone(t);
  });

  it('drops the store’s toast when another snack bar replaces it (MatSnackBar shows one at a time)', async () => {
    const t = mount();
    announce(t.store, 'x');
    await settle();
    expect(t.store.toast()).not.toBeNull();
    TestBed.inject(MatSnackBar).open('Something else');
    await vi.waitFor(() => expect(t.store.toast()).toBeNull());
    expect(t.store.canUndo()).toBe(true);
  });

  it('reports its height to the toast area, so the notification host stacks above it, and clears it when gone', async () => {
    const observed: (() => void)[] = [];
    vi.stubGlobal('ResizeObserver', class { constructor(cb: () => void) { observed.push(cb); } observe(): void {} disconnect(): void {} });
    const height = vi.spyOn(HTMLElement.prototype, 'offsetHeight', 'get').mockReturnValue(48);
    try {
      const t = mount();
      const area = TestBed.inject(ToastArea);
      expect(area.lift()).toBe(0);
      announce(t.store, 'x');
      await settle();
      await vi.waitFor(() => expect(area.lift()).toBe(48));
      height.mockReturnValue(72);
      observed.forEach((cb) => cb());
      expect(area.lift()).toBe(72);
      t.store.dismissToast();
      await settle();
      await vi.waitFor(() => expect(area.lift()).toBe(0));
    } finally {
      height.mockRestore();
      vi.unstubAllGlobals();
    }
  });

  it('closes with its component', async () => {
    const t = mount();
    announce(t.store, 'x');
    await settle();
    expect(t.toast()).not.toBeNull();
    t.f.destroy();
    await gone(t);
  });

  it('draws its close icon at 14 px or more', async () => {
    const t = mount();
    announce(t.store, 'x');
    await settle();
    expect(Number(t.toast()!.querySelector('pd-glyph svg')!.getAttribute('width'))).toBeGreaterThanOrEqual(14);
  });

  it('sits on the notification host’s baseline (bottom right, 360 px), above the status bar and the page footer, from tokens', async () => {
    const MATERIAL_SCSS = (await nodeFs()).readFileSync('projects/paddock-ui/scss/_material.scss', 'utf8');
    // jsdom has no layout: the rule is read from the stylesheet source; e2e/shell-demo-undo.spec.ts measures it in a browser.
    const rule = /\.mat-mdc-snack-bar-container\.pd-undo-snack \{([^}]*)\}/.exec(MATERIAL_SCSS)?.[1]?.replace(/\s+/g, ' ') ?? '';
    expect(rule).toMatch(/margin: 0 var\(--pd-space-4\) calc\(var\(--pd-statusbar-height[^)]*\) \+ var\(--pd-page-footer-safe[^)]*\) \+ var\(--pd-space-2\)\)/);
    expect(MATERIAL_SCSS).toMatch(/pd-undo-snack \.mat-mdc-snackbar-surface \{[^}]*width: 360px/);
    expect(UNDO_SNACK_CLASS).toBe('pd-undo-snack');
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
