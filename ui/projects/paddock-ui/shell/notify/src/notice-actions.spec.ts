import { TestBed } from '@angular/core/testing';
import { NoticeActions, repairButtonLabel, repairLabel } from './notice-actions';
import { OPEN_SETTINGS } from './notify-config';
import { notifyBed } from './spec-helpers';
import type { Notice } from './wire.types';

const notice = { id: 1, title: 't' } as Notice;

describe('NoticeActions', () => {
  it('labels every repair', () => {
    expect((['restart', 'reset', 'liftQuarantine', 'install', 'retry', 'openSettings', 'useFallback'] as const).map(repairLabel)).toEqual([
      'Restart',
      'Try again',
      'Allow again',
      'Install',
      'Try again',
      'Open Settings…',
      'Use the fallback',
    ]);
  });

  it('names the chip’s Repair button for its action and service (rule 19)', () => {
    expect(repairButtonLabel('restart', 'the demo engine')).toBe('Restart demo engine');
    expect(repairButtonLabel('reset', 'the search index')).toBe('Reset search index');
    expect(repairButtonLabel('install', 'model runtime')).toBe('Install model runtime');
    expect(repairButtonLabel('retry', 'the store')).toBe('Try again');
  });

  it('opens Settings when the app can, and has no button otherwise', async () => {
    const opened: string[] = [];
    notifyBed({ extra: [{ provide: OPEN_SETTINGS, useValue: (s: string) => void opened.push(s) }] });
    const actions = TestBed.inject(NoticeActions);
    const a = { type: 'openSettings', section: 'performance' };
    expect(actions.available(a)).toBe(true);
    expect(actions.label(a, notice)).toBe('Open Settings…');
    await expect(actions.run(a, notice)).resolves.toBeNull();
    await actions.run({ type: 'openSettings' }, notice);
    expect(opened).toEqual(['performance', '']);
    TestBed.resetTestingModule();
    notifyBed();
    const without = TestBed.inject(NoticeActions);
    expect(without.available(a)).toBe(false);
    expect(without.label(a, notice)).toBe('openSettings');
    await expect(without.run(a, notice)).resolves.toBe('This window cannot do “openSettings”.');
  });
});
