import { EnvironmentInjector, provideZonelessChangeDetection } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { defineCommand, defineMenuItem, msg, provideContributions } from '@decryp7/paddock-ui/shell';
import { contributionProblems } from '@decryp7/paddock-ui/shell/testing';
import { provideMenus } from './app-menu';
import { defineSubmenu, provideSubmenus, type SubmenuDef } from './submenus';

const cmd = (id: string) => defineCommand({ id, title: msg(id, id), run: () => undefined });

function check(...providers: unknown[]): string[] {
  TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection(), ...(providers as never[])] });
  return contributionProblems(TestBed.inject(EnvironmentInjector));
}

describe('submenus', () => {
  it('a valid set has no problems, and the kind registers once however often it is provided', () => {
    const more = defineSubmenu({ id: 'sd.more', menu: 'view', title: msg('sd.more', 'More'), order: 100 });
    const inner = defineSubmenu({ id: 'sd.inner', menu: 'sd.more', title: msg('sd.inner', 'Inner'), group: 'x', order: 1 });
    expect(more.group).toBe('main');
    expect(Object.isFrozen(more)).toBe(true);
    expect(
      check(
        provideMenus({ appName: 'Demo' }),
        provideSubmenus(more),
        provideSubmenus(inner),
        provideContributions({ commands: [cmd('sd.a')], menus: [defineMenuItem({ menu: 'sd.inner', command: 'sd.a', order: 1 }), defineMenuItem({ menu: 'sd.ctx', command: 'sd.a', order: 1 })] }),
      ),
    ).toEqual([]);
  });

  it('reports each defect', () => {
    const bad = { id: 'Bad', menu: 'nowhere at all', title: { id: '', text: '' }, group: 'A', order: -1 } as unknown as SubmenuDef;
    const orphan = defineSubmenu({ id: 'sd.orphan', menu: 'sd.missing', title: msg('sd.orphan', 'O'), order: 1 });
    const a = defineSubmenu({ id: 'sd.a', menu: 'sd.b', title: msg('sd.a', 'A'), order: 1 });
    const b = defineSubmenu({ id: 'sd.b', menu: 'sd.a', title: msg('sd.b', 'B'), order: 1 });
    const deep = [1, 2, 3, 4].map((n) => defineSubmenu({ id: `sd.d${n}`, menu: n === 1 ? 'file' : `sd.d${n - 1}`, title: msg(`sd.d${n}`, `D${n}`), order: 1 }));
    const found = check(
      provideSubmenus(bad, orphan, a, b, ...deep),
      provideContributions({ commands: [cmd('sd.x')], menus: [defineMenuItem({ menu: 'file', command: 'sd.x', order: 1 }), defineMenuItem({ menu: 'help', command: 'sd.x', order: 1 })] }),
    );
    expect(found).toEqual(
      expect.arrayContaining([
        'submenu Bad: id "Bad" is not a namespaced id (pw.models.recent)',
        "submenu Bad: title has no message id (use msg('<ns>.<name>', 'Text'))",
        'submenu Bad: menu "nowhere at all" is neither a shell menu nor a submenu id',
        'submenu Bad: group "A" is malformed',
        'submenu Bad: order must be a number from 0 to 100000',
        'submenu sd.orphan is in the missing menu sd.missing',
        'submenu sd.a is inside itself (sd.a > sd.b > sd.a)',
        'submenu sd.d4 is nested 4 deep; at most 3 are allowed',
        'command sd.x is in two menus (file and help); the native menu bar shows a command once',
      ]),
    );
  });
});
