import { EnvironmentInjector, createEnvironmentInjector } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { Router } from '@angular/router';
import { appProviders, makeMock } from '../../testing/test-helpers';
import { provideDeepLinks, safeRoute } from './deep-link';

describe('safeRoute', () => {
  it('lets the known screens and runner pages through', () => {
    for (const ok of ['/', '/runners', '/activity', '/logs', '/updates', '/settings', '/runners/build-win', '/logs/mac.mini_1']) {
      expect(safeRoute(ok)).toBe(ok);
    }
  });

  it('refuses anything else', () => {
    for (const bad of [
      '',
      'logs',
      '//evil.example',
      'https://evil.example',
      '/unknown',
      '/settings/x',
      '/runners/a/b',
      '/runners/../settings',
      '/logs/bad name',
      '/logs/-x',
      '/logs?runner=a',
      '/logs#x',
      '/runners/' + 'a'.repeat(65),
      '/' + 'a'.repeat(200),
      '/runners/',
    ]) {
      expect(safeRoute(bad), bad).toBeNull();
    }
    for (const notText of [null, undefined, 3, {}]) expect(safeRoute(notText)).toBeNull();
  });
});

describe('provideDeepLinks', () => {
  it('navigates when the shell asks, ignores unsafe routes and stops when destroyed', () => {
    const mock = makeMock();
    TestBed.configureTestingModule({ providers: appProviders(mock) });
    const router = TestBed.inject(Router);
    const go = vi.spyOn(router, 'navigateByUrl').mockResolvedValue(true);
    const env = createEnvironmentInjector([provideDeepLinks()], TestBed.inject(EnvironmentInjector));
    mock.requestNavigation('/logs/build-win');
    mock.requestNavigation('https://evil.example');
    expect(go.mock.calls).toEqual([['/logs/build-win']]);
    env.destroy();
    mock.requestNavigation('/settings');
    expect(go).toHaveBeenCalledTimes(1);
    mock.dispose();
  });
});
