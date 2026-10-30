import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../core/backend';
import type { MockBackend } from '../core/mock-backend';
import { makeMock } from '../../testing/test-helpers';
import { DebriefStore } from './debrief.store';
import { NoticeLog, recordShell } from '../../testing/test-helpers';

function setup() {
  const mock = makeMock({ scenario: 'demo' });
  TestBed.configureTestingModule({ providers: [...recordShell(), { provide: BACKEND, useValue: mock }] });
  return { mock, store: TestBed.inject(DebriefStore), toasts: TestBed.inject(NoticeLog) };
}

describe('DebriefStore', () => {
  let mock: MockBackend;
  afterEach(() => {
    mock.dispose();
    TestBed.resetTestingModule();
  });

  it('looks for Debrief once and falls back to "not installed" when asking fails', async () => {
    const s = setup();
    mock = s.mock;
    const spy = vi.spyOn(mock, 'debriefStatus');
    await s.store.load();
    await s.store.load();
    expect(spy).toHaveBeenCalledTimes(1);
    expect(s.store.status()?.installed).toBe(true);
    TestBed.resetTestingModule();
    const t = setup();
    mock = t.mock;
    vi.spyOn(mock, 'debriefStatus').mockRejectedValue(new Error('x'));
    await t.store.load();
    expect(t.store.status()).toEqual({ installed: false, program: null, needsUpdate: false, cliName: 'debrief' });
  });

  it('says to update Debrief when only the old program was found', async () => {
    const s = setup();
    mock = s.mock;
    vi.spyOn(mock, 'debriefStatus').mockResolvedValue({ installed: false, program: null, needsUpdate: true, cliName: 'debrief' });
    vi.spyOn(mock, 'openInDebrief').mockResolvedValue({ result: 'notInstalled', file: '/f.log', message: 'Update Debrief.' });
    await s.store.load();
    await s.store.open('build-win', 'x');
    expect(s.toasts.notices.at(-1)?.message).toContain('Update Debrief to use Open in Debrief');
  });

  it('tells the user what happened: opened, not installed, nothing to open, failure', async () => {
    const s = setup();
    mock = s.mock;
    await s.store.open('build-win', 'run-1', 1, 2);
    expect(s.toasts.notices.at(-1)?.message).toContain('Sent to Debrief');
    vi.spyOn(mock, 'openInDebrief').mockResolvedValueOnce({ result: 'notInstalled', file: '/f.log', message: 'Install it.' });
    await s.store.open('build-win', 'x');
    expect(s.toasts.notices.at(-1)).toMatchObject({ level: 'warning' });
    expect(s.toasts.notices.at(-1)?.message).toContain('Debrief is not installed');
    expect(s.toasts.notices.at(-1)?.message).toContain('/f.log');
    vi.spyOn(mock, 'openInDebrief').mockResolvedValueOnce({ result: 'noLines', message: 'Rotated away.' });
    await s.store.open('build-win', 'x', 1, 2);
    expect(s.toasts.notices.at(-1)?.message).toContain('Nothing to open');
    vi.spyOn(mock, 'openInDebrief').mockRejectedValueOnce({ code: 'internal', message: 'boom' });
    await s.store.open('build-win', 'x');
    expect(s.toasts.notices.at(-1)).toMatchObject({ level: 'error' });
    expect(s.toasts.notices.at(-1)?.message).toContain('Could not open Debrief');
    expect(s.store.busy()).toBe(false);
  });
});
