import { defineCommand, defineDialog, defineHealthService, defineKeybinding, defineMenuItem, defineNotificationKind, definePage, defineSettingsPage, defineStatusItem } from './define';
import { isMessage, msg } from './message';
import { type } from './types';

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

  it('resolves a dialog modality only where the kind decides', () => {
    const base = { title: msg('sd.d', 'D'), commands: [], load, result: type<boolean>() };
    expect(defineDialog({ ...base, id: 'sd.ask', kind: 'decision' }).modality).toBe('modal');
    expect(defineDialog({ ...base, id: 'sd.view', kind: 'viewer' }).modality).toBe('modeless');
    expect(defineDialog({ ...base, id: 'sd.form', kind: 'form', modality: 'modeless' }).modality).toBe('modeless');
    expect(defineDialog({ ...base, id: 'sd.form2', kind: 'form' }).modality).toBeUndefined();
    expect(defineDialog({ ...base, id: 'sd.bad', kind: 'nope' as 'form' }).modality).toBeUndefined();
  });

  it('checks messages', () => {
    expect(isMessage(msg('pw.models.refresh', 'Refresh'))).toBe(true);
    expect(isMessage(msg('Refresh', 'Refresh'))).toBe(false);
    expect(isMessage(msg('pw.x', '  '))).toBe(false);
    expect(isMessage('pw.x')).toBe(false);
    expect(isMessage(null)).toBe(false);
  });
});
