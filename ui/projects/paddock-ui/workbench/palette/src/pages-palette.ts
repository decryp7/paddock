import { EnvironmentProviders, inject, Injector, makeEnvironmentProviders } from '@angular/core';
import { msg } from '@decryp7/paddock-ui/shell';
import { ShellPages } from '@decryp7/paddock-ui/shell/pages';
import { defaultWorkbenchPorts, definePaletteProvider, provideLiveContributions, WORKBENCH_PORTS, type PaletteItem } from '@decryp7/paddock-ui/workbench';
import { rank } from './match';
import { PaletteStore } from './palette.store';
import { providePalette, type PaletteOptions } from './provide-palette';

/** Id of the "Go to" provider the pages palette contributes. */
export const GO_TO_PAGE_PROVIDER = 'pd.pages.go-to';

/**
 * The command palette of a window on the pages frame (Pitwall, Pitcrew, Spotter, Dyno): the same store, commands and keys
 * as the workbench's (`providePalette`: Mod+Shift+P lists the commands, Mod+P is "Go to…"), with a plain-text provider that
 * lists the pages the window offers now (`ShellPages.visible`, sidebar order) and shows the one accepted. There are no
 * views or dock here, so the workbench ports are the defaults except `openPalette`. Place `<pd-command-palette />` next to
 * `<pd-shell-frame />` in the root template.
 */
export function providePagesPalette(options: PaletteOptions = {}): EnvironmentProviders {
  return makeEnvironmentProviders([
    providePalette(options),
    {
      provide: WORKBENCH_PORTS,
      useFactory: () => {
        const injector = inject(Injector);
        return { ...defaultWorkbenchPorts(), openPalette: (prefix?: string) => injector.get(PaletteStore).open(prefix) };
      },
    },
    provideLiveContributions((injector) => ({
      paletteProviders: [
        definePaletteProvider({
          id: GO_TO_PAGE_PROVIDER,
          title: msg('pd.pages.palette.go-to', 'Pages'),
          order: 100,
          placeholder: msg('pd.pages.palette.go-to.placeholder', 'Go to a page…'),
          search: (query, _ctx, request) => {
            const pages = injector.get(ShellPages);
            const entries = pages.visible().map((p) => ({ id: p.id, title: p.title.text, icon: p.icon }));
            return Promise.resolve(
              rank(entries, query)
                .slice(0, request.limit)
                .map((r): PaletteItem => ({ id: r.item.id, label: r.item.title, icon: r.item.icon })),
            );
          },
          accept: (item) => void injector.get(ShellPages).show(item.id),
        }),
      ],
    })),
  ]);
}
