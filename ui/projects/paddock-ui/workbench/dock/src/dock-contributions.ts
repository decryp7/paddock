import type { Injector } from '@angular/core';
import { defineCommand, defineKeybinding, defineMenuItem, msg, type Message } from '@decryp7/paddock-ui/shell';
import { provideLiveContributions, type WorkbenchBatch } from '@decryp7/paddock-ui/workbench';
import { Perspectives, PERSPECTIVE_KEYS } from './perspectives';
import { WorkbenchDock } from './dock.service';
import { DOCK_OPTIONS } from './dock-options';
import { dockLayoutDialogs, LAYOUT_MANAGE_DIALOG_ID, LAYOUT_SAVE_DIALOG_ID } from './layout-dialogs';
import type { LayoutManageResult } from './layout-manage-dialog';
import { WindowLayouts } from './window-layouts';

/** Command ids of the dock (`pd.wb.dock.*`). */
export const DOCK_COMMANDS = {
  popOut: 'pd.wb.dock.pop-out',
  float: 'pd.wb.dock.float',
  panelMenu: 'pd.wb.dock.panel-menu',
  autoHide: 'pd.wb.dock.auto-hide',
  autoHideAll: 'pd.wb.dock.auto-hide-all',
  pinAll: 'pd.wb.dock.pin-all',
  closeAllDocuments: 'pd.wb.dock.close-all-documents',
  switcher: 'pd.wb.dock.switcher',
  fullScreen: 'pd.wb.dock.full-screen',
  /**
   * `{ name?: string }`: saves the arrangement as a named layout (and perspective). Given a name it saves at once; without one
   * it opens the "Save layout" dialog (a modal form: the name, validated for empty, duplicate and too long).
   */
  layoutSave: 'pd.wb.dock.layout.save',
  /** Opens the "Manage layouts" tool window: apply, rename and delete (with Undo) the saved layouts. */
  layoutManage: 'pd.wb.dock.layout.manage',
  /** `{ name: string }`: applies a saved layout. */
  layoutApply: 'pd.wb.dock.layout.apply',
  /** `{ name: string }`: deletes a saved layout. */
  layoutDelete: 'pd.wb.dock.layout.delete',
  layoutReset: 'pd.wb.dock.layout.reset',
  /** Perspective `n` (1 to 9): `pd.wb.dock.perspective.<n>`. */
  perspective: (n: number): string => `pd.wb.dock.perspective.${n}`,
} as const;

const CATEGORY = msg('pd.wb.dock.category', 'Window');
const nameOf = (args: unknown): string | null => (typeof args === 'object' && args !== null && typeof (args as { name?: unknown }).name === 'string' ? (args as { name: string }).name : null);

/**
 * The dock's commands, keys and Window-menu entries (ids `pd.wb.dock.*`) for a window whose injector has a
 * `WorkbenchDock`. Every function resolves the services only when it runs (the registry is being built when this is).
 */
/** The titles of "Switch to perspective n" (literals, so the message catalog extracts each). */
const PERSPECTIVE_TITLES: readonly Message[] = [
  msg('pd.wb.dock.perspective-1', 'Switch to perspective 1'),
  msg('pd.wb.dock.perspective-2', 'Switch to perspective 2'),
  msg('pd.wb.dock.perspective-3', 'Switch to perspective 3'),
  msg('pd.wb.dock.perspective-4', 'Switch to perspective 4'),
  msg('pd.wb.dock.perspective-5', 'Switch to perspective 5'),
  msg('pd.wb.dock.perspective-6', 'Switch to perspective 6'),
  msg('pd.wb.dock.perspective-7', 'Switch to perspective 7'),
  msg('pd.wb.dock.perspective-8', 'Switch to perspective 8'),
  msg('pd.wb.dock.perspective-9', 'Switch to perspective 9'),
];

