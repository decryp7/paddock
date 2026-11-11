import { NgComponentOutlet } from '@angular/common';
import { afterNextRender, ChangeDetectionStrategy, Component, DestroyRef, ElementRef, effect, Injector, inject, input, linkedSignal, output, signal, untracked, viewChild } from '@angular/core';
import { PD_ICON_PATHS, PdGlyph, PdStatusGlyph } from '@decryp7/paddock-ui/icons';
import { PdNeedsAi, PitwallStore, type NextStep } from '@decryp7/paddock-ui/shell/pitwall';
import { AssistantStore, type ActionsGate } from './assistant.store';
import { AssistantAttachments, attachmentChip, type AttachmentChip, type AttachSource } from './composer-ports';
import type { Attachment, ContextUse } from '../generated/assistant';
import { chipText, conversationTitle, waitingPhase, type TurnPart, type TurnView } from './chat-model';
import type { ProviderPhase } from '../generated/assistant';
import { PdAssistantRichText, type CitationOpened } from './rich-text';
import { ASSISTANT_CITATIONS, injectToolRenderers, PdAssistantToolCard, rendererFor } from './tool-cards';

/**
 * A pane shorter than this (px) is short: the files tray folds to two rows behind its "N files" chip, the question box is two
 * lines, the empty state gives up its icon and a refusal's banner is tighter (review 2 P2-A). It is the pane's own height
 * (review 3 P2-2), so a pane docked in a tall window folds too; a host may hide its own extras at the same height.
 */
