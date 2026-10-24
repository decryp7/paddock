import { NgComponentOutlet } from '@angular/common';
import { ChangeDetectionStrategy, Component, ElementRef, effect, inject, input, output, viewChild } from '@angular/core';
import { PdGlyph } from '@decryp7/paddock-ui/icons';
import { PdNeedsAi, PitwallStore, type NextStep } from '@decryp7/paddock-ui/shell/pitwall';
import { AssistantStore } from './assistant.store';
import type { TurnPart } from './chat-model';
import { PdAssistantRichText, type CitationOpened } from './rich-text';
import { ASSISTANT_CITATIONS, injectToolRenderers, PdAssistantToolCard, rendererFor } from './tool-cards';

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
  imports: [NgComponentOutlet, PdGlyph, PdAssistantRichText, PdAssistantToolCard, PdNeedsAi],
  template: `
    <header class="bar">
      <span class="title">{{ t.title }}</span>
      <span class="spacer"></span>
      <button type="button" class="pd-btn pd-btn--ghost pd-btn--sm" data-testid="assistant-list-toggle" [attr.aria-expanded]="store.listOpen()" [attr.aria-label]="t.conversations" (click)="store.toggleListOpen()">
        <pd-glyph name="menu" [size]="14" />
      </button>
      <button type="button" class="pd-btn pd-btn--ghost pd-btn--sm" data-testid="assistant-new" [disabled]="store.busy()" [attr.aria-label]="t.newChat" [attr.title]="t.newChat" (click)="store.newConversation()">
        <pd-glyph name="plus" [size]="14" />
      </button>
    </header>

    @if (store.listOpen()) {
      <ul class="list" data-testid="assistant-list" [attr.aria-label]="t.conversations">
        @for (c of store.conversations(); track c.id) {
          <li class="row" [class.active]="c.id === store.activeId()">
            <button type="button" class="open" data-testid="assistant-open" [disabled]="store.busy()" [attr.title]="c.title" (click)="store.open(c.id)">{{ c.title }}</button>
            <button type="button" class="pd-btn pd-btn--ghost pd-btn--sm" data-testid="assistant-delete" [attr.aria-label]="t.deleteChat + ': ' + c.title" (click)="store.remove(c.id)">
              <pd-glyph name="trash" [size]="14" />
            </button>
          </li>
        } @empty {
          <li class="empty">{{ t.noConversations }}</li>
        }
      </ul>
    }

    @if (pitwall) {
      <pd-needs-ai class="needs" [feature]="t.feature" [settingsLink]="settingsLink()" (step)="step.emit($event)" (openSettings)="openSettings.emit()" />
    }

    <div class="log" role="log" aria-live="polite" [attr.aria-busy]="store.busy()" data-testid="assistant-log" #log>
      @if (store.isLoading() && !store.turns().length) {
        <p class="hint" data-testid="assistant-loading">{{ t.loading }}</p>
      }
      @if (store.requestError(); as problem) {
        <div class="failure" role="alert" data-testid="assistant-load-failed">
          <span>{{ problem }}</span>
          <button type="button" class="pd-btn pd-btn--sm" data-testid="assistant-retry" (click)="store.load()">{{ t.retry }}</button>
        </div>
      } @else if (!store.turns().length && store.isLoaded()) {
        <p class="hint" data-testid="assistant-empty">{{ t.empty }}</p>
      }
      @for (turn of store.turns(); track $index) {
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
            @if (turn.running && !turn.parts.length) {
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
            @if (turn.error && !turn.error.needsAi) {
              <p class="error" role="alert" data-testid="assistant-error">{{ turn.error.message }}</p>
            }
          </div>
        </article>
      }
    </div>

    @if (store.approval(); as ask) {
      <section class="approval" role="alertdialog" aria-labelledby="pd-assistant-approval" data-testid="assistant-approval">
        <p id="pd-assistant-approval"><strong>{{ t.wantsTo }}</strong> {{ ask.summary }}</p>
        <div class="actions">
          <button type="button" class="pd-btn pd-btn--primary pd-btn--sm" data-testid="assistant-allow" (click)="store.approve(ask.callId, true)">{{ t.allow }}</button>
          <button type="button" class="pd-btn pd-btn--sm" data-testid="assistant-decline" (click)="store.approve(ask.callId, false)">{{ t.decline }}</button>
        </div>
      </section>
    }

    @if (store.failure(); as failure) {
      <p class="error" role="alert" data-testid="assistant-failure">{{ failure }} <button type="button" class="pd-btn pd-btn--ghost pd-btn--sm" (click)="store.dismissFailure()">{{ t.dismiss }}</button></p>
    }

    <form class="composer" (submit)="$event.preventDefault(); store.send()">
      @if (store.chips().length) {
        <ul class="chips" [attr.aria-label]="t.context" data-testid="assistant-chips">
          @for (chip of store.chips(); track chip.id) {
            <li>
              <button type="button" class="chip" data-testid="assistant-chip" [class.off]="!chip.on" [attr.aria-pressed]="chip.on" [attr.title]="chip.on ? t.removeChip : t.addChip" (click)="store.toggleChip(chip.id)">
                @if (chip.icon) {
                  <pd-glyph [name]="chip.icon" [size]="14" />
                }
                <span class="chip-label">{{ chip.label }}</span>
                @if (chip.detail) {
                  <span class="chip-detail">{{ chip.detail }}</span>
                }
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
        [attr.placeholder]="t.placeholder"
        [attr.maxlength]="store.limit()"
        [value]="store.draft()"
        (input)="store.setDraft($any($event.target).value)"
        (keydown)="key($event)"
      ></textarea>
      <div class="send-row">
        @if (store.hasWriteTools()) {
          <label class="allow" [attr.title]="t.allowHint">
            <input type="checkbox" data-testid="assistant-allow-actions" [checked]="store.allowWrite()" [disabled]="store.busy()" (change)="store.setAllowWrite($any($event.target).checked)" />
            <span>{{ t.allowActions }}</span>
          </label>
        }
        <span class="spacer"></span>
        @if (store.busy()) {
          <button type="button" class="pd-btn" data-testid="assistant-stop" [disabled]="store.run() === 'stopping'" (click)="store.stop()">
            <pd-glyph name="stop" [size]="14" /> {{ t.stop }}
          </button>
        } @else {
          <button type="submit" class="pd-btn pd-btn--primary" data-testid="assistant-send" [disabled]="!store.canSend() || blocked()">
            <pd-glyph name="play" [size]="14" /> {{ t.send }}
          </button>
        }
      </div>
    </form>
  `,
  styles: `
    :host { display: flex; flex-direction: column; height: 100%; min-height: 0; min-width: 0; container-type: inline-size; background: var(--pd-surface-1); color: var(--pd-text); font-size: var(--pd-text-body); }
    .bar { display: flex; align-items: center; gap: var(--pd-space-1); padding: var(--pd-space-1) var(--pd-space-2); border-bottom: 1px solid var(--pd-hairline); min-height: var(--pd-toolbar-height); }
    .title { font-weight: var(--pd-weight-semibold); }
    .spacer { flex: 1; }
    .list { list-style: none; margin: 0; padding: var(--pd-space-1); max-height: 40%; overflow-y: auto; border-bottom: 1px solid var(--pd-hairline); }
    .row { display: flex; align-items: center; gap: var(--pd-space-1); border-radius: var(--pd-radius-control); }
    .row.active { background: var(--pd-accent-tint); }
    .row:hover { background: var(--pd-hover); }
    .open { all: unset; flex: 1; min-width: 0; padding: var(--pd-space-1) var(--pd-space-2); cursor: pointer; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .open:focus-visible { outline: 2px solid var(--pd-focus-ring); border-radius: var(--pd-radius-control); }
    .empty, .hint { color: var(--pd-text-muted); padding: var(--pd-space-2); margin: 0; }
    .needs { margin: var(--pd-space-2); }
    .log { flex: 1; min-height: 0; overflow-y: auto; padding: var(--pd-space-3); display: flex; flex-direction: column; gap: var(--pd-space-4); }
    .turn { display: flex; flex-direction: column; gap: var(--pd-space-2); min-width: 0; }
    .user { align-self: flex-end; max-width: 90%; background: var(--pd-accent-tint); border-radius: var(--pd-radius-card); padding: var(--pd-space-2) var(--pd-space-3); overflow-wrap: anywhere; }
    .user p { margin: 0; white-space: pre-wrap; }
    .attached, .chips { list-style: none; margin: var(--pd-space-1) 0 0; padding: 0; display: flex; flex-wrap: wrap; gap: var(--pd-space-1); }
    .answer { display: flex; flex-direction: column; gap: var(--pd-space-2); min-width: 0; }
    .note { margin: 0; color: var(--pd-text-muted); font-size: var(--pd-text-label); }
    .error { margin: 0; color: var(--pd-danger); overflow-wrap: anywhere; padding: 0 var(--pd-space-3); }
    .failure { display: flex; gap: var(--pd-space-2); align-items: center; color: var(--pd-danger); }
    .approval { margin: 0 var(--pd-space-3); padding: var(--pd-space-3); border: 1px solid var(--pd-warning); background: var(--pd-warning-tint); border-radius: var(--pd-radius-card); }
    .approval p { margin: 0 0 var(--pd-space-2); overflow-wrap: anywhere; }
    .actions { display: flex; gap: var(--pd-space-2); flex-wrap: wrap; }
    .composer { display: flex; flex-direction: column; gap: var(--pd-space-2); padding: var(--pd-space-2) var(--pd-space-3) var(--pd-space-3); border-top: 1px solid var(--pd-hairline); }
    .chip { display: inline-flex; align-items: center; gap: var(--pd-space-1); max-width: 100%; min-height: 24px; padding: 0 var(--pd-space-2); border: 1px solid var(--pd-border-control); border-radius: var(--pd-radius-pill); background: var(--pd-surface-2); color: var(--pd-text); font: inherit; font-size: var(--pd-text-label); cursor: pointer; }
    .chip.static { cursor: default; min-height: 0; }
    .chip.off { opacity: 0.6; text-decoration: line-through; }
    .chip:focus-visible { outline: 2px solid var(--pd-focus-ring); outline-offset: 1px; }
    .chip-label { font-weight: var(--pd-weight-medium); }
    .chip-detail { color: var(--pd-text-muted); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; min-width: 0; }
    .input { width: 100%; box-sizing: border-box; resize: none; font: inherit; color: var(--pd-text); background: var(--pd-surface-2); border: 1px solid var(--pd-border-control); border-radius: var(--pd-radius-input); padding: var(--pd-space-2); }
    .input:focus-visible { outline: 2px solid var(--pd-focus-ring); outline-offset: 0; }
    .send-row { display: flex; align-items: center; gap: var(--pd-space-2); flex-wrap: wrap; }
    .allow { display: inline-flex; align-items: center; gap: var(--pd-space-1); font-size: var(--pd-text-label); color: var(--pd-text-muted); }
    @container (max-width: 360px) { .log { padding: var(--pd-space-2); } .user { max-width: 100%; } .chip-detail { display: none; } }
  `,
})
export class PdAssistant {
  protected readonly store = inject(AssistantStore);
  protected readonly pitwall = inject(PitwallStore, { optional: true });
  private readonly renderers = injectToolRenderers();
  private readonly citations = inject(ASSISTANT_CITATIONS);
  private readonly log = viewChild<ElementRef<HTMLElement>>('log');
  private readonly inputEl = viewChild<ElementRef<HTMLTextAreaElement>>('input');

  /** Offer "Open AI settings" in the needs-AI state (the app's AI page). */
  readonly settingsLink = input(false);
  /** A step only the app can run (open or install Pitwall, update, change permissions), from the needs-AI state. */
  readonly step = output<NextStep>();
  /** The user chose "Open AI settings". */
  readonly openSettings = output<void>();
  /** A citation was activated and the app has no handler registered. */
  readonly citation = output<CitationOpened>();

  protected readonly t = {
    title: $localize`:@@pd.wb.assistant.title:Assistant`,
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
  };

  constructor() {
    // Keeps the newest text in view while an answer streams in (DOM only).
    effect(() => {
      this.store.turns();
      const el = this.log()?.nativeElement;
      if (el) el.scrollTop = el.scrollHeight;
    });
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
      if (!this.blocked()) void this.store.send();
    }
  }

  /** Moves the keyboard focus to the question (the command `pd.wb.assistant.focus`). */
  focusInput(): void {
    this.inputEl()?.nativeElement.focus();
  }
}
