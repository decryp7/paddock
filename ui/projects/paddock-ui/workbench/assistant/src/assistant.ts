import { NgComponentOutlet } from '@angular/common';
import { afterNextRender, ChangeDetectionStrategy, Component, ElementRef, effect, Injector, inject, input, linkedSignal, output, signal, viewChild } from '@angular/core';
import { PD_ICON_PATHS, PdGlyph, PdStatusGlyph } from '@decryp7/paddock-ui/icons';
import { PdNeedsAi, PitwallStore, type NextStep } from '@decryp7/paddock-ui/shell/pitwall';
import { AssistantStore, type ActionsGate } from './assistant.store';
import { chipText, conversationTitle, waitingPhase, type TurnPart, type TurnView } from './chat-model';
import type { ProviderPhase } from '../generated/assistant';
import { PdAssistantRichText, type CitationOpened } from './rich-text';
import { ASSISTANT_CITATIONS, injectToolRenderers, PdAssistantToolCard, rendererFor } from './tool-cards';

/** How close to the bottom (px) the user must be for the log to follow the streaming text. */
const SCROLL_STICK_PX = 24;

/**
 * The assistant pane (plan W6; Debrief's `chat-panel` and `chat-composer`): the conversation with streamed answers drawn as
 * text (never HTML), tool cards (an app's own renderer per tool, else a calm generic card), citations as buttons, the
 * context chips the app contributes (removable before sending), the conversation list, the approval prompt for tools that
 * write, and the shared "needs local AI" state when Pitwall is missing. Enter sends, Shift+Enter adds a line, Escape stops.
 *
 * Needs `AssistantBackend` in the window (`provideAssistant`) and, for the needs-AI state, `providePitwall`.
 */
