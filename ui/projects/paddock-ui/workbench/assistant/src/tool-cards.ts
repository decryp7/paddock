import { ChangeDetectionStrategy, Component, inject, InjectionToken, input, type EnvironmentProviders, makeEnvironmentProviders, type Type } from '@angular/core';
import { PdGlyph, PdStatusGlyph, type PdStatus } from '@decryp7/paddock-ui/icons';
import { hasValue, type ToolCard } from './chat-model';
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
  imports: [PdGlyph, PdStatusGlyph],
  template: `
    <div class="card" [attr.data-state]="declined() ? 'declined' : stopped() ? 'stopped' : card().state" data-testid="assistant-tool-card">
      @if (card().state === 'running') {
        <span class="spin" aria-hidden="true" data-testid="assistant-tool-spinner"></span>
      } @else if (status(); as st) {
        <pd-status-glyph class="state" [status]="st" [size]="14" />
      } @else {
        <pd-glyph class="state" [name]="icon()" [size]="14" />
      }
      <span class="name">{{ card().title || card().name || unreadable }}</span>
      @if (card().summary) {
        <span class="call" [attr.title]="card().summary">{{ card().summary }}</span>
      }
      <span class="result" [class.muted]="declined() || stopped()" data-testid="assistant-tool-result">{{ declined() ? declinedText : stopped() ? stoppedText : stateText() }}</span>
      @if (approvalText(); as how) {
        <span class="sep" aria-hidden="true">·</span><span class="how" data-testid="assistant-tool-approval">{{ how }}</span>
      }
      @if (hasDetails()) {
        <details class="more" data-testid="assistant-tool-details">
          <summary><pd-glyph class="chev" name="chevron-right" [size]="14" /> {{ detailsLabel }}</summary>
          <dl>
            @if (hasArguments()) {
              <dt>{{ argumentsLabel }}</dt>
              <dd><pre data-testid="assistant-tool-args">{{ argumentsText() }}</pre></dd>
            }
            @if (card().output) {
              <dt>{{ resultLabel }}</dt>
              <dd><pre data-testid="assistant-tool-output">{{ card().output }}</pre></dd>
            }
          </dl>
        </details>
      }
    </div>
  `,
  styles: `
    :host { display: block; min-width: 0; }
    .card { display: flex; flex-wrap: wrap; align-items: baseline; gap: var(--pd-space-1) var(--pd-space-2); padding: var(--pd-space-2) var(--pd-space-3); border: 1px solid var(--pd-hairline); border-radius: var(--pd-radius-control); background: var(--pd-surface-2); color: var(--pd-text-muted); font-size: var(--pd-text-label); min-width: 0; }
    .card[data-state='failed'] { border-color: var(--pd-danger); }
    .state { align-self: center; }
    /* A 14 px spinner like the status bar's: a faint track with a quarter arc (static at 45 degrees without motion). */
    .spin { align-self: center; flex: none; box-sizing: border-box; width: 14px; height: 14px; border-radius: 50%; border: 2px solid color-mix(in srgb, var(--pd-text-muted) 35%, transparent); border-top-color: var(--pd-text); transform: rotate(45deg); animation: pd-tool-spin 0.9s linear infinite; }
    @keyframes pd-tool-spin { to { transform: rotate(405deg); } }
    @media (prefers-reduced-motion: reduce) { .spin { animation: none; } }
    .name { font-weight: var(--pd-weight-semibold); color: var(--pd-text); }
    .call { min-width: 0; flex: 1 1 8ch; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .result { overflow-wrap: anywhere; }
    .result.muted { color: var(--pd-text-muted); }
    .how, .sep { color: var(--pd-text-muted); }
    .more { flex: 1 0 100%; min-width: 0; }
    .more summary { cursor: pointer; color: var(--pd-text-muted); list-style: none; display: inline-flex; align-items: center; gap: var(--pd-space-1); }
    .more summary::-webkit-details-marker { display: none; }
    .more summary:focus-visible { outline: 2px solid var(--pd-focus-ring); border-radius: var(--pd-radius-control); }
    .chev { transition: transform var(--pd-duration-fast, 120ms); }
    .more[open] .chev { transform: rotate(90deg); }
    .more dl { margin: var(--pd-space-1) 0 0; display: grid; grid-template-columns: auto 1fr; gap: var(--pd-space-1) var(--pd-space-2); }
    .more dt { color: var(--pd-text-muted); }
    .more dd { margin: 0; min-width: 0; }
    .more pre { margin: 0; max-height: 160px; overflow: auto; font-family: var(--pd-font-mono); white-space: pre-wrap; overflow-wrap: anywhere; color: var(--pd-text); }
  `,
})
export class PdAssistantToolCard {
  readonly card = input.required<ToolCard>();
  protected readonly unreadable = $localize`:@@pd.wb.assistant.tool.unreadable:(unreadable call)`;
  protected readonly declinedText = $localize`:@@pd.wb.assistant.tool.declined:Declined`;
  protected readonly stoppedText = $localize`:@@pd.wb.assistant.tool.stopped:Stopped`;
  protected readonly detailsLabel = $localize`:@@pd.wb.assistant.tool.details:Details`;
  protected readonly argumentsLabel = $localize`:@@pd.wb.assistant.tool.arguments:Arguments`;
  protected readonly resultLabel = $localize`:@@pd.wb.assistant.tool.output:Result`;
  protected hasArguments(): boolean {
    return hasValue(this.card().input);
  }
  /** The arguments as sent, in full (the box scrolls). */
  protected argumentsText(): string {
    return JSON.stringify(this.card().input, null, 2) ?? '';
  }
  /** A call that ran can be opened to see what it was sent and what it answered. */
  protected hasDetails(): boolean {
    const c = this.card();
    return (c.state === 'ok' || c.state === 'failed') && (this.hasArguments() || !!c.output);
  }
  /** The user stopped the turn while the call ran: neutral too. */
  protected stopped(): boolean {
    return this.card().stopped === true && this.card().state === 'failed';
  }
  /** The user said no: a neutral card, not a failure. */
  protected declined(): boolean {
    return this.card().declined === true && this.card().state === 'failed';
  }
  /** The status the card stands for (a result or a real failure), drawn from the shared map; null for the user's own choices and a wait. */
  protected status(): PdStatus | null {
    if (this.declined() || this.stopped()) return null;
    const s = this.card().state;
    return s === 'ok' ? 'success' : s === 'failed' ? 'error' : null;
  }
  /** The glyph of a card that is not a status: declined (×), stopped, waiting for a decision. */
  protected icon(): string {
    if (this.declined()) return 'close';
    if (this.stopped()) return 'stop';
    return this.card().state === 'awaiting' ? 'lock' : 'clock';
  }
  /** How the call that ran was approved (none when the policy did not ask: the app's default for read-only tools). */
  protected approvalText(): string | null {
    switch (this.card().approval) {
      case 'once':
        return $localize`:@@pd.wb.assistant.tool.approval.once:Allowed once`;
      case 'always':
        return $localize`:@@pd.wb.assistant.tool.approval.always:Always allowed`;
      case 'remembered':
        return $localize`:@@pd.wb.assistant.tool.approval.remembered:Allowed earlier`;
      default:
        return null;
    }
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