export function dockContributions(injector: Injector): WorkbenchBatch {
  const dock = () => injector.get(WorkbenchDock);
  const perspectives = () => injector.get(Perspectives);
  const layouts = () => injector.get(WindowLayouts);
  const perspectiveAt = (n: number) =>
    defineCommand({
      id: DOCK_COMMANDS.perspective(n),
      title: PERSPECTIVE_TITLES[n - 1] as Message,
      category: CATEGORY,
      when: () => !!perspectives().at(n),
      run: () => {
        void perspectives().switchToAt(n);
      },
    });
  return {
    commands: [
      defineCommand({ id: DOCK_COMMANDS.popOut, title: msg('pd.wb.dock.cmd.pop-out', 'Pop out to window'), category: CATEGORY, when: () => dock().canPopOut(dock().activePanel() ?? ''), run: () => void dock().popOutActive() }),
      defineCommand({ id: DOCK_COMMANDS.float, title: msg('pd.wb.dock.cmd.float', 'Float panel'), category: CATEGORY, when: () => dock().canFloatActive(), run: () => void dock().floatActive() }),
      defineCommand({ id: DOCK_COMMANDS.panelMenu, title: msg('pd.wb.dock.cmd.panel-menu', 'Panel menu'), category: CATEGORY, run: () => dock().openActivePanelMenu() }),
      defineCommand({ id: DOCK_COMMANDS.autoHide, title: msg('pd.wb.dock.cmd.auto-hide', 'Auto hide or pin tool window'), category: CATEGORY, run: () => dock().toggleAutoHideActive() }),
      defineCommand({ id: DOCK_COMMANDS.autoHideAll, title: msg('pd.wb.dock.cmd.auto-hide-all', 'Auto hide all tool windows'), category: CATEGORY, run: () => void dock().autoHideAll() }),
      defineCommand({ id: DOCK_COMMANDS.pinAll, title: msg('pd.wb.dock.cmd.pin-all', 'Pin all tool windows'), category: CATEGORY, when: () => dock().autoHide().items.length > 0, run: () => dock().pinAll() }),
      defineCommand({ id: DOCK_COMMANDS.closeAllDocuments, title: msg('pd.wb.dock.cmd.close-all-documents', 'Close all documents'), category: CATEGORY, run: () => dock().closeAllDocuments() }),
      defineCommand({ id: DOCK_COMMANDS.switcher, title: msg('pd.wb.dock.cmd.switcher', 'Switch panel'), category: CATEGORY, run: () => dock().switch() }),
      defineCommand({ id: DOCK_COMMANDS.fullScreen, title: msg('pd.wb.dock.cmd.full-screen', 'Full screen'), category: CATEGORY, run: () => dock().toggleFullScreen() }),
      defineCommand({
        id: DOCK_COMMANDS.layoutSave,
        title: msg('pd.wb.dock.cmd.layout-save', 'Save layout…'),
        category: CATEGORY,
        run: async (ctx, args) => {
          const given = nameOf(args);
          if (given !== null) {
            perspectives().saveCurrent(given);
            return;
          }
          await layouts().ready;
          const names = layouts().list().map((l) => l.name);
          const suggested = `${$localize`:@@pd.wb.dock.layout.default-name:Layout`} ${names.length + 1}`;
          const name = await ctx.openDialog<string>(LAYOUT_SAVE_DIALOG_ID, { names, suggested });
          if (typeof name === 'string') perspectives().saveCurrent(name);
        },
      }),
      defineCommand({
        id: DOCK_COMMANDS.layoutManage,
        title: msg('pd.wb.dock.cmd.layout-manage', 'Manage layouts…'),
        category: CATEGORY,
        run: async (ctx) => {
          const result = await ctx.openDialog<LayoutManageResult>(LAYOUT_MANAGE_DIALOG_ID, {});
          // The window edited the stored layouts from its own window: read them again before anything is applied.
          await perspectives().refresh();
          if (result?.apply) layouts().apply(result.apply);
        },
      }),
      defineCommand({ id: DOCK_COMMANDS.layoutApply, title: msg('pd.wb.dock.cmd.layout-apply', 'Apply saved layout'), category: CATEGORY, when: () => layouts().list().length > 0, run: (_ctx, args) => void dock().applyNamedLayout(nameOf(args) ?? '') }),
      defineCommand({ id: DOCK_COMMANDS.layoutDelete, title: msg('pd.wb.dock.cmd.layout-delete', 'Delete saved layout'), category: CATEGORY, when: () => layouts().list().length > 0, run: (_ctx, args) => dock().deleteNamedLayout(nameOf(args) ?? '') }),
      defineCommand({ id: DOCK_COMMANDS.layoutReset, title: msg('pd.wb.dock.cmd.layout-reset', 'Reset perspective'), category: CATEGORY, run: () => perspectives().reset() }),
      ...Array.from({ length: PERSPECTIVE_KEYS }, (_, i) => perspectiveAt(i + 1)),
    ],
    dialogs: [...dockLayoutDialogs(injector.get(DOCK_OPTIONS))],
    keybindings: [
      defineKeybinding({ command: DOCK_COMMANDS.popOut, key: 'Ctrl+Alt+O' }),
      defineKeybinding({ command: DOCK_COMMANDS.panelMenu, key: 'Alt+-' }),
      defineKeybinding({ command: DOCK_COMMANDS.autoHide, key: 'Ctrl+Alt+H' }),
      defineKeybinding({ command: DOCK_COMMANDS.fullScreen, key: 'Shift+Alt+Enter' }),
      ...Array.from({ length: PERSPECTIVE_KEYS }, (_, i) => defineKeybinding({ command: DOCK_COMMANDS.perspective(i + 1), key: `Mod+${i + 1}` })),
    ],
    menus: [
      defineMenuItem({ menu: 'window', command: DOCK_COMMANDS.popOut, group: 'dock', order: 100 }),
      defineMenuItem({ menu: 'window', command: DOCK_COMMANDS.float, group: 'dock', order: 110 }),
      defineMenuItem({ menu: 'window', command: DOCK_COMMANDS.autoHide, group: 'dock', order: 120 }),
      defineMenuItem({ menu: 'window', command: DOCK_COMMANDS.autoHideAll, group: 'dock', order: 130 }),
      defineMenuItem({ menu: 'window', command: DOCK_COMMANDS.pinAll, group: 'dock', order: 140 }),
      defineMenuItem({ menu: 'window', command: DOCK_COMMANDS.closeAllDocuments, group: 'dock', order: 150 }),
      defineMenuItem({ menu: 'window', command: DOCK_COMMANDS.switcher, group: 'dock', order: 160 }),
      defineMenuItem({ menu: 'window', command: DOCK_COMMANDS.fullScreen, group: 'dock', order: 170 }),
      defineMenuItem({ menu: 'window', command: DOCK_COMMANDS.layoutSave, group: 'layout', order: 200 }),
      defineMenuItem({ menu: 'window', command: DOCK_COMMANDS.layoutManage, group: 'layout', order: 205 }),
      defineMenuItem({ menu: 'window', command: DOCK_COMMANDS.layoutReset, group: 'layout', order: 210 }),
    ],
  };
}

/** Registers `dockContributions` through the shell's one registry. `provideWorkbenchDock` includes it. */
export const provideDockContributions = () => provideLiveContributions(dockContributions);