@Component({
  selector: 'pd-assistant',
  changeDetection: ChangeDetectionStrategy.OnPush,
  providers: [AssistantStore],
  imports: [PdStatusGlyph, NgComponentOutlet, PdGlyph, PdAssistantRichText, PdAssistantToolCard, PdNeedsAi],
  template: `
    <header class="bar">
      <h2 class="title" data-testid="assistant-title" [attr.title]="store.title()">{{ store.title() }}</h2>
      <span class="spacer"></span>
      <button type="button" class="pd-btn pd-btn--ghost pd-btn--sm" data-testid="assistant-list-toggle" [attr.aria-expanded]="store.listOpen()" [attr.aria-label]="t.conversations" [attr.title]="t.conversations" (click)="store.toggleListOpen()">
        <pd-glyph name="history" [size]="14" />
      </button>
      <button type="button" class="pd-btn pd-btn--ghost pd-btn--sm" data-testid="assistant-new" [disabled]="store.busy()" [attr.aria-label]="t.newChat" [attr.title]="t.newChat" (click)="store.newConversation()">
        <pd-glyph name="plus" [size]="14" />
      </button>
    </header>

    @if (store.listOpen()) {
      <ul class="list" data-testid="assistant-list" [attr.aria-label]="t.conversations">
        @for (c of store.visibleConversations(); track c.id) {
          <li class="row" [class.active]="c.id === store.activeId()">
            <button type="button" class="open" data-testid="assistant-open" [disabled]="store.busy()" [attr.title]="heading(c.title)" (click)="store.open(c.id)">{{ heading(c.title) }}</button>
            <button type="button" class="pd-btn pd-btn--ghost pd-btn--sm del" data-testid="assistant-delete" [attr.aria-label]="t.deleteChat + ': ' + heading(c.title)" [attr.title]="t.deleteChat" (click)="deleteChat(c.id)">
              <pd-glyph name="trash" [size]="14" />
            </button>
          </li>
        } @empty {
          <li class="empty">{{ t.noConversations }}</li>
        }
      </ul>
    }

    @if (store.undoable(); as gone) {
      <div class="undo" role="status" data-testid="assistant-undo">
        <span class="undo-text">{{ deleted(heading(gone.title)) }}</span>
        <button type="button" #undoButton class="pd-btn pd-btn--ghost pd-btn--sm" data-testid="assistant-undo-button" (click)="store.undoRemove(gone.id)">{{ t.undo }}</button>
      </div>
    }

    @if (pitwall) {
      <pd-needs-ai class="needs" [feature]="t.feature" [settingsLink]="settingsLink()" (step)="step.emit($event)" (openSettings)="openSettings.emit()" />
    }

    <div class="log" role="log" aria-live="off" [attr.aria-busy]="store.busy()" data-testid="assistant-log" #log (scroll)="scrolled()">
      @if (store.isLoading() && !store.turns().length) {
        <p class="hint" data-testid="assistant-loading">{{ t.loading }}</p>
      }
      @if (store.requestError(); as problem) {
        <div class="failure" role="alert" data-testid="assistant-load-failed">
          <span>{{ problem }}</span>
          <button type="button" class="pd-btn pd-btn--sm" data-testid="assistant-retry" (click)="store.load()">{{ t.retry }}</button>
        </div>
      } @else if (!store.turns().length && store.isLoaded()) {
        <div class="empty-state" data-testid="assistant-empty">
          @if (emptyTitle(); as title) {
            <pd-glyph class="empty-icon" name="chat" [size]="22" />
            <p class="empty-title" data-testid="assistant-empty-title">{{ title }}</p>
          }
          <p class="hint">{{ emptyHint() ?? t.empty }}</p>
          @if (emptyAction(); as action) {
            <button type="button" class="pd-btn" data-testid="assistant-empty-action" (click)="emptyActionClick.emit()">{{ action }}</button>
          }
        </div>
      }
      @for (turn of store.turns(); track $index; let last = $last) {
        <article class="turn" data-testid="assistant-turn">
          <div class="user" data-testid="assistant-user">
            <p>{{ turn.user }}</p>
            @if (turn.context.length) {
              <ul class="attached" [attr.aria-label]="t.attached">
                @for (c of turn.context; track c.id) {
                  <li class="chip static">{{ c.label }}</li>
                }
              </ul>
            }
          </div>
          <div class="answer" data-testid="assistant-answer">
            @for (part of turn.parts; track $index) {
              @if (part.kind === 'text') {
                <pd-assistant-rich-text [text]="part.text" [citations]="turn.citations" (cite)="cited($event)" />
              } @else if (renderer(part); as component) {
                <ng-container *ngComponentOutlet="component; inputs: { card: part.card }" />
              } @else {
                <pd-assistant-tool-card [card]="part.card" />
              }
            }
            @if (turn.running && turn.parts.at(-1)?.kind === 'text') {
              <span class="pulse" aria-hidden="true" data-testid="assistant-streaming"><i></i><i></i><i></i></span>
            }
            @if (phaseOf(turn); as phase) {
              <p class="hint phase" role="status" data-testid="assistant-phase"><span class="pulse" aria-hidden="true"><i></i><i></i><i></i></span> {{ phaseText(phase) }}</p>
            } @else if (turn.running && !turn.parts.length) {
              <p class="hint" data-testid="assistant-thinking">{{ t.thinking }}</p>
            }
            @for (note of turn.notes; track $index) {
              <p class="note" data-testid="assistant-note">{{ note }}</p>
            }
            @if (turn.message) {
              <p class="note" data-testid="assistant-budget">{{ turn.message }}</p>
            }
            @if (turn.end === 'cancelled') {
              <p class="note" data-testid="assistant-stopped">{{ t.stopped }}</p>
            }
            @if (failedOf(turn); as err) {
              <div class="fail" role="alert" data-testid="assistant-error">
                <pd-status-glyph class="fail-icon" status="error" [size]="16" />
                <div class="fail-body">
                  <p class="fail-title">{{ t.failedTitle }}</p>
                  <p class="fail-cause" data-testid="assistant-error-cause">{{ failedCause() ?? t.failedCause }}</p>
                  @if (details() === $index) {
                    <p class="fail-details" data-testid="assistant-error-details">{{ err.message }}</p>
                  }
                  <div class="fail-actions">
                    @if (last) {
                      <button type="button" class="pd-btn" data-testid="assistant-error-retry" [disabled]="store.busy() || blocked()" (click)="store.retry()">{{ t.retryTurn }}</button>
                    }
                    <button type="button" class="pd-btn pd-btn--ghost" data-testid="assistant-error-details-toggle" [attr.aria-expanded]="details() === $index" (click)="details.set(details() === $index ? -1 : $index)">
                      {{ details() === $index ? t.hideDetails : t.showDetails }}
                    </button>
                  </div>
                </div>
              </div>
            }
          </div>
        </article>
      }
    </div>

    @if (unseen()) {
      <button type="button" class="pd-btn pd-btn--sm jump" data-testid="assistant-jump" (click)="jumpToLatest()">
        <pd-glyph name="chevron-down" [size]="14" /> {{ t.jump }}
      </button>
    }
    <p class="sr-only" role="status" aria-live="polite" data-testid="assistant-announce">{{ announcement() }}</p>

    @if (store.approval(); as ask) {
      <section class="approval" [class.danger]="ask.destructive" role="alertdialog" aria-labelledby="pd-assistant-approval" [attr.aria-describedby]="ask.destructive ? 'pd-assistant-approval-final' : null" data-testid="assistant-approval">
        <p id="pd-assistant-approval"><strong>{{ t.wantsTo }}</strong>{{ ' ' }}<span data-testid="assistant-approval-consequence">{{ ask.summary }}</span></p>
        @if (ask.destructive) {
          <p class="final" id="pd-assistant-approval-final" data-testid="assistant-approval-final"><pd-status-glyph status="warning" [size]="14" /> {{ t.cannotUndo }}</p>
        }
        <details class="details" data-testid="assistant-approval-details" [open]="ask.write">
          <summary><pd-glyph class="chev" name="chevron-right" [size]="14" /> {{ t.details }}</summary>
          <dl>
            <dt>{{ t.tool }}</dt>
            <dd data-testid="assistant-approval-tool">{{ ask.name }}</dd>
            <dt>{{ t.arguments }}</dt>
            <dd>
              @if (hasArgs(ask.input)) {
                <pre data-testid="assistant-approval-args">{{ args(ask.input) }}</pre>
              } @else {
                <span class="none" data-testid="assistant-approval-args">{{ t.noArguments }}</span>
              }
            </dd>
          </dl>
        </details>
        <div class="actions">
          <button type="button" class="pd-btn" [class.pd-btn--danger]="ask.destructive" data-testid="assistant-allow" (click)="store.approve(ask.callId, 'allowOnce')">{{ ask.remember ? t.allowOnce : allowLabel(ask.name) }}</button>
          @if (ask.remember) {
            <button type="button" class="pd-btn" data-testid="assistant-always" [attr.title]="t.alwaysHint" (click)="store.approve(ask.callId, 'alwaysAllow')">{{ t.always }}</button>
          }
          <button #declineButton type="button" class="pd-btn" [class.pd-btn--ghost]="!ask.destructive" data-testid="assistant-decline" (click)="store.approve(ask.callId, 'deny')">{{ t.decline }}</button>
        </div>
      </section>
    }

    @if (store.failure(); as failure) {
      <p class="error" role="alert" data-testid="assistant-failure">{{ failure }} <button type="button" class="pd-btn pd-btn--ghost pd-btn--sm" (click)="store.dismissFailure()">{{ t.dismiss }}</button></p>
    }

    <form class="composer" (submit)="$event.preventDefault(); submit()">
      @if (store.chips().length) {
        <ul class="chips" [attr.aria-label]="t.context" data-testid="assistant-chips">
          @for (chip of store.chips(); track chip.id) {
            <li>
              <button type="button" class="chip" data-testid="assistant-chip" [class.off]="!chip.on" [class.inert]="blocked()" [attr.aria-pressed]="chip.on" [disabled]="blocked()" [attr.title]="chip.on ? t.removeChip : t.addChip" (click)="store.toggleChip(chip.id)">
                @if (chip.icon && hasGlyph(chip.icon)) {
                  <pd-glyph [name]="chip.icon" [size]="14" />
                }
                <span class="chip-label">{{ chipText(chip.label, chip.detail) }}</span>
                <pd-glyph [name]="chip.on ? 'close' : 'plus'" [size]="14" />
              </button>
            </li>
          }
        </ul>
      }
      <textarea
        #input
        class="input"
        rows="3"
        data-testid="assistant-input"
        [attr.aria-label]="t.question"
        [disabled]="blocked()"
        [attr.placeholder]="placeholder()"
        [attr.maxlength]="store.limit()"
        (input)="store.setDraft($any($event.target).value)"
        (keydown)="key($event)"
      ></textarea>
      <div class="send-row">
        @if (store.hasWriteTools() && store.actionsGate() === 'tick') {
          <label class="allow" [attr.title]="t.allowHint">
            <input type="checkbox" data-testid="assistant-allow-actions" [checked]="store.allowWrite()" [disabled]="store.busy() || blocked()" (change)="store.setAllowWrite($any($event.target).checked)" />
            <span>{{ t.allowActions }}</span>
          </label>
        }
        <span class="spacer"></span>
        @if (store.busy()) {
          <button type="button" class="pd-btn" data-testid="assistant-stop" [disabled]="store.run() === 'stopping'" (click)="store.stop()">
            <pd-glyph name="stop" [size]="14" /> {{ t.stop }}
          </button>
        } @else {
          <button type="submit" class="pd-btn" [class.pd-btn--primary]="!blocked()" data-testid="assistant-send" [disabled]="!store.canSend() || blocked()">
            <pd-glyph name="play" [size]="14" /> {{ t.send }}
          </button>
        }
      </div>
    </form>
  `,
  styles: `
    :host { display: flex; flex-direction: column; height: 100%; min-height: 0; min-width: 0; container-type: inline-size; background: var(--pd-surface-1); color: var(--pd-text); font-size: var(--pd-text-body); }
    .bar { display: flex; align-items: center; gap: var(--pd-space-1); padding: var(--pd-space-1) var(--pd-space-2); border-bottom: 1px solid var(--pd-hairline); min-height: var(--pd-toolbar-height); }
    .title { margin: 0; font-size: inherit; font-weight: var(--pd-weight-semibold); min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .spacer { flex: 1; }
    .list { list-style: none; margin: 0; padding: var(--pd-space-1) var(--pd-space-3); max-height: 40%; overflow-y: auto; border-bottom: 1px solid var(--pd-hairline); }
    .row { display: flex; align-items: center; gap: var(--pd-space-1); border-radius: var(--pd-radius-control); }
    .row.active { background: var(--pd-accent-tint); }
    .row:hover { background: var(--pd-hover); }
    .del { color: var(--pd-danger); opacity: 0; }
    .row:hover .del, .row:focus-within .del, .del:focus-visible { opacity: 1; }
    @media (hover: none) { .del { opacity: 1; } }
    .undo { display: flex; align-items: center; gap: var(--pd-space-2); padding: var(--pd-space-1) var(--pd-space-3); border-bottom: 1px solid var(--pd-hairline); background: var(--pd-surface-2); }
    .undo-text { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .jump { align-self: center; margin-top: calc(-1 * var(--pd-space-8, 32px)); box-shadow: var(--pd-shadow-1, none); }
    .sr-only { position: absolute; width: 1px; height: 1px; margin: -1px; padding: 0; overflow: hidden; clip-path: inset(50%); white-space: nowrap; border: 0; }
    .details { margin: 0 0 var(--pd-space-2); font-size: var(--pd-text-label); }
    .details summary { cursor: pointer; color: var(--pd-text-muted); list-style: none; display: inline-flex; align-items: center; gap: var(--pd-space-1); }
    .details summary::-webkit-details-marker { display: none; }
    .chev { transition: transform var(--pd-duration-fast, 120ms); }
    .details[open] .chev { transform: rotate(90deg); }
    .none { color: var(--pd-text-muted); }
    .details dl { margin: var(--pd-space-1) 0 0; display: grid; grid-template-columns: auto 1fr; gap: var(--pd-space-1) var(--pd-space-2); }
    .details dt { color: var(--pd-text-muted); }
    .details dd { margin: 0; min-width: 0; }
    .details pre { margin: 0; max-height: 120px; overflow: auto; font-family: var(--pd-font-mono); white-space: pre-wrap; overflow-wrap: anywhere; }
    .open { all: unset; flex: 1; min-width: 0; padding: var(--pd-space-1) var(--pd-space-2); cursor: pointer; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .open:focus-visible { outline: 2px solid var(--pd-focus-ring); border-radius: var(--pd-radius-control); }
    .empty, .hint { color: var(--pd-text-muted); padding: var(--pd-space-2); margin: 0; }
    .empty-state { display: flex; flex-direction: column; align-items: center; gap: var(--pd-space-1); margin: auto; max-width: 480px; text-align: center; }
    .empty-icon { color: var(--pd-accent); }
    .empty-title { margin: 0; font-weight: var(--pd-weight-semibold); }
    .phase { display: flex; align-items: center; gap: var(--pd-space-2); }
    .needs { margin: var(--pd-space-2); }
    .log { flex: 1; min-height: 0; overflow-y: auto; padding: var(--pd-space-3); display: flex; flex-direction: column; gap: var(--pd-space-4); }
    .turn { display: flex; flex-direction: column; gap: var(--pd-space-2); min-width: 0; }
    .user { align-self: flex-end; max-width: 90%; background: var(--pd-accent-tint); border-radius: var(--pd-radius-card); padding: var(--pd-space-2) var(--pd-space-3); overflow-wrap: anywhere; }
    .user p { margin: 0; white-space: pre-wrap; }
    .attached, .chips { list-style: none; margin: var(--pd-space-1) 0 0; padding: 0; display: flex; flex-wrap: wrap; gap: var(--pd-space-1); }
    .answer { display: flex; flex-direction: column; gap: var(--pd-space-2); min-width: 0; }
    .note { margin: 0; color: var(--pd-text-muted); font-size: var(--pd-text-label); }
    .error { margin: 0; color: var(--pd-danger); overflow-wrap: anywhere; padding: 0 var(--pd-space-3); }
    .pulse { display: inline-flex; gap: 3px; align-items: center; height: 14px; }
    .pulse i { width: 5px; height: 5px; border-radius: 50%; background: var(--pd-text-muted); animation: pd-pulse 1.2s ease-in-out infinite; }
    .pulse i:nth-child(2) { animation-delay: 0.2s; }
    .pulse i:nth-child(3) { animation-delay: 0.4s; }
    @keyframes pd-pulse { 0%, 80%, 100% { opacity: 0.25; } 40% { opacity: 1; } }
    @media (prefers-reduced-motion: reduce) { .pulse i { animation: none; opacity: 0.6; } .chev { transition: none; } }
    .fail { display: flex; gap: var(--pd-space-2); align-items: flex-start; min-width: 0; }
    .fail-icon { color: var(--pd-danger); flex: none; margin-top: 2px; }
    .fail-body { min-width: 0; display: flex; flex-direction: column; gap: var(--pd-space-1); }
    .fail p { margin: 0; overflow-wrap: anywhere; }
    .fail-title { color: var(--pd-danger); font-weight: var(--pd-weight-semibold); }
    .fail-cause, .fail-details { color: var(--pd-text-muted); font-size: var(--pd-text-label); }
    .fail-actions { display: flex; gap: var(--pd-space-2); flex-wrap: wrap; margin-top: var(--pd-space-1); }
    .chip.inert { opacity: 0.5; cursor: default; }
    .failure { display: flex; gap: var(--pd-space-2); align-items: center; color: var(--pd-danger); }
    .approval { margin: 0 var(--pd-space-3); padding: var(--pd-space-3); border: 1px solid var(--pd-warning); background: var(--pd-warning-tint); border-radius: var(--pd-radius-card); }
    .approval p { margin: 0 0 var(--pd-space-2); overflow-wrap: anywhere; }
    .approval.danger { border-color: var(--pd-danger); background: var(--pd-danger-tint); }
    .approval .final { display: flex; gap: var(--pd-space-1); align-items: center; color: var(--pd-danger); }
    .actions { display: flex; gap: var(--pd-space-2); flex-wrap: wrap; }
    .composer { display: flex; flex-direction: column; gap: var(--pd-space-2); padding: var(--pd-space-2) var(--pd-space-3) var(--pd-space-3); border-top: 1px solid var(--pd-hairline); }
    .chip { display: inline-flex; align-items: center; gap: var(--pd-space-1); max-width: 100%; min-height: 24px; padding: 0 var(--pd-space-2); border: 1px solid var(--pd-border-control); border-radius: var(--pd-radius-pill); background: var(--pd-surface-2); color: var(--pd-text); font: inherit; font-size: var(--pd-text-label); cursor: pointer; }
    .chip.static { cursor: default; min-height: 0; }
    .chip.off { opacity: 0.6; text-decoration: line-through; }
    .chip:focus-visible { outline: 2px solid var(--pd-focus-ring); outline-offset: 1px; }
    .chip-label { font-weight: var(--pd-weight-medium); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; min-width: 0; }
    .input { width: 100%; box-sizing: border-box; resize: none; font: inherit; color: var(--pd-text); background: var(--pd-surface-2); border: 1px solid var(--pd-border-control); border-radius: var(--pd-radius-input); padding: var(--pd-space-2); }
    .input:focus-visible { outline: 2px solid var(--pd-focus-ring); outline-offset: 0; }
    .send-row { display: flex; align-items: center; gap: var(--pd-space-2); flex-wrap: wrap; }
    .allow { display: inline-flex; align-items: center; gap: var(--pd-space-1); font-size: var(--pd-text-label); color: var(--pd-text-muted); }
    @container (max-width: 360px) { .log { padding: var(--pd-space-2); } .user { max-width: 100%; } }
  `,
})
export class PdAssistant {
  protected readonly store = inject(AssistantStore);
  protected readonly pitwall = inject(PitwallStore, { optional: true });
  private readonly renderers = injectToolRenderers();
  private readonly citations = inject(ASSISTANT_CITATIONS);
  private readonly log = viewChild<ElementRef<HTMLElement>>('log');
  private readonly inputEl = viewChild<ElementRef<HTMLTextAreaElement>>('input');
  private readonly undoButton = viewChild<ElementRef<HTMLButtonElement>>('undoButton');
  private readonly declineButton = viewChild<ElementRef<HTMLButtonElement>>('declineButton');
  private readonly injector = inject(Injector);
  /** The turn whose error details are open (-1: none). */
  protected readonly details = signal(-1);

