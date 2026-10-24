import { provideZonelessChangeDetection, type Type } from '@angular/core';
import { TestBed, type ComponentFixture } from '@angular/core/testing';
import { DIALOG_REF, type DialogRef } from '@decryp7/paddock-ui/shell/dialogs';
import { assertDialogBoots } from '@decryp7/paddock-ui/shell/testing';
import { DOCK_OPTIONS } from './dock-options';
import { DOCK_LAYOUT_STORE, InMemoryDockLayoutStore } from './dock-store';
import { dockLayoutDialogs, LAYOUT_MANAGE_DIALOG_ID, LAYOUT_SAVE_DIALOG_ID } from './layout-dialogs';
import { PdLayoutManageDialog, type LayoutManageResult } from './layout-manage-dialog';
import { PdLayoutSaveDialog } from './layout-save-dialog';
import { EMPTY_EXTRAS, type SavedLayout } from './layout-store';
import { MAX_LAYOUT_NAME, parseNamedLayouts, serializeNamedLayouts, type NamedLayout } from './named-layouts';

const grid = { grid: { root: { type: 'leaf', data: { views: [], id: '1' } }, height: 1, width: 1, orientation: 'HORIZONTAL' }, panels: {} } as never;
const saved = (): SavedLayout => ({ layout: grid, extras: { ...EMPTY_EXTRAS } });
const nl = (name: string): NamedLayout => ({ name, savedAt: 1, saved: saved() });

const [SAVE, MANAGE] = dockLayoutDialogs({ store: new InMemoryDockLayoutStore() });

class FakeRef implements DialogRef<never> {
  readonly closes: unknown[] = [];
  readonly closed = Promise.resolve(undefined);
  close(result?: unknown): void {
    this.closes.push(result);
  }
}

async function open<T>(component: Type<T>, store: InMemoryDockLayoutStore, inputs: Record<string, unknown> = {}): Promise<{ fixture: ComponentFixture<T>; el: HTMLElement; ref: FakeRef; q: <E extends HTMLElement>(testid: string) => E | null }> {
  const ref = new FakeRef();
  TestBed.resetTestingModule();
  TestBed.configureTestingModule({
    providers: [provideZonelessChangeDetection(), { provide: DIALOG_REF, useValue: ref }, { provide: DOCK_LAYOUT_STORE, useValue: store }, { provide: DOCK_OPTIONS, useValue: {} }],
  });
  const fixture = TestBed.createComponent(component);
  for (const [k, v] of Object.entries(inputs)) fixture.componentRef.setInput(k, v);
  document.body.appendChild(fixture.nativeElement);
  await fixture.whenStable();
  const el = fixture.nativeElement as HTMLElement;
  return { fixture, el, ref, q: (id) => el.querySelector(`[data-testid="${id}"]`) };
}

const type = async (fixture: ComponentFixture<unknown>, input: HTMLInputElement, text: string) => {
  input.value = text;
  input.dispatchEvent(new Event('input'));
  await fixture.whenStable();
};

describe('the layout dialogs are declared as ADR 0009 asks', () => {
  it('declares save as a modal form and manage as a modeless tool, each with its own contract', () => {
    expect([SAVE.id, SAVE.kind, SAVE.modality, SAVE.commands]).toEqual([LAYOUT_SAVE_DIALOG_ID, 'form', 'modal', []]);
    expect([MANAGE.id, MANAGE.kind, MANAGE.modality, MANAGE.commands]).toEqual([LAYOUT_MANAGE_DIALOG_ID, 'tool', 'modeless', ['layout_get', 'layout_set']]);
    expect(LAYOUT_SAVE_DIALOG_ID).toBe('pd.wb.dock.layout-save');
    expect(LAYOUT_MANAGE_DIALOG_ID).toBe('pd.wb.dock.layout-manage');
    expect(MANAGE.minWidth).toBeLessThanOrEqual(MANAGE.width ?? 0);
  });

  it('boots in the shell harness: providers, a body, a visible Cancel/Close, Escape, only declared commands', async () => {
    const options = { dialogs: [SAVE, MANAGE], inputs: { [LAYOUT_SAVE_DIALOG_ID]: { names: ['Work'], suggested: 'Layout 2' } } };
    await assertDialogBoots(SAVE, options);
    await assertDialogBoots(MANAGE, options);
  });

  it('gives a window that cannot reach the app a store of its own in memory', async () => {
    const [, plain] = dockLayoutDialogs();
    const providers = (await plain.providers?.()) ?? [];
    expect(providers.length).toBe(2);
  });
});

