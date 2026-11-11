import { EnvironmentProviders, makeEnvironmentProviders } from '@angular/core';
import { SEARCH_EVERYTHING_CONTRIBUTIONS } from './search-everything.contributions';

export { SEARCH_EVERYTHING_COMMAND, SEARCH_EVERYTHING_KEYBINDING } from './search-everything.ids';

/** One line in `app.config.ts`: the search everything palette and its command (Mod+Shift+F). */
export function provideSearchEverything(): EnvironmentProviders {
  return makeEnvironmentProviders(SEARCH_EVERYTHING_CONTRIBUTIONS);
}