  /** Offer "Open AI settings" in the needs-AI state (the app's AI page). */
  readonly settingsLink = input(false);
  /** How tools that write are gated: the per-message "Allow actions" tick (default), or the engine asking on every call. */
  readonly actionsGate = input<ActionsGate>('tick');
  /** The empty conversation's hint in the host app's words (default: the workbench's "tools of this app"). */
  readonly emptyHint = input<string | null>(null);
  /** A title over the empty conversation's hint, with the assistant's icon (none: the hint alone). */
  readonly emptyTitle = input<string | null>(null);
  /** The empty conversation's next step as a button (none: no button); its click is `emptyActionClick`. */
  readonly emptyAction = input<string | null>(null);
  /** The empty state's next step was chosen. */
  readonly emptyActionClick = output<void>();
  /** The likely cause under a failed answer in the host app's words (default: check that Pitwall is running; an app that
   *  is the engine itself says something else). */
  readonly failedCause = input<string | null>(null);
  /** A step only the app can run (open or install Pitwall, update, change permissions), from the needs-AI state. */
  readonly step = output<NextStep>();
  /** The user chose "Open AI settings". */
  readonly openSettings = output<void>();
  /** A citation was activated and the app has no handler registered. */
  readonly citation = output<CitationOpened>();

  protected readonly t = {
    conversations: $localize`:@@pd.wb.assistant.conversations:Conversations`,
    newChat: $localize`:@@pd.wb.assistant.new:New conversation`,
    deleteChat: $localize`:@@pd.wb.assistant.delete:Delete conversation`,
    noConversations: $localize`:@@pd.wb.assistant.no-conversations:No conversations yet.`,
    feature: $localize`:@@pd.wb.assistant.feature:The assistant`,
    loading: $localize`:@@pd.wb.assistant.loading:Loading…`,
    retry: $localize`:@@pd.wb.assistant.retry:Try again`,
    empty: $localize`:@@pd.wb.assistant.empty:Ask a question. The assistant can look things up with the tools of this app.`,
    attached: $localize`:@@pd.wb.assistant.attached:Attached context`,
    thinking: $localize`:@@pd.wb.assistant.thinking:Thinking…`,
    stopped: $localize`:@@pd.wb.assistant.stopped:Stopped.`,
    wantsTo: $localize`:@@pd.wb.assistant.wants-to:The assistant wants to:`,
    allow: $localize`:@@pd.wb.assistant.approve.allow:Allow`,
    allowOnce: $localize`:@@pd.wb.assistant.approve.once:Allow once`,
    always: $localize`:@@pd.wb.assistant.approve.always:Always allow`,
    alwaysHint: $localize`:@@pd.wb.assistant.approve.always-hint:Run this tool from now on without asking. Every call still shows in the chat.`,
    queued: $localize`:@@pd.wb.assistant.phase.queued:Waiting for the model…`,
    loadingModel: $localize`:@@pd.wb.assistant.phase.loading:Loading the model…`,
    decline: $localize`:@@pd.wb.assistant.approve.decline:Decline`,
    dismiss: $localize`:@@pd.wb.assistant.dismiss:Dismiss`,
    context: $localize`:@@pd.wb.assistant.context:Context for this question`,
    removeChip: $localize`:@@pd.wb.assistant.chip.remove:Do not send this context`,
    addChip: $localize`:@@pd.wb.assistant.chip.add:Send this context`,
    question: $localize`:@@pd.wb.assistant.question:Your question`,
    placeholder: $localize`:@@pd.wb.assistant.placeholder:Ask the assistant (Enter sends, Shift+Enter adds a line)`,
    allowActions: $localize`:@@pd.wb.assistant.allow-actions:Allow actions`,
    allowHint: $localize`:@@pd.wb.assistant.allow-hint:Lets the assistant propose changes. Each one still asks for your approval.`,
    stop: $localize`:@@pd.wb.assistant.stop:Stop`,
    send: $localize`:@@pd.wb.assistant.send:Send`,
    undo: $localize`:@@pd.wb.assistant.undo:Undo`,
    jump: $localize`:@@pd.wb.assistant.jump:Jump to latest`,
    details: $localize`:@@pd.wb.assistant.approve.details:Details`,
    tool: $localize`:@@pd.wb.assistant.approve.tool:Tool`,
    arguments: $localize`:@@pd.wb.assistant.approve.arguments:Arguments`,
    noArguments: $localize`:@@pd.wb.assistant.approve.no-arguments:No arguments`,
    cannotUndo: $localize`:@@pd.wb.assistant.approve.cannot-undo:This cannot be undone.`,
    failedTitle: $localize`:@@pd.wb.assistant.turn-failed.title:The answer did not finish.`,
    failedCause: $localize`:@@pd.wb.assistant.turn-failed.cause:Likely cause: the local model stopped answering. Check that Pitwall is running, then try again.`,
    retryTurn: $localize`:@@pd.wb.assistant.turn-failed.retry:Retry`,
    showDetails: $localize`:@@pd.wb.assistant.turn-failed.show:Show details`,
    hideDetails: $localize`:@@pd.wb.assistant.turn-failed.hide:Hide details`,
    answerReady: $localize`:@@pd.wb.assistant.answer-ready:Answer ready`,
    failed: $localize`:@@pd.wb.assistant.answer-failed:The answer failed.`,
  };

