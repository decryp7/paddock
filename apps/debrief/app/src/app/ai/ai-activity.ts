import { DebriefActivityItem } from '../extensions/tokens';
import { AiStatusStore } from './ai-status.store';

/** The AI assistant button (opens the chat `view`) once Pitwall serves a model; before that, a button that opens Settings ▸ AI to connect Pitwall. */
export function aiActivityItems(status: InstanceType<typeof AiStatusStore>, view: string): DebriefActivityItem[] {
  return [
    { id: 'ai', label: $localize`:@@ai.activity.chat:AI assistant`, icon: 'ai', order: 700, when: () => status.hasModel(), run: { view } },
    { id: 'ai-setup', label: $localize`:@@ai.activity.setup:Connect the AI assistant to Pitwall`, icon: 'ai', order: 700, when: () => status.needsSetup(), run: { command: 'ai.setup' } },
  ];
}
