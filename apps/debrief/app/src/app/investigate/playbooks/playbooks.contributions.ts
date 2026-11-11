import { lazyCommandFactory, type CommandStub } from '../../extensions/lazy-registration';
import { PLAYBOOKS_VIEW } from './playbooks.ids';

// The registration file of the Playbooks commands: stubs only; `playbooks.commands.ts` and the store load when the window
// is idle or a command runs. `extensions/lazy-commands.spec.ts` keeps the stubs equal to the code.

const CATEGORY = $localize`:Command palette category@@playbooks.category:View`;

export const PLAYBOOKS_COMMAND_STUBS: readonly CommandStub[] = [
  { id: 'playbooks.show', title: $localize`:@@playbooks.cmd.show:View: Playbooks`, category: CATEGORY, view: PLAYBOOKS_VIEW },
  { id: 'playbooks.new', title: $localize`:@@playbooks.cmd.new:Playbooks: New playbook`, category: CATEGORY },
];

export const PLAYBOOKS_COMMANDS_CONTRIBUTIONS = lazyCommandFactory(() => import('./playbooks.commands').then((m) => m.playbooksCommandsFactory), PLAYBOOKS_COMMAND_STUBS);
