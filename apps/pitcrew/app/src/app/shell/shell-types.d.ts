import type { AppCommand } from './generated/app-commands';

// Pitcrew's command names checked at compile time: a dialog contract naming a command the table lacks does not compile.
declare module '@decryp7/paddock-ui/shell' {
  interface ShellTypes {
    appCommand: AppCommand;
  }
}
