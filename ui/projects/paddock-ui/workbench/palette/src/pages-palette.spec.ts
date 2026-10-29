import { TestBed } from '@angular/core/testing';
import { provideRouter, Router } from '@angular/router';
import { CommandService, definePage, msg, ShellContextHost } from '@decryp7/paddock-ui/shell';
import { KEY_PLATFORM } from '@decryp7/paddock-ui/shell/menu';
import { provideShellTesting, settle } from '@decryp7/paddock-ui/shell/testing';
import { PaletteStore } from './palette.store';
import { PALETTE_RECENT_STORAGE } from './recent';
import { PALETTE_COMMANDS } from './provide-palette';
import { providePagesPalette } from './pages-palette';

interface Ctx {
  readonly hasBackup: boolean;
}
const load = () => Promise.resolve(class {});

function setup(hasBackup = false) {
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
        providers: [{ provide: KEY_PLATFORM, useValue: 'other' }, { provide: PALETTE_RECENT_STORAGE, useValue: null }, providePagesPalette()],
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