  /** The user is at (within 24 px of) the bottom of the log: new text keeps it there. */
  private pinned = true;
  /** New text arrived while the user reads further up (the "Jump to latest" pill). */
  protected readonly unseen = signal(false);
  /** One polite announcement when a turn ends; the streaming answer itself is not announced token by token. */
  protected readonly announcement = linkedSignal<{ running: boolean; turn: TurnView | null }, string>({
    source: () => {
      const turn = this.store.lastTurn();
      return { running: turn?.running === true, turn };
    },
    computation: (now, prev) => {
      if (prev?.source.running && !now.running) return this.finishedNote(now.turn);
      return prev && prev.source.running === now.running ? prev.value : '';
    },
  });

  constructor() {
    // The host's gate reaches the store, which sends it with each question (an input, not derived state of the store).
    effect(() => this.store.setActionsGate(this.actionsGate()));
    // Keeps the newest text in view while an answer streams in, but only for a user who is at the bottom (DOM only).
    // effect-state: `unseen` records the scroll decision of this effect; the user's position is not a signal, so it cannot be derived.
    effect(() => {
      this.store.turns();
      const el = this.log()?.nativeElement;
      if (!el) return;
      // After the new turn's DOM is rendered (an effect runs before the view updates), else the last block stays below.
      if (this.pinned) afterNextRender(() => (el.scrollTop = el.scrollHeight), { injector: this.injector });
      // The pill is state of this effect's scroll decision (the user's position is not a signal): set only, never read.
      else this.unseen.set(true);
    });
    // An action that cannot be undone: Decline is the default, so the focus goes there when it asks (DOM only).
    effect(() => {
      const ask = this.store.approval();
      const button = this.declineButton()?.nativeElement;
      if (ask?.destructive && button) button.focus();
    });
    // Keeps the latest item in view when the log itself gets shorter under a user at the bottom (an approval appears or opens
    // its Details below it): the browser keeps the scroll position, which would hide the waiting card (review 4 P3-1; DOM only).
    effect((onCleanup) => {
      const el = this.log()?.nativeElement;
      if (!el || typeof ResizeObserver === 'undefined') return;
      const watch = new ResizeObserver(() => {
        if (this.pinned) el.scrollTop = el.scrollHeight;
      });
      watch.observe(el);
      onCleanup(() => watch.disconnect());
    });
    // The question box shows the draft. Not a `[value]` binding: that is not re-rendered when typing and Enter happen in one
    // task (the draft goes 'x' then '' before any render), so the sent question would stay in the box (DOM only).
    effect(() => {
      const draft = this.store.draft();
      const el = this.inputEl()?.nativeElement;
      if (el && el.value !== draft) el.value = draft;
    });
  }

