import { EnvironmentProviders, makeEnvironmentProviders } from '@angular/core';
import { defineCommand, defineKeybinding, defineMenuItem, msg } from '@decryp7/paddock-ui/shell';
import { COMMANDS_PREFIX, provideLiveContributions, WORKBENCH_PORTS } from '@decryp7/paddock-ui/workbench';
import { PALETTE_COMMON_COMMANDS, PaletteStore } from './palette.store';

/** Command ids of the palette. */
export const PALETTE_COMMANDS = { commands: 'pd.wb.palette.commands', quickOpen: 'pd.wb.palette.quick-open' } as const;

export interface PaletteOptions {
  /** Commands listed after the recent ones when the commands list opens empty (a first run has no recent ones). */
  readonly commonCommands?: readonly string[];
}

/**
 * The window's command palette (plan W5): its store and its commands, "Show all commands" (Mod+Shift+P, the `>` list)
 * and "Go to…" (Mod+P, the plain-text provider), in the View menu. The workbench frame places `<pd-command-palette />`
 * and routes `WORKBENCH_PORTS.openPalette` to the store.
 */
export function providePalette(options: PaletteOptions = {}): EnvironmentProviders {
  return makeEnvironmentProviders([
    PaletteStore,
    { provide: PALETTE_COMMON_COMMANDS, useValue: Object.freeze([...(options.commonCommands ?? [])]) },
    provideLiveContributions((injector) => ({
      commands: [
        defineCommand({
          id: PALETTE_COMMANDS.commands,
          title: msg('pd.wb.palette.cmd.commands', 'Show all commands'),
          category: msg('pd.wb.category.view', 'View'),
          run: () => injector.get(WORKBENCH_PORTS).openPalette(COMMANDS_PREFIX),
        }),
        defineCommand({
          id: PALETTE_COMMANDS.quickOpen,
          title: msg('pd.wb.palette.cmd.quick-open', 'Go to…'),
          category: msg('pd.wb.category.view', 'View'),
          run: () => injector.get(WORKBENCH_PORTS).openPalette(''),
        }),
      ],
      keybindings: [defineKeybinding({ command: PALETTE_COMMANDS.commands, key: 'Mod+Shift+P' }), defineKeybinding({ command: PALETTE_COMMANDS.quickOpen, key: 'Mod+P' })],
      menus: [
        defineMenuItem({ menu: 'view', command: PALETTE_COMMANDS.commands, group: 'palette', order: 100 }),
        defineMenuItem({ menu: 'view', command: PALETTE_COMMANDS.quickOpen, group: 'palette', order: 110 }),
      ],
    })),
  ]);
}
