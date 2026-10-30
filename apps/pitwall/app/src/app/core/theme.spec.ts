import { TestBed } from '@angular/core/testing';
import { AppearanceStore, BrowserAppearanceBackend, provideAppearance } from '@decryp7/paddock-ui/shell';
import { THEME_TOKENS } from '../shared/theme-tokens';
import { ThemeService } from './theme';

type Listener = (e: { matches: boolean }) => void;

function mockMedia(matches: boolean) {
  let listener: Listener | null = null;
  const mql = { matches, media: '', addEventListener: (_: string, l: Listener) => (listener = l), removeEventListener: () => undefined };
  vi.spyOn(window, 'matchMedia').mockReturnValue(mql as unknown as MediaQueryList);
  return { fire: (m: boolean) => listener?.({ matches: m }) };
}

describe('ThemeService', () => {
  afterEach(() => vi.restoreAllMocks());

  it('without the shell’s appearance follows the system and its live changes', () => {
    const media = mockMedia(false);
    const t = TestBed.inject(ThemeService);
    expect(t.resolved()).toBe('light');
    expect(t.tokens()).toBe(THEME_TOKENS.light);
    media.fire(true);
    expect(t.resolved()).toBe('dark');
    expect(t.tokens()).toBe(THEME_TOKENS.dark);
  });

  it('follows the shell’s Appearance setting in a window that has one', () => {
    mockMedia(false);
    TestBed.configureTestingModule({
      providers: [provideAppearance({ accent: 'pitwall', backend: () => new BrowserAppearanceBackend({ storage: null, channel: null }) })],
    });
    const store = TestBed.inject(AppearanceStore);
    const t = TestBed.inject(ThemeService);
    store.setTheme('dark');
    expect(t.resolved()).toBe('dark');
    expect(t.tokens()).toBe(THEME_TOKENS.dark);
    store.setTheme('light');
    expect(t.resolved()).toBe('light');
  });
});
