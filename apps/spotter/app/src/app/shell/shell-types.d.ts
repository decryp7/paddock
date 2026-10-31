import type { AppCommand } from '../generated/app-commands';

// Spotter's command names checked at compile time: a dialog contract naming a command the table lacks does not compile.
declare module '@decryp7/paddock-ui/shell' {
  interface ShellTypes {
    appCommand: AppCommand;
  }
}