  /** One format for a context chip, in the composer and in the sent question. */
  protected readonly chipText = chipText;

  /** A conversation title as shown: no leading slash command, sentence case. */
  protected heading(raw: string): string {
    return conversationTitle(raw);
  }

  /** Deletes a conversation; the row (and the focused Delete button) goes away, so focus moves to Undo. */
  protected deleteChat(id: string): void {
    this.store.removeLater(id);
    afterNextRender(() => this.undoButton()?.nativeElement.focus(), { injector: this.injector });
  }

  /** The error of a turn that failed for its own reasons (a missing Pitwall is shown by the shared needs-AI state instead). */
  protected failedOf(turn: TurnView): { readonly message: string } | null {
    return turn.error && !turn.error.needsAi ? turn.error : null;
  }

  /** The phase a running turn waits in (the engine's report), shown instead of "Thinking…". */
  protected phaseOf(turn: TurnView): ProviderPhase | null {
    return waitingPhase(turn);
  }

  /** The words of a phase: the load says the time so far in whole seconds once there is one (a growing count, ER-18). */
  protected phaseText(phase: ProviderPhase): string {
    if (phase.kind === 'queued') return this.t.queued;
    const seconds = phase.kind === 'loading' ? Math.floor(phase.elapsedMs / 1000) : 0;
    return seconds > 0 ? $localize`:@@pd.wb.assistant.phase.loading-for:Loading the model… ${seconds}:seconds: s` : this.t.loadingModel;
  }

