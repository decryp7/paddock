import { TestBed } from '@angular/core/testing';
import { SHELL_PORTS, type ShellNotice } from '@decryp7/paddock-ui/shell';
import { Notifier, UNDO_ACTION } from './notifier';

function setup() {
  const notices: ShellNotice[] = [];
  TestBed.configureTestingModule({ providers: [{ provide: SHELL_PORTS, useValue: { showPage: vi.fn(), openDialog: vi.fn(), notify: (n: ShellNotice) => void notices.push(n) } }] });
  return { notifier: TestBed.inject(Notifier), notices };
}

describe('Notifier (the shell’s notification host)', () => {
  it('maps Dyno’s tones to the shell’s levels', () => {
    const { notifier, notices } = setup();
    notifier.show('Saved');
    notifier.show('Careful', 'warn');
    notifier.show('Broken', 'bad');
    notifier.error({ code: 'internal', message: 'No.' });
    expect(notices.map((n) => [n.level, n.message])).toEqual([['success', 'Saved'], ['warning', 'Careful'], ['error', 'Broken'], ['error', 'No.']]);
  });

  it('keeps the latest notices for a test to read back, at most twenty', () => {
    const { notifier } = setup();
    for (let i = 0; i < 25; i++) notifier.show(`n${i}`);
    expect(notifier.recent()).toHaveLength(20);
    expect(notifier.recent().at(-1)?.text).toBe('n24');
  });

  it('turns an action into the notice’s Undo button, run once in this window', () => {
    const { notifier, notices } = setup();
    const run = vi.fn();
    notifier.show('Deleted', 'ok', { label: 'Undo', run });
    const action = notices[0]!.action as { type: string; id: number; label: string };
    expect(action).toMatchObject({ type: UNDO_ACTION, label: 'Undo' });
    notifier.runAction(action.id);
    notifier.runAction(action.id);
    expect(run).toHaveBeenCalledTimes(1);
  });
});