describe('Save layout', () => {
  it('starts with the suggested name selected and saves the cleaned name on Save', async () => {
    const { fixture, q, ref } = await open(PdLayoutSaveDialog, new InMemoryDockLayoutStore(), { names: ['Work'], suggested: 'Layout 2' });
    const input = q<HTMLInputElement>('layout-save-name')!;
    expect(input.value).toBe('Layout 2');
    await type(fixture, input, '  Debugging  ');
    q<HTMLButtonElement>('layout-save-ok')!.click();
    expect(ref.closes).toEqual(['Debugging']);
  });

  it('refuses an empty name, a duplicate (any case) and a name that is too long, saying why and keeping the dialog open', async () => {
    const { fixture, el, q, ref } = await open(PdLayoutSaveDialog, new InMemoryDockLayoutStore(), { names: ['Work'], suggested: 'x' });
    const input = q<HTMLInputElement>('layout-save-name')!;
    const save = q<HTMLButtonElement>('layout-save-ok')!;
    const error = () => el.querySelector('[data-testid="field-error"]')?.textContent?.trim() ?? null;
    expect(error()).toBeNull();
    await type(fixture, input, '   ');
    save.click();
    await fixture.whenStable();
    expect(error()).toBe('Give the layout a name.');
    expect(input.getAttribute('aria-invalid')).toBe('true');
    await type(fixture, input, 'WORK');
    expect(error()).toBe('A layout with this name exists. Pick another name.');
    await type(fixture, input, 'x'.repeat(MAX_LAYOUT_NAME + 1));
    expect(error()).toBe(`Use at most ${MAX_LAYOUT_NAME} characters.`);
    save.click();
    expect(ref.closes).toEqual([]);
    await type(fixture, input, 'Fine');
    expect(error()).toBeNull();
    save.click();
    expect(ref.closes).toEqual(['Fine']);
  });

  it('cancels without a result, and Cancel is the dialog close', async () => {
    const { q, ref } = await open(PdLayoutSaveDialog, new InMemoryDockLayoutStore());
    const cancel = q<HTMLButtonElement>('layout-save-cancel')!;
    expect(cancel.hasAttribute('data-dialog-close')).toBe(true);
    expect(q('layout-save-ok')!.hasAttribute('data-default')).toBe(true);
    cancel.click();
    expect(ref.closes).toEqual([undefined]);
  });
});

