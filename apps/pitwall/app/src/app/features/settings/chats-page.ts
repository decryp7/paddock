import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';
import { MatButton } from '@angular/material/button';
import { MatSlideToggle, type MatSlideToggleChange } from '@angular/material/slide-toggle';
import { formatBytes } from '@decryp7/paddock-ui/format';
import { CONFIRM_DIALOG_ID } from '@decryp7/paddock-ui/shell/confirm';
import { DialogService } from '@decryp7/paddock-ui/shell/dialogs';
import type { ChatStorage, CommandError } from '../../core/api.types';
import { BACKEND } from '../../core/backend';
import { toCommandError } from '../../core/command-error';
import { Panel } from '../../shared/ui/panel';

/**
 * Settings > Chats (ADR 0023 Decision 3): whether new Playground chats are saved, where and how they are kept, and Delete all
 * chats. Each chat's own "Don't save this chat" switch is in the Chat bar; one chat is deleted from the chat's list. Rust
 * keeps the chats and the setting; this page shows them and sends the user's choices.
 */
@Component({
  selector: 'pw-chats-page',
  imports: [MatButton, MatSlideToggle, Panel],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="page">
      <header class="page-head">
        <h2 class="pd-title">Chats</h2>
        <p class="help" data-testid="chats-privacy">
          Chats are kept on this computer in your account's folder. They are not encrypted; anything that can read your files can read them.
        </p>
      </header>
      <pw-panel [loading]="loading()" [ready]="storage() !== null" [error]="error()" (retry)="load()">
        @if (storage(); as s) {
          <section class="section" aria-label="Chats">
            <mat-slide-toggle data-testid="chats-save-new" [checked]="s.saveNew" [disabled]="busy()" (change)="setSaveNew($event)">Save new chats</mat-slide-toggle>
            <p class="note">Off: a new chat starts with "Don't save this chat" on, and is gone when Pitwall closes. Each chat can change it in the Chat bar.</p>
            <p class="usage" data-testid="chats-usage">{{ usage() }}</p>
            <div class="actions">
              <button type="button" matButton="outlined" data-testid="chats-delete-all" [disabled]="busy() || (s.chats === 0 && s.unsaved === 0)" (click)="deleteAll()">
                Delete all chats…
              </button>
            </div>
          </section>
        }
      </pw-panel>
    </div>
  `,
  styles: `
    .page { display: grid; gap: var(--pd-space-3); max-width: 1120px; }
    .pd-title { margin: 0; font-size: var(--pd-text-subtitle); font-weight: var(--pd-weight-semibold); }
    .help { margin: var(--pd-space-1) 0 0; color: var(--pd-text-muted); }
    .section { display: grid; gap: var(--pd-space-2); }
    .note, .usage { margin: 0; color: var(--pd-text-muted); font-size: var(--pd-text-label); }
    .actions { display: flex; gap: var(--pd-space-2); }
  `,
})
export class ChatsPage {
  private readonly backend = inject(BACKEND);
  private readonly dialogs = inject(DialogService);
  protected readonly storage = signal<ChatStorage | null>(null);
  protected readonly loading = signal(false);
  protected readonly busy = signal(false);
  protected readonly error = signal<CommandError | null>(null);

  /** What the saved chats take, in words. */
  protected readonly usage = computed(() => {
    const s = this.storage();
    if (!s) return '';
    const saved = s.chats === 0 ? 'No saved chats' : `${s.chats} saved ${s.chats === 1 ? 'chat' : 'chats'} (${formatBytes(s.bytes)})`;
    const attached = s.attachmentBytes > 0 ? `, attachments ${formatBytes(s.attachmentBytes)}` : '';
    const unsaved = s.unsaved > 0 ? `; ${s.unsaved} not saved until Pitwall closes` : '';
    return `${saved}${attached}${unsaved}.`;
  });

  constructor() {
    void this.load();
  }

  async load(): Promise<void> {
    this.loading.set(true);
    this.error.set(null);
    try {
      this.storage.set(await this.backend.chatStorage());
    } catch (e) {
      this.error.set(toCommandError(e));
    } finally {
      this.loading.set(false);
    }
  }

  private async act(work: () => Promise<ChatStorage>): Promise<void> {
    this.busy.set(true);
    try {
      this.storage.set(await work());
      this.error.set(null);
    } catch (e) {
      this.error.set(toCommandError(e));
    } finally {
      this.busy.set(false);
    }
  }

  protected setSaveNew(change: MatSlideToggleChange): Promise<void> {
    return this.act(() => this.backend.setChatSaveNew(change.checked));
  }

  /** Asks first, naming what goes. */
  protected async deleteAll(): Promise<void> {
    const s = this.storage();
    const n = (s?.chats ?? 0) + (s?.unsaved ?? 0);
    const outcome = await this.dialogs.open(CONFIRM_DIALOG_ID, {
      title: 'Chats',
      inputs: {
        heading: 'Delete all chats?',
        message: `Pitwall deletes ${n === 1 ? 'the chat' : `all ${n} chats`} and ${n === 1 ? 'its' : 'their'} attachments from this computer. This cannot be undone.`,
        confirm: { label: 'Delete all chats', destructive: true },
      },
    }).closed;
    if (outcome.kind !== 'answered') return;
    await this.act(() => this.backend.deleteAllChats());
  }
}
