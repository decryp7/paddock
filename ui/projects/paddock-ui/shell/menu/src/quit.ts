import { defineCommand, defineKeybinding, defineMenuItem, msg, type ContributionBatch } from '@decryp7/paddock-ui/shell';
import { QUIT_COMMAND } from '../generated/menu';

// Quit <App>, contributed once by the frames every shell app uses (rule 42): the command, Mod+Q and its place last in the app menu.
// The native menu bar (macOS) shows it from the model; with the last window closed to the tray, `paddock-menu` hands the
// click to the shell's quit on the Rust side, so Quit works with no window open too.

/** The app menu's last group (after Settings, Services and Hide): Quit with Mod+Q. `provideNativeMenu` (so `provideShellPages` and `provideWorkbench`) registers it: an app never does. To change what Quit does, provide `APP_QUIT`; never contribute a second `pd.app.quit`. */
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