  protected allowLabel(tool: string): string {
    return $localize`:@@pd.wb.assistant.approve.allow-tool:Allow ${tool}:tool:`;
  }

  protected scrolled(): void {
    const el = this.log()?.nativeElement;
    if (!el) return;
    this.pinned = el.scrollHeight - el.scrollTop - el.clientHeight <= SCROLL_STICK_PX;
    if (this.pinned) this.unseen.set(false);
  }

  /** Sends the question; the log follows it to the end (the user's own message is where they look next). */
  protected submit(): void {
    this.pinned = true;
    this.unseen.set(false);
    void this.store.send();
  }

  protected jumpToLatest(): void {
    const el = this.log()?.nativeElement;
    if (el) el.scrollTop = el.scrollHeight;
    this.pinned = true;
    this.unseen.set(false);
  }

  private finishedNote(turn: TurnView | null): string {
    if (!turn) return '';
    if (turn.error) return turn.error.needsAi ? this.t.failed : turn.error.message;
    return turn.end === 'cancelled' ? this.t.stopped : this.t.answerReady;
  }

  protected deleted(title: string): string {
    return $localize`:@@pd.wb.assistant.deleted:Deleted "${title}:title:"`;
  }

  protected hasGlyph(name: string): boolean {
    return name.replace(/^pd:/, '') in PD_ICON_PATHS;
  }

