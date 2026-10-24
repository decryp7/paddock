import { TestBed } from '@angular/core/testing';
import { clearTranslations, loadTranslations } from '@angular/localize';
import { msg } from '@decryp7/paddock-ui/shell';
import {
  activeLocale,
  groupedNumber,
  initI18n,
  LANGUAGE_CONFIG,
  LANGUAGE_KEY,
  LanguageStore,
  loaderFor,
  LocalStorageLanguageStorage,
  parseLanguage,
  plural,
  PSEUDO_LOCALE,
  resolveLocale,
  setActiveLocale,
  SOURCE_LOCALE,
  textDirection,
  type LanguageChoice,
  type LanguageStorage,
} from '@decryp7/paddock-ui/shell/i18n';

class MemoryLanguage implements LanguageStorage {
  constructor(public choice: LanguageChoice = 'system') {}
  load(): LanguageChoice {
    return this.choice;
  }
  save(choice: LanguageChoice): void {
    this.choice = choice;
  }
}

afterEach(() => {
  clearTranslations();
  setActiveLocale(SOURCE_LOCALE);
  localStorage.clear();
});

describe('locale', () => {
  it('resolves the choice: English, pseudo, or the first shipped system language', () => {
    expect(resolveLocale('en', ['de-DE'])).toBe(SOURCE_LOCALE);
    expect(resolveLocale('pseudo', [])).toBe(PSEUDO_LOCALE);
    expect(resolveLocale('system', ['de-DE', 'en-GB'])).toBe('en-GB');
    expect(resolveLocale('system', ['de-DE'], ['de-DE'])).toBe('de-DE');
    expect(resolveLocale('system', ['fr-FR'], [PSEUDO_LOCALE])).toBe(SOURCE_LOCALE);
    expect(resolveLocale('system', ['', 'en-gb'])).toBe('en-GB');
  });

  it('skips a malformed tag', () => {
    expect(resolveLocale('system', ['en-$$', 'en-AU'])).toBe('en-AU');
  });

  it('knows right-to-left languages', () => {
    expect(textDirection('ar-EG')).toBe('rtl');
    expect(textDirection('en-US')).toBe('ltr');
  });

  it('formats numbers and picks plural forms in the active locale (the pseudo-locale formats as English)', () => {
    setActiveLocale(PSEUDO_LOCALE);
    expect(groupedNumber(12345)).toBe('12,345');
    expect(plural(1, { one: 'one job', other: 'jobs' })).toBe('one job');
    expect(plural(0, { '=0': 'no jobs', other: 'jobs' })).toBe('no jobs');
    expect(plural(3, { other: 'jobs' })).toBe('jobs');
    setActiveLocale('de-DE');
    expect(groupedNumber(12345)).toBe('12.345');
    expect(activeLocale()).toBe('de-DE');
  });
});

describe('language storage', () => {
  it('reads only its own version and known choices', () => {
    const s = new LocalStorageLanguageStorage();
    expect(s.load()).toBe('system');
    s.save('pseudo');
    expect(JSON.parse(localStorage.getItem(LANGUAGE_KEY) ?? '')).toEqual({ version: 1, language: 'pseudo' });
    expect(s.load()).toBe('pseudo');
    localStorage.setItem(LANGUAGE_KEY, JSON.stringify({ version: 2, language: 'en' }));
    expect(s.load()).toBe('system');
    localStorage.setItem(LANGUAGE_KEY, '{oops');
    expect(s.load()).toBe('system');
    expect(parseLanguage('klingon')).toBe('system');
  });
});

