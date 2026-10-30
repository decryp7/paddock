import { ChangeDetectionStrategy, Component, createEnvironmentInjector, DestroyRef, EnvironmentInjector, Injector, provideZonelessChangeDetection, runInInjectionContext } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { defineSettingsPage, msg } from '@decryp7/paddock-ui/shell';
import { DIALOG_REF } from '@decryp7/paddock-ui/shell/dialogs';
import { MemorySettingsBackend } from '@decryp7/paddock-ui/shell/settings';
import { SettingsCloseGuards, settingsPageCloseGuard, type SettingsCloseVerdict } from './settings-close';
import { PdSettingsWindow } from './settings-window';
import { settingsBed, settle, SPEC_PAGES } from './spec-helpers';

/** A DestroyRef the test ends by hand. */
function lifetime(): { ref: DestroyRef; end: () => void } {
  const callbacks: (() => void)[] = [];
  const ref = { onDestroy: (fn: () => void) => (callbacks.push(fn), () => undefined), destroyed: false } as unknown as DestroyRef;
  return { ref, end: () => callbacks.splice(0).forEach((fn) => fn()) };
}

describe('SettingsCloseGuards', () => {
  beforeEach(() => TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection(), SettingsCloseGuards] }));

  it('closes with no guard, prefers a wait to a warning, and forgets a guard when its page leaves', () => {
    const guards = TestBed.inject(SettingsCloseGuards);
    expect(guards.check().kind).toBe('close');
    const page = lifetime();
    guards.add(() => ({ kind: 'warn', message: 'Port was not saved: in use.' }), page.ref);
    expect(guards.check()).toEqual({ kind: 'warn', message: 'Port was not saved: in use.' });
    const done = Promise.resolve();
    const stop = guards.add(() => ({ kind: 'wait', done }), lifetime().ref);
    expect(guards.check()).toEqual({ kind: 'wait', done });
    stop();
    stop();
    page.end();
    expect(guards.check().kind).toBe('close');
  });

  it('a guard that throws never keeps the window open', () => {
    const guards = TestBed.inject(SettingsCloseGuards);
    const error = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    guards.add(() => {
      throw new Error('bug');
    }, lifetime().ref);
    expect(guards.check().kind).toBe('close');
    expect(error).toHaveBeenCalled();
  });

  it('settingsPageCloseGuard adds through the injection context and does nothing outside a Settings window', () => {
    const guards = TestBed.inject(SettingsCloseGuards);
    const page = createEnvironmentInjector([], TestBed.inject(EnvironmentInjector));
    runInInjectionContext(page, () => settingsPageCloseGuard(() => ({ kind: 'warn', message: 'x' })));
    expect(guards.check().kind).toBe('warn');
    page.destroy();
    expect(guards.check().kind).toBe('close');
    const clear = runInInjectionContext(Injector.create({ providers: [] }), () => settingsPageCloseGuard(() => ({ kind: 'close' })));
    expect(() => clear()).not.toThrow();
  });
});

/** A page that saves typed values after a pause, as an app writes it. */
@Component({ selector: 'spec-typed-page', changeDetection: ChangeDetectionStrategy.OnPush, template: `<p>Typed</p>` })
class TypedPage {
  static verdict: () => SettingsCloseVerdict = () => ({ kind: 'close' });
  readonly stop = settingsPageCloseGuard(() => TypedPage.verdict());
}

describe('pd-settings-window close with a page’s guard', () => {
  const PAGES = [...SPEC_PAGES, defineSettingsPage({ id: 'sd.typed', group: 'features', title: msg('sd.settings.typed', 'Typed'), order: 200, load: () => Promise.resolve(TypedPage) })];

  async function open() {
    const fixture = { ref: null as unknown as ReturnType<typeof TestBed.createComponent<PdSettingsWindow>> };
    const dismiss = vi.fn(() => {
      if (fixture.ref.componentInstance.dialogCanClose()) closed();
    });
    const closed = vi.fn();
    settingsBed({ backend: new MemorySettingsBackend(), pages: PAGES, extra: [{ provide: DIALOG_REF, useValue: { dismiss, answer: vi.fn(), closed: new Promise(() => undefined) } }] });
    fixture.ref = TestBed.createComponent(PdSettingsWindow);
    fixture.ref.componentRef.setInput('page', 'sd.typed');
    await settle();
    await fixture.ref.whenStable();
    await settle();
    return { fixture: fixture.ref, window: fixture.ref.componentInstance, closed, el: fixture.ref.nativeElement as HTMLElement };
  }

  afterEach(() => (TypedPage.verdict = () => ({ kind: 'close' })));

  it('waits for a save still on its way, then closes once it is done', async () => {
    let finish!: () => void;
    const done = new Promise<void>((r) => (finish = r));
    TypedPage.verdict = () => ({ kind: 'wait', done });
    const { window, closed } = await open();
    expect(window.dialogCanClose()).toBe(false);
    // Asked again while it waits (a second Cmd+W): still waiting, one wait only.
    expect(window.dialogCanClose()).toBe(false);
    TypedPage.verdict = () => ({ kind: 'close' });
    finish();
    await settle();
    expect(closed).toHaveBeenCalledTimes(1);
  });

  it('a wait that fails still asks again', async () => {
    TypedPage.verdict = () => ({ kind: 'wait', done: Promise.reject(new Error('lost')) });
    const { window, closed } = await open();
    expect(window.dialogCanClose()).toBe(false);
    TypedPage.verdict = () => ({ kind: 'close' });
    await settle();
    expect(closed).toHaveBeenCalledTimes(1);
  });

  it('says what was not saved and why, and closes only on "Close anyway"; "Keep editing" stays', async () => {
    TypedPage.verdict = () => ({ kind: 'warn', message: 'Port was not saved: the port is in use.' });
    const { fixture, window, closed, el } = await open();
    expect(window.dialogCanClose()).toBe(false);
    fixture.detectChanges();
    const banner = el.querySelector('[data-testid="settings-not-saved"]')!;
    expect(banner.textContent).toContain('Port was not saved: the port is in use.');
    const [closeAnyway, keep] = Array.from(banner.querySelectorAll('button'));
    expect(closeAnyway!.textContent).toContain('Close anyway');
    keep!.click();
    fixture.detectChanges();
    expect(el.querySelector('[data-testid="settings-not-saved"]')).toBeNull();
    expect(closed).not.toHaveBeenCalled();
    expect(window.dialogCanClose()).toBe(false);
    fixture.detectChanges();
    (el.querySelector('[data-testid="settings-not-saved"] button') as HTMLButtonElement).click();
    expect(closed).toHaveBeenCalledTimes(1);
  });
});
