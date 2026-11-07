import { ChangeDetectionStrategy, Component } from '@angular/core';
import { AssistantBackend, PdAssistant } from '@decryp7/paddock-ui/workbench/assistant';
import { createChatBackend } from './chat-backend';

/**
 * The Playground's Chat tab: the family's assistant pane (`<pd-assistant>`: streamed answers, tool cards, approval cards,
 * Stop) over the chat engine. All logic is the engine's; this only places the pane. Pitwall is the engine itself, so the
 * pane's "needs local AI" state is not provided, and it has no "Allow actions" tick: an action is gated by the connected
 * app's own allow and by Pitwall asking on every call (ADR 0023 revision 3).
 */
@Component({
  selector: 'pw-playground-chat',
  imports: [PdAssistant],
  providers: [{ provide: AssistantBackend, useFactory: createChatBackend }],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `<pd-assistant class="assistant" actionsGate="per-call" [emptyHint]="emptyHint" [failedCause]="failedCause" />`,
  styles: `
    :host { display: flex; flex-direction: column; height: 100%; min-height: 0; }
    .assistant { flex: 1; min-height: 0; border: 1px solid var(--pd-hairline); border-radius: var(--pd-radius-card); box-shadow: var(--pd-shadow-1); overflow: hidden; }
  `,
})
export class PlaygroundChat {
  /** The empty chat: what it can do here (a connected app lends it tools). */
  protected readonly emptyHint = 'Ask a local model. Apps you connect in Settings > Connected apps lend it their tools, and every call shows here.';
  /** Pitwall is the engine: a failed answer is the model's, not a missing Pitwall. */
  protected readonly failedCause = 'Likely cause: the model stopped answering. Try again, or choose another model.';
}
