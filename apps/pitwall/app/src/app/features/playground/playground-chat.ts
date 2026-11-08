import { afterNextRender, ChangeDetectionStrategy, Component, computed, DestroyRef, ElementRef, inject, signal, viewChild } from '@angular/core';
import { MatButton } from '@angular/material/button';
import { MatFormField, MatLabel } from '@angular/material/form-field';
import { MatOptgroup, MatOption, MatSelect } from '@angular/material/select';
import { MAT_SLIDE_TOGGLE_DEFAULT_OPTIONS, MatSlideToggle } from '@angular/material/slide-toggle';
import { CONFIRM_DIALOG_ID } from '@decryp7/paddock-ui/shell/confirm';
import { DialogService } from '@decryp7/paddock-ui/shell/dialogs';
import { OPEN_SETTINGS } from '@decryp7/paddock-ui/shell/notify';
import { ASSISTANT_SHORT_PANE_PX, AssistantAttachments, AssistantBackend, AssistantSpeech, PdAssistant } from '@decryp7/paddock-ui/workbench/assistant';
import type { ChatModel, ChatModels, CommandError, ConnectedApp } from '../../core/api.types';
import { BACKEND } from '../../core/backend';
import { toCommandError } from '../../core/command-error';
import { PitwallAttachments } from './chat-attachments';
import { ChatSaving, createChatBackend, createChatSpeech } from './chat-backend';

/**
 * The Playground's Chat tab: the family's assistant pane (`<pd-assistant>`: streamed answers, tool cards, approval cards,
 * Stop) over the chat engine, with the model picker (ADR 0023 D6: models that cannot use tools are listed but disabled, with
 * the reason; Rust chooses a tool-capable default) and what is connected. All decisions are Rust's; this only places them.
 * Pitwall is the engine itself, so the pane's "needs local AI" state is not provided, and it has no "Allow actions" tick: an
 * action is gated by the connected app's own allow and by Pitwall asking on every call (ADR 0023 revision 3).
 */
