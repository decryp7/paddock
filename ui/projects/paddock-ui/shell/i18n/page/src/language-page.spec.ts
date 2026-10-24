import { provideZonelessChangeDetection } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { defineLanguageSettingsPage, LANGUAGE_SETTINGS_PAGE } from '@decryp7/paddock-ui/shell/i18n/settings';
import { LANGUAGE_CONFIG, PSEUDO_LOCALE, setActiveLocale, SOURCE_LOCALE, type LanguageChoice, type LanguageStorage } from '@decryp7/paddock-ui/shell/i18n';
import { PdLanguagePage } from './language-page';

class MemoryLanguage implements LanguageStorage {
  constructor(public choice: LanguageChoice = 'system') {}
  load(): LanguageChoice {
    return this.choice;
  }
  save(choice: LanguageChoice): void {
    this.choice = choice;
  }
}

async function open(shipped: string[], storage = new MemoryLanguage()) {
  let reloads = 0;
  TestBed.configureTestingModule({
    providers: [provideZonelessChangeDetection(), { provide: LANGUAGE_CONFIG, useValue: { storage, systemLanguages: () => ['en-US'], shipped, reload: () => reloads++ } }],
  });
  const fixture = TestBed.createComponent(PdLanguagePage);
  await fixture.whenStable();
  const el = fixture.nativeElement as HTMLElement;
  return {
    el,
    storage,
    reloads: () => reloads,
    fixture,
    radios: () => [...el.querySelectorAll<HTMLInputElement>('input[type=radio]')],
    choose: async (value: string) => {
      const r = el.querySelector<HTMLInputElement>(`[data-testid="language-${value}"]`);
      r?.click();
      await fixture.whenStable();
    },
  };
}

afterEach(() => setActiveLocale(SOURCE_LOCALE));

describe('the Language page', () => {
  it('lists System default, English and the shipped translations by their own names, with the saved choice checked', async () => {
    const p = await open(['de', 'fr'], new MemoryLanguage('de'));
    expect(p.radios().map((r) => r.value)).toEqual(['system', 'en', 'de', 'fr']);
    expect(p.radios().filter((r) => r.checked).map((r) => r.value)).toEqual(['de']);
    expect(p.el.textContent).toContain('Deutsch');
    expect(p.el.querySelector('[role=radiogroup]')?.getAttribute('aria-labelledby')).toBe('pd-language-title');
    // The window runs in English, the saved choice is German: the restart is offered from the start.
    expect(p.el.querySelector('[data-testid="language-restart"]')).not.toBeNull();
  });

  it('offers the pseudo-locale only where the app ships it', async () => {
    expect((await open(['de'])).radios().map((r) => r.value)).not.toContain('pseudo');
    TestBed.resetTestingModule();
    expect((await open([PSEUDO_LOCALE])).radios().map((r) => r.value)).toContain('pseudo');
  });

  it('saves a choice at once, says the window has to restart and restarts it on request', async () => {
    const p = await open(['de']);
    await p.choose('de');
    expect(p.storage.choice).toBe('de');
    const bar = p.el.querySelector('[data-testid="language-restart"]');
    expect(bar?.getAttribute('role')).toBe('status');
    (p.el.querySelector('[data-testid="language-restart-now"]') as HTMLButtonElement).click();
    expect(p.reloads()).toBe(1);
  });

  it('choosing what the window already shows needs no restart', async () => {
    const p = await open(['de']);
    await p.choose('en');
    expect(p.storage.choice).toBe('en');
    expect(p.el.querySelector('[data-testid="language-restart"]')).toBeNull();
  });
});

describe('defineLanguageSettingsPage', () => {
  it('is a frozen application page with the shell’s id and the app’s loaders', () => {
    const load = () => Promise.resolve(PdLanguagePage);
        const page = defineLanguageSettingsPage({ load });
    expect(page).toMatchObject({ id: LANGUAGE_SETTINGS_PAGE, group: 'application', load });
    expect(page.title.id).toBe('pd.settings.language');
    expect(Object.isFrozen(page)).toBe(true);
  });
});
