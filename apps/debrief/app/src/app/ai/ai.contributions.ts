import { lazyCommandFactory, type CommandStub } from '../extensions/lazy-registration';
import { AI_CHAT_VIEW } from './ai.ids';

// The registration file of the AI commands: stubs only; `ai.commands.ts` and the AI launcher and stores load when the
// window is idle or a command runs. `extensions/lazy-commands.spec.ts` keeps the stubs equal to the code.

const CATEGORY = $localize`:@@ai.provide.category:AI`;

export const AI_COMMAND_STUBS: readonly CommandStub[] = [
  { id: 'ai.chat.show', title: $localize`:@@ai.provide.cmd.chat:AI: Open the assistant chat`, category: CATEGORY, view: AI_CHAT_VIEW },
  { id: 'ai.setup', title: $localize`:@@ai.provide.cmd.setup:AI: Connect to Pitwall…`, category: CATEGORY },
  { id: 'ai.image.open', title: $localize`:@@ai.vision.cmd.open:AI: Open an image…`, category: CATEGORY },
  { id: 'ai.image.related', title: $localize`:@@ai.vision.cmd.related:AI: Find logs related to an image…`, category: CATEGORY },
  { id: 'ai.explain', title: $localize`:@@ai.provide.cmd.explain:AI: Explain the selected event`, category: CATEGORY },
  { id: 'ai.summarize', title: $localize`:@@ai.provide.cmd.summarize:AI: Summarize this range`, category: CATEGORY },
  { id: 'ai.query', title: $localize`:@@ai.provide.cmd.query:AI: Ask AI to write a query…`, category: CATEGORY },
  { id: 'ai.rootCauses', title: $localize`:@@ai.provide.cmd.rootCauses:AI: Find likely root causes`, category: CATEGORY },
  { id: 'ai.propose.pipeline', title: $localize`:@@ai.provide.cmd.pipeline:AI: Propose pipeline steps…`, category: CATEGORY },
  { id: 'ai.propose.markers', title: $localize`:@@ai.provide.cmd.markers:AI: Propose markers…`, category: CATEGORY },
  { id: 'ai.propose.extractors', title: $localize`:@@ai.provide.cmd.extractors:AI: Propose extractors…`, category: CATEGORY },
];

export const AI_COMMANDS_CONTRIBUTIONS = lazyCommandFactory(() => import('./ai.commands').then((m) => m.aiCommandsFactory), AI_COMMAND_STUBS);
