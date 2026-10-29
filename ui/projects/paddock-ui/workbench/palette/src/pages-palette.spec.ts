import { TestBed } from '@angular/core/testing';
import { provideRouter, Router } from '@angular/router';
import { CommandService, definePage, msg, ShellContextHost } from '@decryp7/paddock-ui/shell';
import { WorkbenchRegistry } from '@decryp7/paddock-ui/workbench';
import { KEY_PLATFORM } from '@decryp7/paddock-ui/shell/menu';
import { provideShellTesting, settle } from '@decryp7/paddock-ui/shell/testing';
import { PaletteStore } from './palette.store';
import { PALETTE_RECENT_STORAGE } from './recent';
import { PALETTE_COMMANDS } from './provide-palette';
import { GO_TO_PAGE_PROVIDER, providePagesPalette, type PagesPaletteOptions } from './pages-palette';
import { AppMenu, provideMenus } from '@decryp7/paddock-ui/shell/menu';

interface Ctx {
  readonly hasBackup: boolean;
}
const load = () => Promise.resolve(class {});

function setup(hasBackup = false, options: PagesPaletteOptions = {}) {
  TestBed.resetTestingModule();
  TestBed.configureTestingModule({
    providers: [
      provideRouter([]),
      ...provideShellTesting<Ctx>({
        context: () => Promise.resolve(() => ({ hasBackup })),
        contributions: [
          {
            pages: [
              definePage<Ctx>({ id: 'xx.dashboard', title: msg('xx.dashboard', 'Dashboard'), icon: 'xx:a', order: 100, load }),
              definePage<Ctx>({ id: 'xx.processes', title: msg('xx.processes', 'Processes'), icon: 'xx:b', order: 200, load }),
              definePage<Ctx>({ id: 'xx.backup', title: msg('xx.backup', 'Backup health'), icon: 'xx:c', order: 300, when: (ctx) => ctx.hasBackup, load }),
            ],
          },
        ],
        providers: [provideMenus({ appName: 'Test' }), { provide: KEY_PLATFORM, useValue: 'other' }, { provide: PALETTE_RECENT_STORAGE, useValue: null }, providePagesPalette(options)],
      }),
    ],
  });
  const navigate = vi.spyOn(TestBed.inject(Router), 'navigateByUrl').mockResolvedValue(true);
  return { store: TestBed.inject(PaletteStore), navigate };
}

describe('providePagesPalette', () => {
  it('opens "Go to" with Mod+P and lists every page offered, in sidebar order', async () => {
    const { store } = setup();
    await TestBed.inject(ShellContextHost).load();
    expect(await TestBed.inject(CommandService).run(PALETTE_COMMANDS.quickOpen)).toBe(true);
    await settle();
    expect(store.isOpen()).toBe(true);
    expect(store.source().kind).toBe('provider');
    expect(store.rows().map((r) => r.label)).toEqual(['Dashboard', 'Processes']);
  });

  it('offers a page whose condition holds once the context says so', async () => {
    const { store } = setup(true);
    await TestBed.inject(ShellContextHost).load();
    store.open('');
    await settle();
    expect(store.rows().map((r) => r.label)).toEqual(['Dashboard', 'Processes', 'Backup health']);
  });

  it('filters by what was typed and goes to the page on accept', async () => {
    const { store, navigate } = setup();
    await TestBed.inject(ShellContextHost).load();
    store.open('');
    store.setQuery('proc');
    await settle();
    expect(store.rows().map((r) => r.label)).toEqual(['Processes']);
    expect(await store.accept(0)).toBe(true);
    expect(navigate).toHaveBeenCalledWith('/processes');
    expect(store.isOpen()).toBe(false);
  });

  it('says nothing matches instead of listing every page', async () => {
    const { store } = setup();
    await TestBed.inject(ShellContextHost).load();
    store.open('');
    store.setQuery('zzzz');
    await settle();
    expect(store.rows()).toEqual([]);
  });

  it('hands the page on accept to the main region, also when it is the page shown', async () => {
    const { store } = setup();
    const main = document.createElement('main');
    main.id = 'pd-main';
    main.tabIndex = -1;
    document.body.append(main);
    await TestBed.inject(ShellContextHost).load();
    store.open('');
    await settle();
    expect(await store.accept(0)).toBe(true);
    expect(document.activeElement).toBe(main);
    main.remove();
  });

  it('answers no more rows than the request’s limit', async () => {
    setup();
    await TestBed.inject(ShellContextHost).load();
    const provider = TestBed.inject(WorkbenchRegistry).paletteProviders.find((p) => p.id === GO_TO_PAGE_PROVIDER)!;
    const ctx = await TestBed.inject(ShellContextHost).load();
    const answer = await provider.search('', ctx, { limit: 1, signal: new AbortController().signal });
    expect(answer.map((i) => i.label)).toEqual(['Dashboard']);
  });

  it('puts the pages behind a prefix, or leaves them out', async () => {
    const prefixed = setup(false, { goToPages: { prefix: '#' } });
    await TestBed.inject(ShellContextHost).load();
    prefixed.store.open('#proc');
    await settle();
    expect(prefixed.store.rows().map((r) => r.label)).toEqual(['Processes']);
    prefixed.store.open('');
    expect(prefixed.store.source()).toEqual({ kind: 'commands' });

    const none = setup(false, { goToPages: false });
    await TestBed.inject(ShellContextHost).load();
    none.store.open('proc');
    expect(none.store.source()).toEqual({ kind: 'commands' });
    expect(none.store.prefixes().map((p) => p.prefix)).toEqual(['>']);
  });

  it('lists the palette in the View menu, so the native menu bar carries it with its keys', async () => {
    setup();
    await TestBed.inject(ShellContextHost).load();
    await settle();
    const view = TestBed.inject(AppMenu).menus().find((m) => m.id === 'view');
    const labels = (view?.items ?? []).flatMap((n) => (n.kind === 'item' ? [{ label: n.label, key: n.keybinding }] : []));
    expect(labels).toEqual([
      { label: 'Show all commands', key: 'Mod+Shift+P' },
      { label: 'Go to…', key: 'Mod+P' },
    ]);
  });

  it('opens the commands list with Mod+Shift+P', async () => {
    const { store } = setup();
    await TestBed.inject(ShellContextHost).load();
    await TestBed.inject(CommandService).run(PALETTE_COMMANDS.commands);
    await settle();
    expect(store.isOpen()).toBe(true);
    expect(store.source()).toEqual({ kind: 'commands' });
    expect(store.rows().map((r) => r.label)).toContain('Go to…');
  });
});
