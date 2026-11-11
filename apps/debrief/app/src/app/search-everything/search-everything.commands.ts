import { inject, Injectable } from '@angular/core';
import { DebriefCommand } from '../extensions/tokens';
import { SEARCH_EVERYTHING_COMMAND, SEARCH_EVERYTHING_KEYBINDING } from './search-everything.ids';
import { SearchEverythingLauncher } from './search-everything-launcher';

export function searchEverythingCommands(launcher: SearchEverythingLauncher): DebriefCommand[] {
  return [
    {
      id: SEARCH_EVERYTHING_COMMAND,
      title: $localize`:@@search.cmd.open:Search everything…`,
      category: $localize`:Command palette category|@@search.category:Search`,
      keybinding: SEARCH_EVERYTHING_KEYBINDING,
      run: () => launcher.open(),
    },
  ];
}

/** The search everything command, loaded with the launcher (and the CDK dialog) on first use. */
@Injectable({ providedIn: 'root' })
export class SearchEverythingCommands {
  readonly commands = searchEverythingCommands(inject(SearchEverythingLauncher));
}
