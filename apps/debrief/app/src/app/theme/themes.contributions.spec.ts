import { Injector } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { ContributionRegistry } from '@decryp7/paddock-ui/shell/contributions';
import { contributionProblems } from '@decryp7/paddock-ui/shell/testing';
import { MemoryLogger } from '../../testing/helpers';
import { appConfig } from '../app.config';
import { Logger } from '../core/logger';
import { ThemeService } from '../core/workspace/theme.service';
import { UiPrefsStore } from '../core/workspace/ui-prefs.store';
import { dialogBaseProviders } from '../dialog-windows/dialog-bootstrap';
import { DEBRIEF_KIND } from '../extensions/contributions';
import { BUILT_IN_THEMES } from './themes';
import { provideDebriefThemes, provideThemes } from './themes.contributions';
import { CONSOLE } from './themes/console.theme';

describe('theme packs as contributions (M1)', () => {
  it('registers the built-in packs in Debrief’s themes kind in the main window, and the lint accepts them', () => {
    TestBed.configureTestingModule({ providers: [...appConfig.providers, { provide: Logger, useFactory: () => new MemoryLogger() }] });
    expect(TestBed.inject(ContributionRegistry).items(DEBRIEF_KIND.themes)).toEqual(BUILT_IN_THEMES);
    expect(contributionProblems(TestBed.inject(Injector)).filter((p) => p.startsWith('theme'))).toEqual([]);
  });

  it('registers them in dialog windows too (each is its own bootstrap)', () => {
    TestBed.configureTestingModule({ providers: [...dialogBaseProviders(), { provide: Logger, useFactory: () => new MemoryLogger() }] });
    expect(TestBed.inject(ContributionRegistry).items(DEBRIEF_KIND.themes)).toEqual(BUILT_IN_THEMES);
  });

  it('resolves the chosen theme from the registry (the first registration of an id wins)', () => {
    const tuned = { ...CONSOLE, name: 'Console, tuned' };
    TestBed.configureTestingModule({ providers: [provideThemes([tuned]), provideDebriefThemes()] });
    TestBed.inject(UiPrefsStore).setTheme('console');
    expect(TestBed.inject(ThemeService).theme()).toBe(tuned);
  });

  it('reports a pack that lacks tokens', () => {
    const broken = { ...CONSOLE, id: 'precision' as const, common: {} };
    TestBed.configureTestingModule({ providers: [provideThemes([broken])] });
    expect(contributionProblems(TestBed.inject(Injector)).some((p) => p.startsWith('theme precision') && p.includes('font-sans'))).toBe(true);
  });
});
