import { makeEnvironmentProviders, type EnvironmentProviders } from '@angular/core';
import { APP_QUIT, defineCommand, defineKeybinding, defineMenuItem, msg, shellCommand, type ContributionBatch, type InvokeFn } from '@decryp7/paddock-ui/shell';
import { QUIT_COMMAND } from '../generated/menu';

// Quit <App>, contributed once for every shell app (rule 42): the command, Mod+Q and its place last in the app menu.
// The native menu bar (macOS) shows it from the model; with the last window closed to the tray, `paddock-menu` hands the
// click to the shell's quit on the Rust side, so Quit works with no window open too.

/** The app menu's last group (after Settings, Services and Hide): Quit with Mod+Q. Register it beside `SETTINGS_CONTRIBUTIONS`. */
export const QUIT_CONTRIBUTIONS: ContributionBatch = {
  commands: [
    defineCommand({
      id: QUIT_COMMAND,
      title: msg('pd.app.quit', 'Quit'),
      category: msg('pd.category.app', 'Application'),
      run: (ctx) => ctx.quitApp(),
    }),
  ],
  keybindings: [defineKeybinding({ command: QUIT_COMMAND, key: 'Mod+Q' })],
  menus: [defineMenuItem({ menu: 'app', command: QUIT_COMMAND, group: 'quit', order: 1000 })],
};

/** Quit asks the shell plugin's `app_quit` (every window closes, the exit's teardown runs). `invoke` is Tauri's. */
export function provideShellQuit(invoke: InvokeFn): EnvironmentProviders {
  return makeEnvironmentProviders([{ provide: APP_QUIT, useValue: () => invoke<void>(shellCommand('app_quit')) }]);
}
