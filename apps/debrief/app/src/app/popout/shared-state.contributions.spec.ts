import { Injector, signal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { ContributionRegistry } from '@decryp7/paddock-ui/shell/contributions';
import { contributionProblems } from '@decryp7/paddock-ui/shell/testing';
import { MemoryLogger, provideFakeDock, provideTinyBackend } from '../../testing/helpers';
import { appConfig } from '../app.config';
import { Logger } from '../core/logger';
import { dialogBaseProviders } from '../dialog-windows/dialog-bootstrap';
import { DEBRIEF_KIND } from '../extensions/contributions';
import { DEBRIEF_SHARED_STATE, provideSharedState, sharedUiState, type SharedStateDef } from './shared-state';

describe('shared state as contributions (M1)', () => {
  it('registers the built-in groups in Debrief’s sharedState kind in the main window, and the lint accepts them', () => {
    TestBed.configureTestingModule({ providers: [...appConfig.providers, { provide: Logger, useFactory: () => new MemoryLogger() }] });
    expect(TestBed.inject(ContributionRegistry).items(DEBRIEF_KIND.sharedState)).toEqual(DEBRIEF_SHARED_STATE);
    expect(contributionProblems(TestBed.inject(Injector)).filter((p) => p.startsWith('shared state'))).toEqual([]);
  });

  it('gives a dialog window the built-in groups without registering them (its bootstrap must not import the stores they bind)', () => {
    TestBed.configureTestingModule({ providers: [...dialogBaseProviders(), provideTinyBackend(), provideFakeDock(), { provide: Logger, useFactory: () => new MemoryLogger() }] });
    expect(TestBed.inject(ContributionRegistry).items(DEBRIEF_KIND.sharedState)).toEqual([]);
    expect(sharedUiState(TestBed.inject(Injector), 'display').map((v) => v.key)).toContain('theme');
  });

  it('syncs a contributed group (a plugin’s) in the scopes that include it, after the built-ins', () => {
    const value = signal(1);
    const plugin: SharedStateDef = {
      id: 'db.shared.test',
      scope: 'summary',
      values: () => [{ key: 'testValue', read: () => value(), apply: (v) => typeof v === 'number' && value.set(v) }],
    };
    TestBed.configureTestingModule({ providers: [provideTinyBackend(), provideFakeDock(), ...provideSharedState(DEBRIEF_SHARED_STATE), ...provideSharedState([plugin])] });
    const injector = TestBed.inject(Injector);
    const keys = (scope: 'display' | 'summary' | 'full') => sharedUiState(injector, scope).map((v) => v.key);
    expect(keys('display')).not.toContain('testValue');
    expect(keys('summary').at(-1)).toBe('testValue');
    expect(keys('full')).toEqual([...keys('full').filter((k) => k !== 'testValue'), 'testValue']);
    expect(keys('full').slice(0, 3)).toEqual(['query', 'selected', 'jump']);
    expect(keys('display')).toEqual(['theme', 'scheme', 'uiFont', 'timezone', 'lifecycle', 'dateFormat', 'density', 'sourceColors']);
  });
});
