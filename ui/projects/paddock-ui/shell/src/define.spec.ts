import { fakeDialogContract } from '@decryp7/paddock-ui/shell/testing';
import { defineCommand, defineDialog, defineHealthService, defineKeybinding, defineMenuItem, defineNotificationKind, definePage, defineSettingsPage, defineStatusItem } from './define';
import { isMessage, msg } from './message';
import { type, type IpcCommandName } from './types';
import type { ShellCommand } from '../generated/shell-commands';

const load = () => Promise.resolve(class {});

describe('define*', () => {
  it('returns frozen values with defaults filled in', () => {
    const cmd = defineCommand({ id: 'sd.hello', title: msg('sd.hello', 'Hello'), run: () => undefined });
    expect(Object.isFrozen(cmd)).toBe(true);
    expect(defineKeybinding({ command: 'sd.hello', key: 'Mod+H' }).scope).toBe('window');
    expect(defineMenuItem({ menu: 'view', command: 'sd.hello', order: 100 }).group).toBe('main');
    expect(definePage({ id: 'sd.home', title: msg('sd.home', 'Home'), icon: 'pd:home', order: 100, load }).section).toBe('top');
    expect(Object.isFrozen(defineStatusItem({ id: 'sd.s', title: msg('sd.s', 'S'), side: 'end', priority: 1, load }))).toBe(true);
    expect(Object.isFrozen(defineSettingsPage({ id: 'sd.p', group: 'application', title: msg('sd.p', 'P'), order: 1, load }))).toBe(true);
    expect(Object.isFrozen(defineHealthService({ id: 'sd.engine', label: msg('sd.engine', 'the engine'), icon: 'pd:engine' }))).toBe(true);
    const kind = defineNotificationKind({ id: 'sd.update', title: msg('sd.update', 'Update'), actions: ['install'] });
    expect(Object.isFrozen(kind.actions)).toBe(true);
  });

  it('takes a dialog from its Rust contract and adds only what TypeScript knows', () => {
    const contract = fakeDialogContract({ id: 'sd.form', kind: 'form', modality: 'modeless', commands: ['a_x'] });
    const ui = { title: msg('sd.d', 'D'), load, result: type<boolean>() };
    const def = defineDialog(contract, ui);
    expect(def).toEqual({ ...contract, ...ui, commands: ['a_x'] });
    expect(Object.isFrozen(def) && Object.isFrozen(def.commands)).toBe(true);
    expect(defineDialog(fakeDialogContract({ id: 'sd.ask', kind: 'decision' }), ui).modality).toBe('modal');
  });

  it("types dialog commands as the app's names plus the shell plugin's", () => {
    // Without a ShellTypes.appCommand augmentation the app's part is string; the shell's part is the plugin's table (S17).
    expectTypeOf<IpcCommandName>().toEqualTypeOf<string>();
    expectTypeOf<'notifications_post' | 'shell_version' | 'dialog_open'>().toExtend<ShellCommand>();
    expectTypeOf<'demo_status'>().not.toExtend<ShellCommand>();
  });

  it('checks messages', () => {
    expect(isMessage(msg('pw.models.refresh', 'Refresh'))).toBe(true);
    expect(isMessage(msg('Refresh', 'Refresh'))).toBe(false);
    expect(isMessage(msg('pw.x', '  '))).toBe(false);
    expect(isMessage('pw.x')).toBe(false);
    expect(isMessage(null)).toBe(false);
  });
});
