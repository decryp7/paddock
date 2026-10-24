import { defineMenuItem, msg, type MenuItemDef } from '@decryp7/paddock-ui/shell';
import { buildMenu, buildMenus, foldAppMenu, itemsAt, type MenuCommandState, type MenuSource } from './menu-model';
import { defineSubmenu, type SubmenuDef } from './submenus';

function source(items: MenuItemDef[], submenus: SubmenuDef[] = [], commands: Record<string, MenuCommandState | null> = {}, hidden: string[] = []): MenuSource {
  return {
    items,
    submenus,
    command: (id) => (id in commands ? commands[id]! : { title: id.toUpperCase(), enabled: true }),
    shown: (e) => !hidden.includes('command' in e ? e.command : e.id),
    keyFor: (id) => (id === 'sd.a' ? 'Mod+A' : undefined),
  };
}

const at = (menu: string, command: string, group: string, order: number) => defineMenuItem({ menu, command, group, order });

describe('menu model', () => {
  it('groups by the lowest order, separates groups and sorts by order then id', () => {
    const src = source([at('file', 'sd.c', 'late', 300), at('file', 'sd.b', 'first', 200), at('file', 'sd.a', 'first', 100), at('file', 'sd.d', 'late', 50), at('file', 'sd.e', 'first', 200)]);
    expect(buildMenu('file', src)).toEqual([
      { kind: 'item', id: 'sd.d', label: 'SD.D', enabled: true },
      { kind: 'item', id: 'sd.c', label: 'SD.C', enabled: true },
      { kind: 'separator' },
      { kind: 'item', id: 'sd.a', label: 'SD.A', enabled: true, keybinding: 'Mod+A' },
      { kind: 'item', id: 'sd.b', label: 'SD.B', enabled: true },
      { kind: 'item', id: 'sd.e', label: 'SD.E', enabled: true },
    ]);
  });

  it('shows enabled and checked states from the commands', () => {
    const src = source([at('view', 'sd.t', 'main', 1), at('view', 'sd.off', 'main', 2)], [], { 'sd.t': { title: 'Toggle', enabled: true, checked: false }, 'sd.off': { title: 'Off', enabled: false } });
    expect(buildMenu('view', src)).toEqual([
      { kind: 'item', id: 'sd.t', label: 'Toggle', enabled: true, checked: false },
      { kind: 'item', id: 'sd.off', label: 'Off', enabled: false },
    ]);
  });

  it('leaves out unavailable commands, hidden entries, and menus and submenus that end up empty', () => {
    const sub = defineSubmenu({ id: 'sd.more', menu: 'view', title: msg('sd.more', 'More'), order: 10 });
    const empty = defineSubmenu({ id: 'sd.empty', menu: 'view', title: msg('sd.empty', 'Empty'), order: 20 });
    const src = source([at('view', 'sd.gone', 'main', 1), at('sd.more', 'sd.in', 'main', 1), at('sd.empty', 'sd.hidden', 'main', 1), at('help', 'sd.gone', 'main', 1)], [sub, empty], { 'sd.gone': null }, ['sd.hidden']);
    expect(buildMenus([{ id: 'view', label: 'View' }, { id: 'help', label: 'Help' }], src)).toEqual([
      { id: 'view', label: 'View', items: [{ kind: 'submenu', id: 'sd.more', label: 'More', items: [{ kind: 'item', id: 'sd.in', label: 'SD.IN', enabled: true }] }] },
    ]);
    const hiddenSub = source([at('sd.more', 'sd.in', 'main', 1)], [sub], {}, ['sd.more']);
    expect(buildMenu('view', hiddenSub)).toEqual([]);
  });

  it('never lists a command twice in one menu and survives a submenu cycle', () => {
    const a = defineSubmenu({ id: 'sd.a', menu: 'sd.b', title: msg('sd.a', 'A'), order: 1 });
    const b = defineSubmenu({ id: 'sd.b', menu: 'sd.a', title: msg('sd.b', 'B'), order: 1 });
    const src = source([at('file', 'sd.x', 'one', 1), at('file', 'sd.x', 'two', 2), at('sd.a', 'sd.y', 'main', 1)], [a, b]);
    expect(buildMenu('file', src)).toEqual([{ kind: 'item', id: 'sd.x', label: 'SD.X', enabled: true }]);
    expect(buildMenu('sd.a', src).length).toBeGreaterThan(0);
    const deep = [1, 2, 3, 4].map((n) => defineSubmenu({ id: `sd.d${n}`, menu: n === 1 ? 'file' : `sd.d${n - 1}`, title: msg(`sd.d${n}`, `D${n}`), order: 1 }));
    const deepSrc = source([at('sd.d4', 'sd.leaf', 'main', 1), at('sd.d3', 'sd.mid', 'main', 1)], deep);
    const d1 = buildMenu('file', deepSrc)[0];
    expect(d1?.kind).toBe('submenu');
    expect(JSON.stringify(d1)).not.toContain('sd.leaf');
    expect(JSON.stringify(d1)).toContain('sd.mid');
  });

  it('finds the items at a path, empty once gone', () => {
    const menus = [{ id: 'view', label: 'View', items: [{ kind: 'submenu' as const, id: 'sd.more', label: 'More', items: [{ kind: 'item' as const, id: 'sd.x', label: 'X', enabled: true }] }] }];
    expect(itemsAt(menus, ['view', 'sd.more'])).toEqual([{ kind: 'item', id: 'sd.x', label: 'X', enabled: true }]);
    expect(itemsAt(menus, ['view', 'sd.gone'])).toEqual([]);
    expect(itemsAt(menus, ['help'])).toEqual([]);
  });
});

describe('foldAppMenu', () => {
  const item = (id: string) => ({ kind: 'item', id, label: id, enabled: true }) as const;
  it('appends the app menu to File after a separator and drops it', () => {
    const menus = [
      { id: 'app', label: 'Demo', items: [item('pd.settings.open')] },
      { id: 'file', label: 'File', items: [item('sd.open')] },
      { id: 'help', label: 'Help', items: [item('sd.about')] },
    ];
    expect(foldAppMenu(menus, 'File')).toEqual([
      { id: 'file', label: 'File', items: [item('sd.open'), { kind: 'separator' }, item('pd.settings.open')] },
      { id: 'help', label: 'Help', items: [item('sd.about')] },
    ]);
  });

  it('makes a File menu when there is none, and leaves menus without an app menu as they are', () => {
    expect(foldAppMenu([{ id: 'app', label: 'Demo', items: [item('a')] }], 'Fichier')).toEqual([{ id: 'file', label: 'Fichier', items: [item('a')] }]);
    const plain = [{ id: 'view', label: 'View', items: [item('v')] }];
    expect(foldAppMenu(plain, 'File')).toEqual(plain);
  });
});