describe('initI18n', () => {
  const root = (): HTMLElement => document.createElement('html');

  it('loads the pseudo-locale and sets lang and dir; msg() texts follow the loaded translations', async () => {
    const el = root();
    const title = msg('pd.test.title', 'Jobs');
    const locale = await initI18n({ loaders: { [PSEUDO_LOCALE]: async () => ({ 'pd.test.title': '⟦Ĵööƀš⟧' }) }, storage: new MemoryLanguage('pseudo'), root: el });
    expect(locale).toBe(PSEUDO_LOCALE);
    expect(el.lang).toBe(PSEUDO_LOCALE);
    expect(el.dir).toBe('ltr');
    expect(activeLocale()).toBe(PSEUDO_LOCALE);
    expect(title.text).toBe('⟦Ĵööƀš⟧');
    expect($localize`:@@pd.test.title:Jobs`).toBe('⟦Ĵööƀš⟧');
    expect(msg('pd.test.other', 'Other').text).toBe('Other');
  });

  it('English needs no file and loads none', async () => {
    let loaded = 0;
    const el = root();
    const locale = await initI18n({ loaders: { [PSEUDO_LOCALE]: async () => (loaded++, {}) }, storage: new MemoryLanguage('en'), root: el, languages: ['ar-EG'] });
    expect(locale).toBe(SOURCE_LOCALE);
    expect(loaded).toBe(0);
    expect(el.lang).toBe(SOURCE_LOCALE);
  });

  it('a broken or failing translation file falls back to English and says so', async () => {
    const warnings: string[] = [];
    const warn = (m: string): void => void warnings.push(m);
    for (const loader of [async () => ({ x: 1 }) as never, async () => [] as never, () => Promise.reject(new Error('chunk missing'))]) {
      const el = root();
      expect(await initI18n({ loaders: { [PSEUDO_LOCALE]: loader }, storage: new MemoryLanguage('pseudo'), root: el, warn })).toBe(SOURCE_LOCALE);
      expect(el.lang).toBe(SOURCE_LOCALE);
    }
    expect(warnings).toHaveLength(3);
    expect(warnings[0]).toContain('using English');
  });

  it('uses a loader of another region of the same language, never the pseudo-locale for a real one', () => {
    const de = async () => ({});
    expect(loaderFor('de-AT', { 'de-DE': de })).toBe(de);
    expect(loaderFor('en-GB', { [PSEUDO_LOCALE]: de })).toBeUndefined();
    expect(loaderFor(PSEUDO_LOCALE, { 'en-US': de })).toBeUndefined();
  });

  it('reads the stored choice, the browser languages and <html> by default', async () => {
    localStorage.setItem(LANGUAGE_KEY, JSON.stringify({ version: 1, language: 'en' }));
    expect(await initI18n({ loaders: {} })).toBe(SOURCE_LOCALE);
    expect(document.documentElement.lang).toBe(SOURCE_LOCALE);
  });

  it('installs translations with loadTranslations by default (the same table $localize reads)', async () => {
    await initI18n({ loaders: { 'de-DE': async () => ({ 'pd.test.hello': 'Hallo' }) }, storage: new MemoryLanguage('system'), languages: ['de-DE'], root: root() });
    expect($localize`:@@pd.test.hello:Hello`).toBe('Hallo');
  });
});

describe('msg without translations', () => {
  it('is its source text, also after translations were cleared', () => {
    loadTranslations({ 'pd.test.a': 'A!' });
    const m = msg('pd.test.a', 'A');
    expect(m.text).toBe('A!');
    clearTranslations();
    expect(m.text).toBe('A');
    expect(Object.isFrozen(m)).toBe(true);
  });
});

describe('LanguageStore', () => {
  function setup(choice: LanguageChoice, systemLanguages: readonly string[] = ['en-US']) {
    const storage = new MemoryLanguage(choice);
    let reloads = 0;
    TestBed.configureTestingModule({
      providers: [LanguageStore, { provide: LANGUAGE_CONFIG, useValue: { storage, systemLanguages: () => systemLanguages, shipped: [PSEUDO_LOCALE], reload: () => reloads++ } }],
    });
    return { store: TestBed.inject(LanguageStore), storage, reloads: () => reloads };
  }

  it('saves a new choice and says a restart is needed; reload asks the app to restart', () => {
    const { store, storage, reloads } = setup('system');
    expect(store.reloadPending()).toBe(false);
    expect(store.locale()).toBe(SOURCE_LOCALE);
    store.choose('pseudo');
    expect(storage.choice).toBe('pseudo');
    expect(store.reloadPending()).toBe(true);
    store.choose('pseudo');
    store.reload();
    expect(reloads()).toBe(1);
  });

  it('choosing what the window already shows needs no restart', () => {
    const { store } = setup('pseudo');
    store.choose('en');
    expect(store.reloadPending()).toBe(false);
  });

  it('defaults to localStorage and the page reload', () => {
    TestBed.configureTestingModule({ providers: [LanguageStore] });
    const config = TestBed.inject(LANGUAGE_CONFIG);
    expect(config.shipped).toEqual([]);
    expect(config.systemLanguages().length).toBeGreaterThan(0);
    expect(TestBed.inject(LanguageStore).choice()).toBe('system');
  });
});
