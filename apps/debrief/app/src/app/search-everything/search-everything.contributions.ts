import { lazyCommands, type CommandStub } from '../extensions/lazy-registration';
import { SEARCH_EVERYTHING_COMMAND, SEARCH_EVERYTHING_KEYBINDING } from './search-everything.ids';

// The registration file of search everything: the command's stub only. The launcher and the CDK dialog it opens load
// when the window is idle or the command runs, not before the first paint (debrief-1-2-api.md §14.3).

export const SEARCH_EVERYTHING_COMMAND_STUBS: readonly CommandStub[] = [
  {
    id: SEARCH_EVERYTHING_COMMAND,
    title: $localize`:@@search.cmd.open:Search everything…`,
    category: $localize`:Command palette category|@@search.category:Search`,
    keybinding: SEARCH_EVERYTHING_KEYBINDING,
  },
];

export const SEARCH_EVERYTHING_CONTRIBUTIONS = lazyCommands(
  () => import('./search-everything.commands').then((m) => m.SearchEverythingCommands),
  SEARCH_EVERYTHING_COMMAND_STUBS,
);
