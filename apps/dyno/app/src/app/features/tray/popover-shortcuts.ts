import type { SuTrayPlatform } from '@decryp7/paddock-ui';

/** Which keys the popover window itself binds. It binds none (the main window owns Settings and Quit shortcuts), so the footer menu shows no shortcut. */
export interface PopoverBindings {
  readonly settings: boolean;
  readonly quit: boolean;
}

/** The keys bound in the popover window. */
export const POPOVER_BINDINGS: PopoverBindings = { settings: false, quit: false };

const KEYS = { settings: ',', quit: 'Q' } as const;

/** The shortcut to show beside a menu item, or null when the popover window does not bind it: ⌘, and ⌘Q on macOS, Ctrl+, and Ctrl+Q elsewhere. */
export function shortcutLabel(platform: SuTrayPlatform, item: keyof PopoverBindings, bindings: PopoverBindings = POPOVER_BINDINGS): string | null {
  if (!bindings[item]) return null;
  return platform === 'macos' ? `⌘${KEYS[item]}` : `Ctrl+${KEYS[item]}`;
}
