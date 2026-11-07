import { ChangeDetectionStrategy, Component } from '@angular/core';
import { AssistantBackend, PdAssistant } from '@decryp7/paddock-ui/workbench/assistant';
import { createChatBackend } from './chat-backend';

/**
 * The Playground's Chat tab: the family's assistant pane (`<pd-assistant>`: streamed answers, tool cards, approval cards,
 * Stop) over the chat engine. All logic is the engine's; this only places the pane. Pitwall is the engine itself, so the
 * pane's "needs local AI" state is not provided.
 */
@Component({
  selector: 'pw-playground-chat',
  imports: [PdAssistant],
  providers: [{ provide: AssistantBackend, useFactory: createChatBackend }],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `<pd-assistant class="assistant" />`,
  styles: `
    :host { display: flex; flex-direction: column; height: 100%; min-height: 0; }
    .assistant { flex: 1; min-height: 0; border: 1px solid var(--pd-hairline); border-radius: var(--pd-radius-card); box-shadow: var(--pd-shadow-1); overflow: hidden; }
  `,
})
export class PlaygroundChat {}
