import { ChangeDetectionStrategy, Component, computed, DestroyRef, inject, signal } from '@angular/core';
import { MatButton } from '@angular/material/button';
import { MatFormField, MatHint, MatLabel } from '@angular/material/form-field';
import { MatOption, MatSelect } from '@angular/material/select';
import { OPEN_SETTINGS } from '@decryp7/paddock-ui/shell/notify';
import { AssistantBackend, PdAssistant } from '@decryp7/paddock-ui/workbench/assistant';
import type { ChatModels, CommandError, ConnectedApp } from '../../core/api.types';
import { BACKEND } from '../../core/backend';
import { toCommandError } from '../../core/command-error';
import { createChatBackend } from './chat-backend';

/**
 * The Playground's Chat tab: the family's assistant pane (`<pd-assistant>`: streamed answers, tool cards, approval cards,
 * Stop) over the chat engine, with the model picker (ADR 0023 D6: models that cannot use tools are listed but disabled, with
 * the reason; Rust chooses a tool-capable default) and what is connected. All decisions are Rust's; this only places them.
 * Pitwall is the engine itself, so the pane's "needs local AI" state is not provided, and it has no "Allow actions" tick: an
 * action is gated by the connected app's own allow and by Pitwall asking on every call (ADR 0023 revision 3).
 */
@Component({
  selector: 'pw-playground-chat',
  imports: [PdAssistant, MatFormField, MatLabel, MatHint, MatSelect, MatOption, MatButton],
  providers: [{ provide: AssistantBackend, useFactory: createChatBackend }],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="bar">
      <mat-form-field class="model" subscriptSizing="dynamic">
        <mat-label>Model</mat-label>
        <mat-select data-testid="chat-model" panelClass="pw-chat-model-panel" [value]="picker()?.selected ?? null" [disabled]="!picker()" (selectionChange)="choose($event.value)">
          @for (m of picker()?.models ?? []; track m.id) {
            <mat-option [value]="m.id" [disabled]="m.disabled !== null"><span class="opt-name">{{ m.name }}</span>@if (m.disabled) { <span class="why">{{ m.disabled }}</span> }</mat-option>
          }
        </mat-select>
        @if (picker()?.note; as note) {
          <mat-hint data-testid="chat-model-note">{{ note }}</mat-hint>
        }
      </mat-form-field>
      <p class="tools" data-testid="chat-apps" role="status">{{ appsLine() }}</p>
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
      emptyAction="Connected apps…"
      [emptyHint]="emptyHint"
      [failedCause]="failedCause"
      (emptyActionClick)="openApps()"
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
    ::ng-deep .pw-chat-model-panel .mat-mdc-option.mdc-list-item--disabled { opacity: 1; }
    ::ng-deep .pw-chat-model-panel .mat-mdc-option.mdc-list-item--disabled .mdc-list-item__primary-text { opacity: 1; color: var(--pd-text-muted); }
    ::ng-deep .pw-chat-model-panel .why { margin-left: var(--pd-space-2); color: var(--pd-text-muted); opacity: 1; }
    .tools { margin: 0; flex: 1; min-width: 12ch; color: var(--pd-text-muted); font-size: var(--pd-text-label); }
    .problem { margin: 0; color: var(--pd-text); }
    .assistant { flex: 1; min-height: 0; border: 1px solid var(--pd-hairline); border-radius: var(--pd-radius-card); box-shadow: var(--pd-shadow-1); overflow: hidden; }
  `,
})
export class PlaygroundChat {
  private readonly backend = inject(BACKEND);
  private readonly openSettings = inject(OPEN_SETTINGS, { optional: true });
  protected readonly picker = signal<ChatModels | null>(null);
  protected readonly apps = signal<ConnectedApp[] | null>(null);
  protected readonly problem = signal<CommandError | null>(null);
  /** The empty chat: what it can do here (a connected app lends it tools). */
  protected readonly emptyHint = 'Ask a question. Apps you connect lend the model their tools, and every tool call shows here.';
  /** Pitwall is the engine: a failed answer is the model's, not a missing Pitwall. */
  protected readonly failedCause = 'Likely cause: the model stopped answering. Try again, or choose another model.';

  /** What the chat can use now, in words: the connected apps and how many tools they lend. */
  protected readonly appsLine = computed(() => {
    const apps = this.apps();
    if (!apps) return '';
    const on = apps.filter((a) => a.connected);
    if (!on.length) return 'No app connected: the chat answers without tools.';
    return 'Tools from ' + on.map((a) => `${a.title} (${a.tools.length})`).join(', ') + '.';
  });

  constructor() {
    void this.load();
    // Settings is another window: a connect made there reaches this tab through Rust's event.
    const stop = this.backend.onConnectedApps((apps) => this.apps.set(apps));
    inject(DestroyRef).onDestroy(stop);
  }

  async load(): Promise<void> {
    this.problem.set(null);
    try {
      const [picker, apps] = await Promise.all([this.backend.chatModels(), this.backend.connectedApps()]);
      this.picker.set(picker);
      this.apps.set(apps);
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

  protected openApps(): void {
    void this.openSettings?.('pw.connected-apps');
  }
}
