import { TestBed } from '@angular/core/testing';
import { SHELL_PORTS, type ShellNotice } from '@decryp7/paddock-ui/shell';
import { Notify, levelOf, messageOf } from './notify';

describe('Notify', () => {
  function setup() {
    const sent: ShellNotice[] = [];
    TestBed.configureTestingModule({
      providers: [{ provide: SHELL_PORTS, useValue: { showPage: () => undefined, openDialog: async () => undefined, notify: (n: ShellNotice) => sent.push(n) } }],
    });
    return { notify: TestBed.inject(Notify), sent };
  }

  it('hands a notice to the shell with its level and one line', () => {
    const { notify, sent } = setup();
    notify.show({ tone: 'success', icon: 'check', title: 'Registered build-pc', body: null });
    notify.show({ tone: 'danger', title: 'build-pc: that did not work', body: 'no space' });
    expect(sent).toEqual([
      { level: 'success', message: 'Registered build-pc' },
      { level: 'error', message: 'build-pc: that did not work: no space' },
    ]);
  });

  it('maps every tone to a shell level', () => {
    expect(['success', 'info', 'warning', 'danger', 'neutral'].map((t) => levelOf(t as never))).toEqual(['success', 'info', 'warning', 'error', 'info']);
    expect(messageOf({ title: 'a', body: 'b' })).toBe('a: b');
  });
});
