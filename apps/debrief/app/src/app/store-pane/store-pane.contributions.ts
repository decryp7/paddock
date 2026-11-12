import { lazyCommands, type CommandStub } from '../extensions/lazy-registration';

// The registration file of View ▸ Store's commands: stubs only, so the Store pane's store (and its SQL console state)
// loads when the window is idle or a command runs (debrief-1-2-api.md §14.3).

const CATEGORY = $localize`:Command palette category@@store.category:View`;

export const STORE_PANE_COMMAND_STUBS: readonly CommandStub[] = [
  { id: 'store.show', title: $localize`:@@store.cmd.show:Open Store`, category: CATEGORY },
  { id: 'store.sql', title: $localize`:@@store.cmd.sql:Open SQL console`, category: CATEGORY },
];

export const STORE_PANE_COMMANDS_CONTRIBUTIONS = lazyCommands(() => import('./store-pane.commands').then((m) => m.StorePaneCommands), STORE_PANE_COMMAND_STUBS);