  /** The call's arguments as the prompt's details show them (bounded). */
  protected hasArgs(input: unknown): boolean {
    return !(input === undefined || input === null || (typeof input === 'object' && Object.keys(input).length === 0));
  }

  protected args(input: unknown): string {
    const text = JSON.stringify(input, null, 2) ?? '';
    return text.length <= 2000 ? text : `${text.slice(0, 1999)}…`;
  }

  /** The composer's hint: the question, or what to do about Pitwall in the state's own words. */
  protected placeholder(): string {
    switch (this.pitwall?.kind()) {
      case 'notRunning':
        return $localize`:@@pd.wb.assistant.hint.not-running:Start Pitwall to ask questions`;
      case 'notInstalled':
        return $localize`:@@pd.wb.assistant.hint.not-installed:Install Pitwall to ask questions`;
      case 'needsPairing':
        return $localize`:@@pd.wb.assistant.hint.pairing:Connect Pitwall to ask questions`;
      case 'paused':
        return $localize`:@@pd.wb.assistant.hint.paused:Resume Pitwall to ask questions`;
      case 'noSuitableModel':
        return $localize`:@@pd.wb.assistant.hint.model:Download a model in Pitwall to ask questions`;
      case 'incompatible':
        return $localize`:@@pd.wb.assistant.hint.incompatible:Update to ask questions`;
      case 'unavailable':
        return $localize`:@@pd.wb.assistant.hint.unavailable:Pitwall is not available right now`;
      default:
        return this.t.placeholder;
    }
  }

  /** Pitwall is missing, not paired or has no model: the shared state shows the fix, the composer waits. */
  protected blocked(): boolean {
    return this.pitwall?.needsAttention() === true;
  }

  protected renderer(part: TurnPart): ReturnType<typeof rendererFor> {
    return part.kind === 'tool' ? rendererFor(this.renderers, part.card.name) : null;
  }

  protected cited(c: CitationOpened): void {
    if (this.citations) void this.citations.open(c);
    else this.citation.emit(c);
  }

  /** Enter sends, Shift+Enter adds a line (not while an input method composes), Escape stops. */
  protected key(e: KeyboardEvent): void {
    if (e.key === 'Escape' && this.store.busy()) {
      e.preventDefault();
      void this.store.stop();
    } else if (e.key === 'Enter' && !e.shiftKey && !e.isComposing) {
      e.preventDefault();
      if (!this.blocked()) this.submit();
    }
  }

  /** Moves the keyboard focus to the question (the command `pd.wb.assistant.focus`). */
  focusInput(): void {
    this.inputEl()?.nativeElement.focus();
  }
}
