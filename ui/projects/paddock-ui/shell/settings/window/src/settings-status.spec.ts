import { ChangeDetectionStrategy, Component, createEnvironmentInjector, DestroyRef, EnvironmentInjector, Injector, provideZonelessChangeDetection, runInInjectionContext, signal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { defineSettingsPage, msg } from '@decryp7/paddock-ui/shell';
import { DIALOG_REF } from '@decryp7/paddock-ui/shell/dialogs';
import { MemorySettingsBackend, SettingsStore } from '@decryp7/paddock-ui/shell/settings';
import { PdSettingsWindow } from './settings-window';
import { SettingsStatus, settingsPageStatus } from './settings-status';
import { settingsBed, settle, SPEC_PAGES } from './spec-helpers';

/** A DestroyRef the test ends by hand. */
function lifetime(): { ref: DestroyRef; end: () => void } {
  const callbacks: (() => void)[] = [];
  const ref = { onDestroy: (fn: () => void) => (callbacks.push(fn), () => undefined), destroyed: false } as unknown as DestroyRef;
  return { ref, end: () => callbacks.splice(0).forEach((fn) => fn()) };
}

describe('SettingsStatus', () => {
  beforeEach(() => TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection(), SettingsStatus] }));

  it('has no text until a page claims the line, and the latest claim with text wins', () => {
    const status = TestBed.inject(SettingsStatus);
    expect(status.text()).toBeNull();
    const first = signal<string | null>('Save applies the changes.');
    const second = signal<string | null>(null);
    status.claim(first, lifetime().ref);
    expect(status.text()).toBe('Save applies the changes.');
    status.claim(second, lifetime().ref);
    // An empty later claim leaves the earlier one's text.
    expect(status.text()).toBe('Save applies the changes.');
    second.set('Not saved yet.');
    expect(status.text()).toBe('Not saved yet.');
    first.set('');
    second.set(null);
    expect(status.text()).toBeNull();
  });

  it('clears a claim when its page leaves, or early through the returned function (twice is harmless)', () => {
    const status = TestBed.inject(SettingsStatus);
    const page = lifetime();
    status.claim(() => 'From the page', page.ref);
    const other = status.claim(() => 'From another part', lifetime().ref);
    expect(status.text()).toBe('From another part');
    other();
    other();
    expect(status.text()).toBe('From the page');
    page.end();
    expect(status.text()).toBeNull();
  });

  it('settingsPageStatus claims through the injection context and does nothing outside a Settings window', () => {
    const status = TestBed.inject(SettingsStatus);
    // A page's injector: destroying it is the page leaving.
    const page = createEnvironmentInjector([], TestBed.inject(EnvironmentInjector));
    runInInjectionContext(page, () => settingsPageStatus(() => 'Saved with Save'));
    expect(status.text()).toBe('Saved with Save');
    page.destroy();
    expect(status.text()).toBeNull();

    // No SettingsStatus provided: a no-op that returns a harmless clear.
    const bare = Injector.create({ providers: [] });
    const clear = runInInjectionContext(bare, () => settingsPageStatus(() => 'ignored'));
    expect(() => clear()).not.toThrow();
  });
});

/** A settings page that saves with its own button, as an app writes it. */
@Component({
  selector: 'spec-explicit-save-page',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `<p>Signature</p>`,
})
class ExplicitSavePage {
  static readonly dirty = signal(false);
  readonly clear = settingsPageStatus(() => (ExplicitSavePage.dirty() ? 'Not saved yet: Save applies the changes.' : 'Changes on this page are saved with Save.'));
}

describe('pd-settings-window footer status', () => {
  const PAGES = [...SPEC_PAGES, defineSettingsPage({ id: 'sd.explicit', group: 'features', title: msg('sd.settings.explicit', 'Signature'), order: 200, load: () => Promise.resolve(ExplicitSavePage) })];

  async function open(page: string) {
    const backend = new MemorySettingsBackend();
    settingsBed({ backend, pages: PAGES, extra: [{ provide: DIALOG_REF, useValue: { dismiss: vi.fn(), answer: vi.fn(), closed: new Promise(() => undefined) } }] });
    const fixture = TestBed.createComponent(PdSettingsWindow);
    fixture.componentRef.setInput('page', page);
    await settle();
    await fixture.whenStable();
    await settle();
    return fixture;
  }

  beforeEach(() => ExplicitSavePage.dirty.set(false));

  it('shows the page’s own line while it is shown, follows its signal, and returns to the default when the page leaves', async () => {
    const fixture = await open('sd.explicit');
    const window = fixture.componentInstance;
    expect((fixture.nativeElement as HTMLElement).textContent).toContain('Signature');
    expect(window.dialogStatus()).toBe('Changes on this page are saved with Save.');
    ExplicitSavePage.dirty.set(true);
    expect(window.dialogStatus()).toBe('Not saved yet: Save applies the changes.');

    TestBed.inject(SettingsStore).select('sd.general');
    await settle();
    await fixture.whenStable();
    await settle();
    expect(window.dialogStatus()).toBe('Changes are saved as you make them.');
  });

  it('still puts the notice of the change just made first', async () => {
    const fixture = await open('sd.explicit');
    TestBed.inject(SettingsStore).setNotice('General saved.');
    expect(fixture.componentInstance.dialogStatus()).toBe('General saved.');
    TestBed.inject(SettingsStore).setNotice(null);
    expect(fixture.componentInstance.dialogStatus()).toBe('Changes on this page are saved with Save.');
  });
});
