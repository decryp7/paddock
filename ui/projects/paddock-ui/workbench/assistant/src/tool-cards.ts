import { ChangeDetectionStrategy, Component, inject, InjectionToken, input, type EnvironmentProviders, makeEnvironmentProviders, type Type } from '@angular/core';
import { PdGlyph } from '@decryp7/paddock-ui/icons';
import type { ToolCard } from './chat-model';
import type { CitationOpened } from './rich-text';

/**
 * How an app renders the result of one of its tools (plan 2.2: Debrief's tool cards stay in Debrief as renderers). The
 * component gets the `card` input (and `(cite)` is not available: a renderer that links calls the app's own services). The
 * default card is calm and generic: the tool's name, the call and a one-line result.
 */
export interface AssistantToolRenderer {
  /** The tool's name as in `AssistantTool::spec`. */
  readonly tool: string;
  /** A standalone component with a `card` input of type `ToolCard`. */
  readonly component: Type<unknown>;
}

export const ASSISTANT_TOOL_RENDERERS = new InjectionToken<readonly AssistantToolRenderer[]>('PADDOCK_ASSISTANT_TOOL_RENDERERS', {
  providedIn: 'root',
  factory: () => [],
});

/** What the pane does when the user activates a citation (the app's choice: jump to a record, apply a query, open a page). */
export interface AssistantCitationHandler {
  open(citation: CitationOpened): void | Promise<void>;
}

export const ASSISTANT_CITATIONS = new InjectionToken<AssistantCitationHandler | null>('PADDOCK_ASSISTANT_CITATIONS', { providedIn: 'root', factory: () => null });

/** Registers renderers of tool results and, optionally, the handler of citations, for the window's assistant pane. */
export function provideAssistantRenderers(options: { readonly tools?: readonly AssistantToolRenderer[]; readonly citations?: AssistantCitationHandler }): EnvironmentProviders {
  return makeEnvironmentProviders([
    { provide: ASSISTANT_TOOL_RENDERERS, useValue: Object.freeze([...(options.tools ?? [])]) },
    { provide: ASSISTANT_CITATIONS, useValue: options.citations ?? null },
  ]);
}

/** The renderer registered for a tool, if any. */
export function rendererFor(renderers: readonly AssistantToolRenderer[], tool: string): Type<unknown> | null {
  return renderers.find((r) => r.tool === tool)?.component ?? null;
}

/** The default card of a tool call: its name, the call, the state and a one-line result. */
@Component({
  selector: 'pd-assistant-tool-card',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [PdGlyph],
  template: `
    <div class="card" [attr.data-state]="card().state" data-testid="assistant-tool-card">
      <pd-glyph class="state" [name]="icon()" [size]="14" />
      <span class="name">{{ card().name || unreadable }}</span>
      @if (card().summary) {
        <span class="call" [attr.title]="card().summary">{{ card().summary }}</span>
      }
      <span class="result" data-testid="assistant-tool-result">{{ stateText() }}</span>
    </div>
  `,
  styles: `
    :host { display: block; min-width: 0; }
    .card { display: flex; flex-wrap: wrap; align-items: baseline; gap: var(--pd-space-1) var(--pd-space-2); padding: var(--pd-space-2) var(--pd-space-3); border: 1px solid var(--pd-hairline); border-radius: var(--pd-radius-control); background: var(--pd-surface-2); color: var(--pd-text-muted); font-size: var(--pd-text-label); min-width: 0; }
    .card[data-state='failed'] { border-color: var(--pd-danger); }
    .state { align-self: center; }
    .name { font-weight: var(--pd-weight-semibold); color: var(--pd-text); }
    .call { min-width: 0; flex: 1 1 8ch; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .result { overflow-wrap: anywhere; }
  `,
})
export class PdAssistantToolCard {
  readonly card = input.required<ToolCard>();
  protected readonly unreadable = $localize`:@@pd.wb.assistant.tool.unreadable:(unreadable call)`;
  protected icon(): string {
    const s = this.card().state;
    return s === 'ok' ? 'check' : s === 'failed' ? 'alert' : s === 'awaiting' ? 'lock' : 'clock';
  }
  protected stateText(): string {
    const c = this.card();
    switch (c.state) {
      case 'running':
        return $localize`:@@pd.wb.assistant.tool.running:Working…`;
      case 'awaiting':
        return $localize`:@@pd.wb.assistant.tool.awaiting:Waiting for your decision`;
      default:
        return c.result ?? (c.state === 'ok' ? $localize`:@@pd.wb.assistant.tool.done:Done` : $localize`:@@pd.wb.assistant.tool.failed:Did not finish`);
    }
  }
}

/** Resolves the renderers once for a component. */
export function injectToolRenderers(): readonly AssistantToolRenderer[] {
  return inject(ASSISTANT_TOOL_RENDERERS);
}