export const ASSISTANT_SHORT_PANE_PX = 440;

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
  host: { class: 'pd-assistant', '[class.short]': 'short()' },
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
            @if (turn.attachments.length) {
              <ul class="attached" [attr.aria-label]="t.files" data-testid="assistant-turn-files">
                @for (a of turn.attachments; track a.sha256) {
                  @let chip = chipOf(a);
                  <li class="chip static file" data-testid="assistant-turn-file" [attr.title]="chipTitle(chip)">
                    @if (thumbOf(a.sha256, chip); as src) {
                      <img class="thumb turn-thumb" [src]="src" alt="" data-testid="assistant-turn-thumb" />
                    } @else {
                      <pd-glyph [name]="glyphOf(a)" [size]="14" />
                    }
                    <span class="chip-label">{{ chip.label }}</span>
                  </li>
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
            <!-- TODO(ADR 0023 WP-4b, Debrief 1.2 package 2.6c): "Still working" after a quiet spell, from paddock-jobs::patience
                 (ADR 0031). It lands only once 2.6c has created that crate; no stall judgement of our own here (rule 55). -->
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
      <div class="banner error" role="alert" data-testid="assistant-failure">
        <pd-status-glyph class="banner-glyph" status="error" [size]="16" />
        <span class="banner-text" data-testid="assistant-failure-text">{{ failure }}</span>
        <button type="button" class="pd-btn pd-btn--sm" data-testid="assistant-failure-dismiss" (click)="store.dismissFailure()">{{ t.dismiss }}</button>
      </div>
    }

    @if (store.fullBy(); as by) {
      <div class="banner full" role="status" data-testid="assistant-context-full">
        <pd-status-glyph class="banner-glyph" status="error" [size]="16" />
        <span class="banner-text">{{ fullNote() }}</span>
        @if (by === 'chat' && store.activeId()) {
          <button type="button" class="pd-btn pd-btn--sm" data-testid="assistant-context-new" [disabled]="store.busy()" (click)="store.newConversation()">{{ t.startNewChat }}</button>
        }
      </div>
    }

    <form
      class="composer"
      [class.dropping]="dropping()"
      data-testid="assistant-composer"
      (submit)="$event.preventDefault(); submit()"
      (dragover)="dragOver($event)"
      (dragleave)="dragLeave($event)"
      (drop)="dropped($event)"
    >
      @if (dropping()) {
        <p class="drop-hint" aria-hidden="true"><span class="drop-label">{{ t.dropHere }}</span></p>
      }
      @if (store.attached().length || store.attaching()) {
        <ul
          #tray
          id="pd-assistant-files"
          class="chips files-tray"
          [class.folded]="short() && !filesOpen()"
          [class.open]="short() && filesOpen()"
          [class.scrolls]="trayOverflows() && !(short() && !filesOpen())"
          [attr.aria-label]="t.files"
          data-testid="assistant-files"
        >
          @if (short() && (trayOverflows() || filesOpen())) {
            <li class="summary-item">
              <button
                type="button"
                class="chip summary"
                data-testid="assistant-files-summary"
                aria-controls="pd-assistant-files"
                [attr.aria-expanded]="filesOpen()"
                [attr.title]="filesOpen() ? t.foldFiles : t.showFiles"
                (click)="filesOpen.set(!filesOpen())"
              >
                <pd-glyph name="file" [size]="14" />
                <span class="chip-label">{{ filesCount() }}</span>
                <pd-glyph class="summary-chev" [class.up]="filesOpen()" name="chevron-down" [size]="14" />
              </button>
            </li>
          }
          @for (chip of store.attached(); track chip.attachment.sha256) {
            <li class="chip file" [class.has-thumb]="!!thumbOf(chip.attachment.sha256, chip)" data-testid="assistant-file" [attr.title]="chipTitle(chip)">
              @if (thumbOf(chip.attachment.sha256, chip); as src) {
                <img class="thumb" [src]="src" alt="" data-testid="assistant-file-thumb" />
              } @else {
                <pd-glyph [name]="glyphOf(chip.attachment)" [size]="14" />
              }
              <span class="chip-label">{{ chip.label }}</span>
              @if (chip.detail) {
                <span class="chip-detail">{{ chip.detail }}</span>
              }
              <button type="button" class="chip-x" data-testid="assistant-file-remove" [attr.aria-label]="removeLabel(chip.label)" [attr.title]="t.removeFile" (click)="detach(chip)">
                <pd-glyph name="close" [size]="14" />
              </button>
            </li>
          }
          @if (store.attaching()) {
            <li class="chip static busy" role="status" data-testid="assistant-attaching"><span class="pulse" aria-hidden="true"><i></i><i></i><i></i></span> {{ t.attaching }}</li>
          }
        </ul>
      }
      @if (store.imageBlock(); as why) {
        <p class="image-block" role="status" data-testid="assistant-image-block">
          <pd-status-glyph status="warning" [size]="14" />
          <span>{{ why }}</span>
        </p>
      }
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
        [rows]="short() ? 2 : 3"
        data-testid="assistant-input"
        [attr.aria-label]="t.question"
        [disabled]="blocked()"
        [attr.placeholder]="placeholder()"
        [attr.maxlength]="store.limit()"
        (input)="store.setDraft($any($event.target).value)"
        (keydown)="key($event)"
        (paste)="pasted($event)"
      ></textarea>
      <div class="send-row">
        @if (store.canAttach()) {
          <input #picker type="file" hidden multiple data-testid="assistant-file-input" [attr.accept]="store.attachRules()?.accept ?? null" (change)="picked($event)" />
          <button type="button" class="pd-btn pd-btn--ghost pd-btn--sm compose-btn" data-testid="assistant-attach" [disabled]="blocked() || !store.roomToAttach()" [attr.aria-label]="t.attach" [attr.title]="store.attachRules()?.hint ?? t.attach" (click)="picker.click()">
            <pd-glyph name="paperclip" [size]="16" />
          </button>
        }
        @if (store.canSpeak()) {
          <button
            type="button"
            class="pd-btn pd-btn--ghost pd-btn--sm compose-btn mic"
            data-testid="assistant-mic"
            [class.recording]="store.speech().kind === 'recording'"
            [disabled]="blocked() || store.speech().kind === 'transcribing'"
            [attr.aria-pressed]="store.speech().kind === 'recording'"
            [attr.aria-label]="store.speech().kind === 'recording' ? t.micStop : t.mic"
            [attr.title]="store.speech().kind === 'recording' ? t.micStop : t.micHint"
            (click)="store.toggleSpeech()"
          >
            <pd-glyph [name]="store.speech().kind === 'recording' ? 'stop' : 'mic'" [size]="16" />
          </button>
        }
        @switch (store.speech().kind) {
          @case ('recording') {
            <span class="speech rec" role="status" data-testid="assistant-recording"><i class="rec-dot" aria-hidden="true"></i>{{ recordingText() }}</span>
          }
          @case ('transcribing') {
            <span class="speech" role="status" data-testid="assistant-transcribing"><span class="pulse" aria-hidden="true"><i></i><i></i><i></i></span> {{ t.transcribing }}</span>
          }
          @default {
            @if (store.context(); as used) {
              <span
                class="ctx"
                [attr.data-level]="used.level"
                data-testid="assistant-context"
                [attr.title]="meterTitle(used)"
                [attr.role]="used.limitTokens > 0 ? 'meter' : null"
                [attr.aria-label]="used.limitTokens > 0 ? t.contextUsed : null"
                [attr.aria-valuemin]="used.limitTokens > 0 ? 0 : null"
                [attr.aria-valuemax]="used.limitTokens > 0 ? used.limitTokens : null"
                [attr.aria-valuenow]="used.limitTokens > 0 ? used.usedTokens : null"
                [attr.aria-valuetext]="used.limitTokens > 0 ? meterSpoken(used) : null"
              >
                @if (used.limitTokens > 0) {
                  <span class="ctx-bar" aria-hidden="true"><span class="ctx-fill" [style.width.%]="fillPercent(used)"></span></span>
                }
                @if (used.level === 'high' || used.level === 'full') {
                  <pd-status-glyph class="ctx-glyph" [status]="used.level === 'full' ? 'error' : 'warning'" [size]="14" />
                }
                <span class="ctx-text">{{ meterText(used) }}</span>
              </span>
            }
          }
        }
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
          <button type="submit" class="pd-btn" [class.pd-btn--primary]="!blocked()" data-testid="assistant-send" [disabled]="!store.canSend() || blocked()" [attr.title]="store.full() ? fullNote() : null">
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
    .banner { display: flex; align-items: center; gap: var(--pd-space-2); margin: 0 var(--pd-space-3) var(--pd-space-2); padding: var(--pd-space-2) var(--pd-space-3); border: 1px solid var(--pd-danger); background: var(--pd-danger-tint); border-radius: var(--pd-radius-card); }
    .banner-glyph { flex: none; color: var(--pd-danger); }
    .banner-text { flex: 1; min-width: 0; overflow-wrap: anywhere; }
    .banner .pd-btn { flex: none; }
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
    .chip.file { cursor: default; padding-right: 2px; }
    /* Many files wrap into rows; past about three rows the tray scrolls, so the question box and the log keep their room. */
    .files-tray { box-sizing: border-box; max-height: 92px; overflow-y: auto; padding-block: 1px; }
    /*
     * A short pane (under ASSISTANT_SHORT_PANE_PX) folds the tray to two rows (24 px chips, a 4 px gap, 1 px padding each side)
     * behind an "N files" chip that opens it, the question box to two lines and a refusal to a tighter banner, and the empty
     * state gives up its icon (review 2 P2-A). The log takes 120 px before anything else grows; in a pane too short for that and
     * the composer it gives way first, down to a 56 px floor (two lines of text), so it never collapses (review 3 P2-3).
     */
    .files-tray.folded { max-height: 54px; overflow: hidden; }
    /* Opened in a short pane it shows about two and a half rows, so the question, Send and the log's floor keep their room. */
    .files-tray.open { max-height: 82px; }
    /* A tray that scrolls says so: its foot fades, and the padding under the last row is the fade, so at the end it is clear. */
    .files-tray.scrolls { --tray-fade: 16px; padding-bottom: var(--tray-fade); mask-image: linear-gradient(to bottom, var(--pd-surface-1) calc(100% - var(--tray-fade)), transparent); }
    .summary { font-weight: var(--pd-weight-medium); background: var(--pd-surface-1); }
    .summary:hover { background: var(--pd-hover); }
    .summary-chev { transition: transform var(--pd-duration-fast, 120ms); }
    .summary-chev.up { transform: rotate(180deg); }
    .log { flex: 1 1 120px; min-height: 56px; }
    :host(.short) .empty-state { gap: 0; }
    :host(.short) .empty-icon { display: none; }
    :host(.short) .log { padding-block: var(--pd-space-2); }
    :host(.short) .banner { padding-block: var(--pd-space-1); }
    @media (prefers-reduced-motion: reduce) { .summary-chev { transition: none; } }
    .chip.static.file { padding-right: var(--pd-space-2); }
    .chip-detail { color: var(--pd-text-muted); white-space: nowrap; }
    .chip-x { all: unset; display: inline-grid; place-items: center; width: 24px; height: 24px; margin-block: -1px; border-radius: var(--pd-radius-pill); color: var(--pd-text-muted); cursor: pointer; }
    .chip-x:hover { background: var(--pd-hover); color: var(--pd-text); }
    .chip-x:focus-visible { outline: 2px solid var(--pd-focus-ring); }
    .chip.busy { color: var(--pd-text-muted); gap: var(--pd-space-2); }
    /* A picture's thumbnail replaces the file glyph: 28 px in the tray (the chip grows to 32), 20 px in a short pane's folded tray
       (the chip stays 24), 40 px in the sent question. object-fit keeps the picture's shape inside the square. */
    .thumb { width: 28px; height: 28px; flex: none; object-fit: cover; border-radius: var(--pd-radius-tag); background: var(--pd-surface-1); }
    .chip.has-thumb { padding: 3px var(--pd-space-1) 3px 3px; min-height: 32px; border-radius: var(--pd-radius-control); }
    .turn-thumb { width: 40px; height: 40px; }
    .chip.static.file:has(.turn-thumb) { padding: 3px var(--pd-space-2) 3px 3px; border-radius: var(--pd-radius-control); gap: var(--pd-space-2); }
    :host(.short) .chip.has-thumb { min-height: 24px; padding-block: 1px; border-radius: var(--pd-radius-pill); }
    :host(.short) .chip.has-thumb .thumb { width: 20px; height: 20px; }
    .image-block { display: flex; align-items: flex-start; gap: var(--pd-space-2); margin: 0; font-size: var(--pd-text-label); color: var(--pd-text); overflow-wrap: anywhere; }
    .composer { position: relative; }
    .composer.dropping { background: var(--pd-accent-tint); box-shadow: inset 0 0 0 2px var(--pd-accent); border-radius: inherit; }
    .drop-hint { position: absolute; inset: 0; margin: 0; display: grid; place-items: center; color: var(--pd-accent); font-weight: var(--pd-weight-semibold); pointer-events: none; z-index: 1; }
    .composer.dropping > :not(.drop-hint) { opacity: 0.25; }
    /* The words sit on the pane's surface, so the faded question box's edge never runs through them. */
    .drop-label { padding: var(--pd-space-1) var(--pd-space-3); border-radius: var(--pd-radius-pill); background: var(--pd-surface-1); box-shadow: inset 0 0 0 1px var(--pd-accent); }
    .compose-btn { min-width: 28px; padding-inline: var(--pd-space-1); }
    /* Pressed while recording (a red tint, unlike a hover); back to a plain ghost button the moment recording stops. */
    .mic.recording, .mic.recording:hover { color: var(--pd-danger); background: var(--pd-danger-tint); }
    .speech { display: inline-flex; align-items: center; gap: var(--pd-space-2); font-size: var(--pd-text-label); color: var(--pd-text-muted); font-variant-numeric: tabular-nums; }
    .speech.rec { color: var(--pd-text); }
    .rec-dot { width: 8px; height: 8px; border-radius: 50%; background: var(--pd-danger); animation: pd-pulse 1.2s ease-in-out infinite; }
    .ctx { display: inline-flex; align-items: center; gap: var(--pd-space-2); min-width: 0; font-size: var(--pd-text-label); color: var(--pd-text-muted); font-variant-numeric: tabular-nums; }
    .ctx-text { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .ctx-bar { flex: none; width: 48px; height: 4px; border-radius: 2px; background: var(--pd-border-strong); overflow: hidden; }
    .ctx-fill { display: block; height: 100%; background: var(--pd-text-muted); border-radius: inherit; }
    .ctx[data-level='high'] { color: var(--pd-text); }
    .ctx[data-level='high'] .ctx-fill { background: var(--pd-warning); }
    .ctx[data-level='full'] { color: var(--pd-danger); }
    .ctx[data-level='full'] .ctx-fill { background: var(--pd-danger); }
    .ctx-glyph { flex: none; }
    .ctx[data-level='high'] .ctx-glyph { color: var(--pd-warning); }
    .ctx[data-level='full'] .ctx-glyph { color: var(--pd-danger); }
    @media (prefers-reduced-motion: reduce) { .rec-dot { animation: none; } }
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
  private readonly attachments = inject(AssistantAttachments, { optional: true });
  /** Files are being dragged over the composer (its drop highlight). */
  protected readonly dropping = signal(false);
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
  /** The conversation now open (null: a new one, not sent yet), so a host can keep per-chat state beside the pane. */
  readonly activeChange = output<string | null>();

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
    files: $localize`:@@pd.wb.assistant.files:Attached files`,
    attach: $localize`:@@pd.wb.assistant.attach:Attach files`,
    attaching: $localize`:@@pd.wb.assistant.attaching:Attaching…`,
    removeFile: $localize`:@@pd.wb.assistant.attach.remove:Remove from this question`,
    dropHere: $localize`:@@pd.wb.assistant.attach.drop:Drop files to attach them`,
    mic: $localize`:@@pd.wb.assistant.mic:Speak your question`,
    micHint: $localize`:@@pd.wb.assistant.mic.hint:Speak your question (press again to stop; Escape cancels)`,
    micStop: $localize`:@@pd.wb.assistant.mic.stop:Stop recording`,
    transcribing: $localize`:@@pd.wb.assistant.transcribing:Writing down what you said…`,
    contextFull: $localize`:@@pd.wb.assistant.context.full-note:This chat fills what the model can read.`,
    filesTooLong: $localize`:@@pd.wb.assistant.context.files-too-long:These files are too long for the model. Remove a file.`,
    showFiles: $localize`:@@pd.wb.assistant.files.show:Show all attached files`,
    foldFiles: $localize`:@@pd.wb.assistant.files.fold:Show fewer attached files`,
    closeToLimit: $localize`:@@pd.wb.assistant.context.close:close to the limit`,
    startNewChat: $localize`:@@pd.wb.assistant.context.start-new:Start a new chat`,
    contextUsed: $localize`:@@pd.wb.assistant.context.label:Context used`,
  };

  /** The pane is short (under `ASSISTANT_SHORT_PANE_PX` tall): the files tray folds to two rows behind its "N files" chip. */
  protected readonly short = signal(false);
  /** The files tray holds more than it shows: folded, more than its two rows; open, more than it shows before it scrolls
   *  (measured after it renders). */
  protected readonly trayOverflows = signal(false);
  /** The user opened the folded files tray; it folds again when the files go (sent or removed). */
  protected readonly filesOpen = linkedSignal({ source: () => this.store.attached().length === 0, computation: () => false });
  private readonly tray = viewChild<ElementRef<HTMLElement>>('tray');

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
    // A short pane folds the files tray (DOM only: the pane's height is a measurement, not a signal).
    if (typeof ResizeObserver !== 'undefined') {
      const host = inject<ElementRef<HTMLElement>>(ElementRef).nativeElement;
      const watch = new ResizeObserver(() => {
        const height = host.clientHeight;
        this.short.set(height > 0 && height < ASSISTANT_SHORT_PANE_PX);
      });
      watch.observe(host);
      inject(DestroyRef).onDestroy(() => watch.disconnect());
    }
    // effect-state: whether the files tray overflows is a measurement of the rendered DOM (wrapping), not derivable state.
    effect((onCleanup) => {
      this.store.attached();
      this.store.attaching();
      this.short();
      this.filesOpen();
      const el = this.tray()?.nativeElement;
      if (!el) return;
      const measure = (): void => this.trayOverflows.set(el.scrollHeight > el.clientHeight + 1);
      afterNextRender(measure, { injector: this.injector });
      if (typeof ResizeObserver === 'undefined') return;
      const watch = new ResizeObserver(measure);
      watch.observe(el);
      onCleanup(() => watch.disconnect());
    });
    // effect-state: the host's small copies of pictures are a cache filled by asking it (an I/O answer, not derivable); a question's
    // pictures are asked for once each, and what the chips just attached carry is remembered for the sent question.
    effect(() => {
      const chips = this.store.attached();
      const turns = this.store.turns();
      untracked(() => {
        for (const c of chips) if (c.thumbnail) this.rememberThumbnail(c.attachment.sha256, c.thumbnail);
        const port = this.attachments;
        for (const t of turns) {
          for (const a of t.attachments) {
            if (a.kind !== 'image' || !port || this.thumbnails().has(a.sha256) || this.thumbnailsAsked.has(a.sha256)) continue;
            this.thumbnailsAsked.add(a.sha256);
            void port
              .thumbnail(a)
              .then((url) => url && this.rememberThumbnail(a.sha256, url))
              .catch(() => undefined);
          }
        }
      });
    });
    // The host's gate reaches the store, which sends it with each question (an input, not derived state of the store).
    effect(() => this.store.setActionsGate(this.actionsGate()));
    // Tells the host which conversation is open (an output: outside the signal graph).
    effect(() => {
      const id = this.store.activeId();
      untracked(() => this.activeChange.emit(id));
    });
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
        // An empty conversation has nothing to follow: its empty state stays whole from the top when the composer grows
        // (files attached on a short window).
        if (this.pinned && this.store.turns().length) el.scrollTop = el.scrollHeight;
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
    if (e.key === 'Escape' && this.store.speech().kind === 'recording') {
      e.preventDefault();
      void this.store.cancelSpeech();
    } else if (e.key === 'Escape' && this.store.busy()) {
      e.preventDefault();
      void this.store.stop();
    } else if (e.key === 'Enter' && !e.shiftKey && !e.isComposing) {
      e.preventDefault();
      if (!this.blocked()) this.submit();
    }
  }

  /** Small copies of pictures by digest: those of the chips just attached, and those the host gives for a reopened question. */
  private readonly thumbnails = signal<ReadonlyMap<string, string>>(new Map());
  private readonly thumbnailsAsked = new Set<string>();

  /** A picture's thumbnail (the chip's own, else the host's copy); `null` for text and for a picture with none. */
  protected thumbOf(sha256: string, chip: AttachmentChip): string | null {
    return chip.thumbnail ?? this.thumbnails().get(sha256) ?? null;
  }

  private rememberThumbnail(sha256: string, url: string): void {
    if (this.thumbnails().get(sha256) !== url) this.thumbnails.update((m) => new Map(m).set(sha256, url));
  }

  /** An attachment's chip as the host names it (a reopened conversation's too). */
  protected chipOf(a: Attachment): AttachmentChip {
    return this.attachments?.chipOf(a) ?? attachmentChip(a);
  }

  protected glyphOf(a: Attachment): string {
    return a.kind === 'file' ? 'file-text' : a.kind === 'audio' ? 'mic' : 'file';
  }

  protected chipTitle(chip: AttachmentChip): string {
    return chip.detail ? `${chip.label} (${chip.detail})` : chip.label;
  }

  protected removeLabel(name: string): string {
    return $localize`:@@pd.wb.assistant.attach.remove-named:Remove ${name}:name:`;
  }

  /** Takes a file off the next question; the focus goes back to the question (the chip and its button are gone). */
  protected detach(chip: AttachmentChip): void {
    this.store.detach(chip.attachment.sha256);
    this.focusInput();
  }

  private sources(files: FileList | null | undefined): AttachSource[] {
    return Array.from(files ?? []).map((f) => ({ name: f.name, type: f.type, size: f.size, bytes: async () => new Uint8Array(await f.arrayBuffer()) }));
  }

  /** The picker's files go to the engine; the picker is emptied so the same file can be chosen again. */
  protected picked(e: Event): void {
    const input = e.target as HTMLInputElement;
    const files = this.sources(input.files);
    input.value = '';
    void this.store.attach(files);
  }

  /** Pasted files are attached (pasted text stays text). */
  protected pasted(e: ClipboardEvent): void {
    if (!this.store.canAttach() || this.blocked()) return;
    const files = this.sources(e.clipboardData?.files);
    if (!files.length) return;
    e.preventDefault();
    void this.store.attach(files);
  }

  private carriesFiles(e: DragEvent): boolean {
    return this.store.canAttach() && !this.blocked() && Array.from(e.dataTransfer?.types ?? []).includes('Files');
  }

  protected dragOver(e: DragEvent): void {
    if (!this.carriesFiles(e)) return;
    e.preventDefault();
    if (e.dataTransfer) e.dataTransfer.dropEffect = 'copy';
    this.dropping.set(true);
  }

  protected dragLeave(e: DragEvent): void {
    // Leaving for a child of the composer is not leaving it.
    const to = e.relatedTarget as Node | null;
    if (!to || !(e.currentTarget as Node).contains(to)) this.dropping.set(false);
  }

  protected dropped(e: DragEvent): void {
    this.dropping.set(false);
    if (!this.carriesFiles(e)) return;
    e.preventDefault();
    void this.store.attach(this.sources(e.dataTransfer?.files));
  }

  /** "Recording 0:07": the engine's elapsed time, in minutes and seconds. */
  protected recordingText(): string {
    const s = this.store.speech();
    const total = s.kind === 'recording' ? Math.floor(s.elapsedMs / 1000) : 0;
    const time = `${Math.floor(total / 60)}:${String(total % 60).padStart(2, '0')}`;
    return $localize`:@@pd.wb.assistant.recording:Recording ${time}:time:`;
  }

  /** A token count as people read it: 950, 1.2k, 32k. */
  protected tokens(n: number): string {
    if (n < 1000) return String(n);
    const k = n / 1000;
    return `${k < 10 ? k.toFixed(1).replace(/\.0$/, '') : Math.round(k)}k`;
  }

  protected fillPercent(used: ContextUse): number {
    return used.limitTokens > 0 ? Math.min(100, (used.usedTokens / used.limitTokens) * 100) : 0;
  }

  /** The count: the engine's own (or "about" for its estimate) out of the room the conversation has. */
  private count(used: ContextUse): string {
    const n = this.tokens(used.usedTokens);
    if (used.limitTokens <= 0) return used.exact ? $localize`:@@pd.wb.assistant.context.tokens:${n}:n: tokens` : $localize`:@@pd.wb.assistant.context.about-tokens:About ${n}:n: tokens`;
    const of = this.tokens(used.limitTokens);
    return used.exact ? $localize`:@@pd.wb.assistant.context.of:${n}:n: of ${of}:of: tokens` : $localize`:@@pd.wb.assistant.context.about-of:About ${n}:n: of ${of}:of: tokens`;
  }

  /** The meter's words, its level included (never colour alone): "6.9k of 7.2k tokens · close to the limit", "Full: …". */
  protected meterText(used: ContextUse): string {
    if (used.level === 'full') {
      const of = this.tokens(used.limitTokens);
      const over = used.usedTokens - used.limitTokens;
      return over > 0 ? $localize`:@@pd.wb.assistant.context.over:Full: over by ${this.tokens(over)}:over: of ${of}:of: tokens` : $localize`:@@pd.wb.assistant.context.full:Full: ${of}:n: of ${of}:of: tokens`;
    }
    if (used.level === 'high') return `${this.count(used)} · ${this.t.closeToLimit}`;
    return this.count(used);
  }

  /** What a screen reader says for the meter: the same words, with a comma for the dot. */
  protected meterSpoken(used: ContextUse): string {
    return this.meterText(used).replace(' · ', ', ');
  }

  /** The meter's tooltip: what the count is, and what its level means. */
  protected meterTitle(used: ContextUse): string {
    const base = $localize`:@@pd.wb.assistant.context.title:What the next question carries before its own text: the conversation the model reads and the tools' definitions.`;
    if (used.level === 'high') return `${base} ${$localize`:@@pd.wb.assistant.context.high:Close to what the model can read.`}`;
    if (used.reserveTokens > 0) return `${base} ${$localize`:@@pd.wb.assistant.context.reserve:${this.tokens(used.reserveTokens)}:n: more are kept for the answer.`}`;
    if (used.level === 'full') return `${base} ${this.fullNote()}`;
    return base;
  }

  /** The folded tray's summary: how many files the next question carries. */
  protected filesCount(): string {
    const n = this.store.attached().length;
    return n === 1 ? $localize`:@@pd.wb.assistant.files.one:1 file` : $localize`:@@pd.wb.assistant.files.many:${n}:count: files`;
  }

  /** Why nothing more can be sent: the chat is too long, or the files waiting in the composer are (review 2 P2-C). */
  protected fullNote(): string {
    return this.store.fullBy() === 'files' ? this.t.filesTooLong : this.t.contextFull;
  }

  /** Moves the keyboard focus to the question (the command `pd.wb.assistant.focus`). */
  focusInput(): void {
    this.inputEl()?.nativeElement.focus();
  }
}