@Component({
  selector: 'pw-playground-chat',
  imports: [PdAssistant, MatFormField, MatLabel, MatSelect, MatOption, MatOptgroup, MatButton, MatSlideToggle],
  providers: [
    ChatSaving,
    { provide: AssistantBackend, useFactory: createChatBackend },
    // Text and code files (ADR 0023 Decision 4, WP-4b); Rust checks and keeps them.
    { provide: AssistantAttachments, useClass: PitwallAttachments },
    // Speech input: none in the app until WP-7 (null: the pane shows no microphone); the browser build can play it.
    { provide: AssistantSpeech, useFactory: () => createChatSpeech() },
    // The switch moves only when the chat's state does (after "Delete the saved copy?" is answered), never on the click.
    { provide: MAT_SLIDE_TOGGLE_DEFAULT_OPTIONS, useValue: { disableToggleValue: true } },
  ],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="bar">
      <mat-form-field class="model" subscriptSizing="dynamic">
        <mat-label>Model</mat-label>
        <mat-select data-testid="chat-model" panelClass="pw-chat-model-panel" [value]="picker()?.selected ?? null" [disabled]="!picker()" (selectionChange)="choose($event.value)">
          @for (m of usable(); track m.id) {
            <mat-option [value]="m.id">{{ m.name }}</mat-option>
          }
          <!-- Why these can't be chosen is said once per reason, inside the open list (not under the field, where it moved the bar). -->
          @for (g of groups(); track g.reason) {
            <mat-optgroup [label]="g.note" data-testid="chat-model-note">
              @for (m of g.models; track m.id) {
                <mat-option [value]="m.id" disabled>{{ m.name }}<span class="why"> · {{ m.disabled }}</span></mat-option>
              }
            </mat-optgroup>
          }
        </mat-select>
      </mat-form-field>
      <p class="tools" data-testid="chat-apps" role="status">{{ appsLine() }}</p>
      <!-- ADR 0023 Decision 3: per chat; a new chat starts with Settings > Chats' default. -->
      <mat-slide-toggle data-testid="chat-dont-save" [checked]="!saved()" [disabled]="moving()" (toggleChange)="dontSave()">Don't save this chat</mat-slide-toggle>
      <button type="button" matButton data-testid="chat-open-apps" (click)="openApps()">Connected apps…</button>
    </div>
    @if (problem(); as p) {
      <p class="problem" role="alert" data-testid="chat-problem">{{ p.message }} <button type="button" matButton (click)="load()">Try again</button></p>
    } @else if (picker() && !picker()!.selected) {
      <p class="problem" role="alert" data-testid="chat-no-tool-model">No downloaded model can use tools. Download one in Models (the Qwen models can), then come back.</p>
    }
    <pd-assistant
      class="assistant"
      actionsGate="per-call"
      emptyTitle="Chat with a local model"
      [emptyAction]="anyConnected() || short() ? null : 'Connected apps…'"
      [emptyHint]="emptyHint"
      [failedCause]="failedCause"
      (emptyActionClick)="openApps()"
      (activeChange)="onActive($event)"
    />
  `,
  styles: `
    :host { display: flex; flex-direction: column; gap: var(--pd-space-2); height: 100%; min-height: 0; }
    .bar { display: flex; align-items: center; gap: var(--pd-space-3); flex-wrap: wrap; }
    .model { width: 360px; max-width: 100%; }
    /*
     * A model that can't use tools stays disabled (aria-disabled), but its name and reason are read at full opacity in the
     * muted text colour, never Material's faded disabled text (UI review: 1.86:1); the options get room between them.
     */
    ::ng-deep .pw-chat-model-panel .mat-mdc-option { min-height: 44px; padding-block: var(--pd-space-1); }
    /*
     * More is below (the disabled groups): the list fades at its bottom edge, wherever the rows fall, and its padding below the
     * last row is the fade's height, so scrolled to the end the last row is clear of it.
     */
    ::ng-deep div.mat-mdc-select-panel.pw-chat-model-panel {
      max-height: 286px;
      padding-bottom: var(--pw-fade);
      mask-image: linear-gradient(to bottom, #000 calc(100% - var(--pw-fade)), transparent);
    }
    ::ng-deep .pw-chat-model-panel .mat-mdc-option.mdc-list-item--disabled { opacity: 1; }
    ::ng-deep .pw-chat-model-panel .mat-mdc-option.mdc-list-item--disabled .mdc-list-item__primary-text { opacity: 1; color: var(--pd-text-muted); }
    ::ng-deep .pw-chat-model-panel .why { color: var(--pd-text-muted); opacity: 1; }
    ::ng-deep .pw-chat-model-panel .mat-mdc-optgroup-label { opacity: 1; color: var(--pd-text-muted); font-size: var(--pd-text-label); white-space: normal; line-height: 1.4; padding-block: var(--pd-space-2); }
    .tools { margin: 0; flex: 1; min-width: 12ch; color: var(--pd-text-muted); font-size: var(--pd-text-label); }
    .problem { margin: 0; color: var(--pd-text); }
    .assistant { flex: 1; min-height: 0; border: 1px solid var(--pd-hairline); border-radius: var(--pd-radius-card); box-shadow: var(--pd-shadow-1); overflow: hidden; }
  `,
})
export class PlaygroundChat {
  private readonly backend = inject(BACKEND);
  private readonly openSettings = inject(OPEN_SETTINGS, { optional: true });
  private readonly dialogs = inject(DialogService);
  private readonly saving = inject(ChatSaving);
  /** The chat open in the pane (null: a new one). */
  private readonly active = signal<string | null>(null);
  /** Settings > Chats' default for new chats. */
  private readonly saveNew = signal(true);
  /** The open chat is saved (for a new chat: whether it will be). */
  protected readonly saved = signal(true);
  /** A chat is being moved between saved and not saved. */
  protected readonly moving = signal(false);
  protected readonly picker = signal<ChatModels | null>(null);
  protected readonly apps = signal<ConnectedApp[] | null>(null);
  protected readonly problem = signal<CommandError | null>(null);
  /** The empty chat: what it can do here (a connected app lends it tools). */
  protected readonly emptyHint = 'Ask a question. Apps you connect lend the model their tools, and every tool call shows here.';
  /** Pitwall is the engine: a failed answer is the model's, not a missing Pitwall. */
  protected readonly failedCause = 'Likely cause: the model stopped answering. Try again, or choose another model.';

  /** Models the chat can use (Rust's order). */
  protected readonly usable = computed(() => (this.picker()?.models ?? []).filter((m) => m.disabled === null));
  /** Models listed but not choosable, grouped by Rust's reason (in Rust's order), each group labelled by its note. */
  protected readonly groups = computed(() => {
    const p = this.picker();
    const out: { reason: string; note: string; models: ChatModel[] }[] = [];
    for (const m of p?.models ?? []) {
      if (m.disabled === null) continue;
      let g = out.find((x) => x.reason === m.disabled);
      if (!g) out.push((g = { reason: m.disabled, note: p?.notes[m.disabled] ?? m.disabled, models: [] }));
      g.models.push(m);
    }
    return out;
  });
  /** The chat pane is short (the pane's own fold height): the empty chat leaves "Connected apps…" to the bar above it, so the
   *  log keeps its room (WP-4b reviews 2 and 3). */
  protected readonly short = signal(false);
  private readonly pane = viewChild(PdAssistant, { read: ElementRef<HTMLElement> });
  /** Some app lends its tools now (the empty chat then needs no "Connected apps…"). */
  protected readonly anyConnected = computed(() => (this.apps() ?? []).some((a) => a.connected));

  /** What the chat can use now, in words: the connected apps and how many tools they lend. */
  protected readonly appsLine = computed(() => {
    const apps = this.apps();
    if (!apps) return '';
    const on = apps.filter((a) => a.connected);
    const back = apps.filter((a) => a.reconnecting);
    if (!on.length && back.length) return `${back.map((a) => a.title).join(', ')} ${back.length === 1 ? 'is' : 'are'} reconnecting: the chat answers without ${back.length === 1 ? 'its' : 'their'} tools for now.`;
    // Only the user can help: say what is missing (Rust's needs), not just "no app".
    const missing = apps.find((a) => a.needs === 'install');
    if (!on.length && missing) return `${missing.title} isn't installed: the chat answers without its tools.`;
    const old = apps.find((a) => a.needs === 'update');
    if (!on.length && old) return `${old.title} needs an update: the chat answers without its tools.`;
    if (!on.length) return 'No app connected: the chat answers without tools.';
    return 'Tools from ' + on.map((a) => `${a.title} (${a.tools.length})`).join(', ') + '.';
  });

  constructor() {
    void this.load();
    // Settings is another window: a connect made there reaches this tab through Rust's event.
    const stop = this.backend.onConnectedApps((apps) => this.apps.set(apps));
    const destroy = inject(DestroyRef);
    destroy.onDestroy(stop);
    // The pane's own height decides, as the pane's folding does (DOM only: a measurement, not a signal).
    afterNextRender(() => {
      const pane = this.pane()?.nativeElement;
      if (!pane || typeof ResizeObserver === 'undefined') return;
      const watch = new ResizeObserver(() => this.short.set(pane.clientHeight > 0 && pane.clientHeight < ASSISTANT_SHORT_PANE_PX));
      watch.observe(pane);
      destroy.onDestroy(() => watch.disconnect());
    });
  }

  async load(): Promise<void> {
    this.problem.set(null);
    try {
      const [picker, apps, storage] = await Promise.all([this.backend.chatModels(), this.backend.connectedApps(), this.backend.chatStorage()]);
      this.picker.set(picker);
      this.apps.set(apps);
      this.saveNew.set(storage.saveNew);
      if (this.active() === null) this.setNext(storage.saveNew);
    } catch (e) {
      this.problem.set(toCommandError(e));
    }
  }

  protected async choose(id: string): Promise<void> {
    try {
      this.picker.set(await this.backend.setChatModel(id));
    } catch (e) {
      this.problem.set(toCommandError(e));
    }
  }

  private setNext(save: boolean): void {
    this.saving.saveNext.set(save);
    this.saved.set(save);
  }

  /** The pane opened a chat, or a new one: the switch shows that chat's state (Rust's, asked when this tab did not start it). */
  protected async onActive(id: string | null): Promise<void> {
    this.active.set(id);
    if (id === null) {
      this.setNext(this.saveNew());
      // Settings > Chats may have changed the default since (another window).
      try {
        const storage = await this.backend.chatStorage();
        this.saveNew.set(storage.saveNew);
        if (this.active() === null) this.setNext(storage.saveNew);
      } catch (e) {
        this.problem.set(toCommandError(e));
      }
      return;
    }
    const started = this.saving.started.get(id);
    if (started !== undefined) return this.saved.set(started);
    try {
      const saved = await this.backend.chatSaved(id);
      if (this.active() === id) this.saved.set(saved);
    } catch (e) {
      this.problem.set(toCommandError(e));
    }
  }

  /**
   * "Don't save this chat". A new chat only takes the choice to its first question. A saved chat asks before its saved copy is
   * deleted (no keeps it saved); turning the switch off saves the chat at once.
   */
  protected async dontSave(): Promise<void> {
    const save = !this.saved();
    const id = this.active();
    if (id === null) return this.setNext(save);
    if (!save) {
      const outcome = await this.dialogs.open(CONFIRM_DIALOG_ID, {
        title: 'Playground',
        inputs: {
          heading: 'Delete the saved copy of this chat?',
          message: 'Pitwall deletes this chat from this computer. It stays open here until Pitwall closes, then it is gone.',
          confirm: { label: 'Delete saved copy', destructive: true },
        },
      }).closed;
      if (outcome.kind !== 'answered') return;
    }
    this.moving.set(true);
    try {
      this.saved.set(await this.backend.setChatSaved(id, save));
      this.saving.started.set(id, this.saved());
    } catch (e) {
      this.problem.set(toCommandError(e));
    } finally {
      this.moving.set(false);
    }
  }

  protected openApps(): void {
    void this.openSettings?.('pw.connected-apps');
  }
}