describe('Manage layouts', () => {
  const stored = async (store: InMemoryDockLayoutStore) => parseNamedLayouts(await store.load('wb.layouts')).map((l) => l.name);
  const seeded = async (names: string[]) => {
    const store = new InMemoryDockLayoutStore();
    await store.save('wb.layouts', serializeNamedLayouts(names.map(nl)));
    return store;
  };
  const rows = (el: HTMLElement) => [...el.querySelectorAll<HTMLElement>('[data-layout]')].map((r) => r.dataset['layout']);

  it('lists the saved layouts and says so when there are none', async () => {
    const some = await open(PdLayoutManageDialog, await seeded(['Work', 'Play']));
    expect(rows(some.el)).toEqual(['Work', 'Play']);
    expect(some.q('layout-manage-empty')).toBeNull();
    const none = await open(PdLayoutManageDialog, new InMemoryDockLayoutStore());
    expect(rows(none.el)).toEqual([]);
    expect(none.q('layout-manage-empty')!.textContent).toContain('No saved layouts');
  });

  it('applies a layout by closing with its name', async () => {
    const { el, ref } = await open(PdLayoutManageDialog, await seeded(['Work', 'Play']));
    el.querySelectorAll<HTMLButtonElement>('[data-testid="layout-manage-apply"]')[1]!.click();
    expect(ref.closes).toEqual([{ apply: 'Play' } satisfies LayoutManageResult]);
  });

  it('renames in place, writing the store, and refuses a duplicate or empty name', async () => {
    const store = await seeded(['Work', 'Play']);
    const { fixture, el, q } = await open(PdLayoutManageDialog, store);
    el.querySelectorAll<HTMLButtonElement>('[data-testid="layout-manage-rename-start"]')[0]!.click();
    await fixture.whenStable();
    const input = q<HTMLInputElement>('layout-manage-rename')!;
    expect(input.value).toBe('Work');
    await type(fixture, input, 'play');
    q<HTMLButtonElement>('layout-manage-rename-ok')!.click();
    await fixture.whenStable();
    expect(el.querySelector('[data-testid="field-error"]')?.textContent).toContain('exists');
    expect(await stored(store)).toEqual(['Work', 'Play']);
    await type(fixture, input, ' ');
    q<HTMLButtonElement>('layout-manage-rename-ok')!.click();
    await fixture.whenStable();
    expect(el.querySelector('[data-testid="field-error"]')?.textContent).toContain('Give the layout a name');
    await type(fixture, input, 'Office');
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    await fixture.whenStable();
    expect(rows(el)).toEqual(['Office', 'Play']);
    expect(await stored(store)).toEqual(['Office', 'Play']);
    expect(q('layout-manage-rename')).toBeNull();
  });

  it('cancels a rename with Escape without closing the dialog (the key stops at the field) and keeps the name when unchanged', async () => {
    const store = await seeded(['Work']);
    const { fixture, el, q } = await open(PdLayoutManageDialog, store);
    const escaped: Event[] = [];
    document.addEventListener('keydown', (e) => escaped.push(e), { once: true });
    q<HTMLButtonElement>('layout-manage-rename-start')!.click();
    await fixture.whenStable();
    q<HTMLInputElement>('layout-manage-rename')!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    await fixture.whenStable();
    expect(escaped).toEqual([]);
    expect(q('layout-manage-rename')).toBeNull();
    q<HTMLButtonElement>('layout-manage-rename-start')!.click();
    await fixture.whenStable();
    q<HTMLButtonElement>('layout-manage-rename-ok')!.click();
    await fixture.whenStable();
    expect(rows(el)).toEqual(['Work']);
    q<HTMLButtonElement>('layout-manage-rename-start')!.click();
    await fixture.whenStable();
    q<HTMLButtonElement>('layout-manage-rename-cancel')!.click();
    await fixture.whenStable();
    expect(q('layout-manage-rename')).toBeNull();
  });

  it('deletes without a confirmation, offers Undo in a status strip, and puts the layout back where it was', async () => {
    const store = await seeded(['A', 'B', 'C']);
    const { fixture, el, q } = await open(PdLayoutManageDialog, store);
    el.querySelectorAll<HTMLButtonElement>('[data-testid="layout-manage-delete"]')[1]!.click();
    await fixture.whenStable();
    expect(rows(el)).toEqual(['A', 'C']);
    expect(await stored(store)).toEqual(['A', 'C']);
    const strip = q('layout-manage-undo')!;
    expect(strip.getAttribute('role')).toBe('status');
    expect(strip.textContent).toContain('Deleted layout B');
    q<HTMLButtonElement>('layout-manage-undo-button')!.click();
    await fixture.whenStable();
    expect(rows(el)).toEqual(['A', 'B', 'C']);
    expect(await stored(store)).toEqual(['A', 'B', 'C']);
    expect(q('layout-manage-undo')).toBeNull();
  });

  it('drops the Undo when another change is made, and does not restore a name that was taken meanwhile', async () => {
    const store = await seeded(['A', 'B']);
    const { fixture, el, q } = await open(PdLayoutManageDialog, store);
    el.querySelectorAll<HTMLButtonElement>('[data-testid="layout-manage-delete"]')[0]!.click();
    await fixture.whenStable();
    expect(q('layout-manage-undo')).not.toBeNull();
    q<HTMLButtonElement>('layout-manage-rename-start')!.click();
    await fixture.whenStable();
    expect(q('layout-manage-undo')).toBeNull();
    q<HTMLButtonElement>('layout-manage-rename-cancel')!.click();
    await fixture.whenStable();
    // Delete the last one: the strip names it; an Undo for a name that exists again is a no-op.
    el.querySelectorAll<HTMLButtonElement>('[data-testid="layout-manage-delete"]')[0]!.click();
    await fixture.whenStable();
    expect(rows(el)).toEqual([]);
    expect(q('layout-manage-empty')).not.toBeNull();
    q<HTMLButtonElement>('layout-manage-undo-button')!.click();
    await fixture.whenStable();
    expect(rows(el)).toEqual(['B']);
  });

  it('closes with the Close button, whose result is nothing', async () => {
    const { q, ref } = await open(PdLayoutManageDialog, await seeded(['A']));
    q<HTMLButtonElement>('layout-manage-close')!.click();
    expect(ref.closes).toEqual([undefined]);
  });
});
