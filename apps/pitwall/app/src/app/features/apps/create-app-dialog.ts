import { ChangeDetectionStrategy, Component, computed, signal } from '@angular/core';
import { MatFormField, MatHint, MatLabel } from '@angular/material/form-field';
import { MatInput } from '@angular/material/input';
import type { DialogAction, DialogContent } from '@decryp7/paddock-ui/shell/dialogs';

/** The id of the "New app" dialog (`src-tauri/src/dialogs.rs`). */
export const CREATE_APP_DIALOG_ID = 'pw.create-app';

/** The longest app name the server accepts. */
export const MAX_APP_NAME = 64;

/**
 * `pw.create-app`: asks for a new app's name and answers it trimmed. The footer is the frame's: Cancel and "Create app",
 * disabled until there is a name; the focus starts in the name field and Enter creates. The opener creates the app and then
 * shows its key (rule 17, ADR 0026 §2).
 */
@Component({
  selector: 'pw-create-app-dialog',
  imports: [MatFormField, MatLabel, MatHint, MatInput],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <p class="msg">For a tool that can't pair by itself. You get a key for it next.</p>
    <mat-form-field class="name">
      <mat-label>Name</mat-label>
      <input matInput id="pw-new-app-name" [attr.maxlength]="max" placeholder="e.g. Obsidian, Raycast, my script" autocomplete="off" spellcheck="false" [value]="name()" (input)="name.set($any($event.target).value)" />
      <mat-hint align="end">{{ name().length }} / {{ max }}</mat-hint>
    </mat-form-field>
  `,
  styles: `
    :host { display: flex; flex-direction: column; gap: var(--pd-space-3); min-width: 0; }
    .msg { margin: 0; color: var(--pd-text-muted); }
    .name { width: 100%; }
  `,
})
export class CreateAppDialog implements DialogContent<string> {
  protected readonly max = MAX_APP_NAME;
  protected readonly name = signal('');
  private readonly trimmed = computed(() => this.name().trim());

  /** The window is titled "New app"; the heading says what this form does. */
  readonly dialogHeading = signal<string | null>('Add an app by hand');

  readonly dialogActions = computed<readonly DialogAction<string>[]>(() => {
    const name = this.trimmed();
    return [
      { kind: 'dismiss', id: 'cancel', label: 'Cancel' },
      { kind: 'answer', id: 'create', label: 'Create app', role: 'primary', enabled: name.length > 0 && name.length <= MAX_APP_NAME, answer: () => name },
    ];
  });
}
