import type { Provider } from '@angular/core';
import { MENU_MODE } from '@decryp7/paddock-ui/shell/menu';

/**
 * The menu mode of a window opened in a browser (no Tauri): `?menu=native` shows the macOS desktop app's native menu bar, so the
 * pictures of the app have no in-window menu and the shell leaves room for the window buttons. In the desktop app (`inTauri`) or
 * without the flag nothing is provided and the shell decides.
 */
export function menuModeProviders(inTauri: boolean, search: string): Provider[] {
  return !inTauri && new URLSearchParams(search).get('menu') === 'native' ? [{ provide: MENU_MODE, useValue: 'native' as const }] : [];
}
