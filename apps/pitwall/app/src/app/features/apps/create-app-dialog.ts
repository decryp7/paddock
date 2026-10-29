import { DialogRef } from '@angular/cdk/dialog';
import { ChangeDetectionStrategy, Component, inject, signal } from '@angular/core';

/** Asks for a new app's name; closes with the trimmed name, or nothing on Cancel. */
@Component({
  selector: 'pw-create-app-dialog',
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: { class: 'pw-dialog', role: 'dialog', 'aria-modal': 'true', 'aria-labelledby': 'pw-new-app-title' },
  template: `
    <form (submit)="$event.preventDefault(); done()">
      <h2 id="pw-new-app-title">New app</h2>
      <p class="faint">For a tool that can't pair by itself. You get a key for it next.</p>
      <label for="pw-new-app-name">Name</label>
      <input id="pw-new-app-name" class="input" maxlength="64" placeholder="e.g. Obsidian, Raycast, my script" autocomplete="off" cdkFocusInitial [value]="name()" (input)="name.set($any($event.target).value)" />
      <div class="row">
        <button type="button" class="btn" (click)="ref.close()">Cancel</button>
        <button type="submit" class="btn primary" [disabled]="!name().trim()">Create app</button>
      </div>
    </form>
  `,
  styles: `
    :host {
      display: block;
      width: 420px;
      padding: var(--pd-space-4);
    }
    h2 {
      font-size: var(--pd-text-body-lg);
      margin-bottom: var(--pd-space-1);
    }
    p {
      font-size: var(--pd-text-label);
      margin-bottom: var(--pd-space-4);
    }
    label {
      display: block;
      margin-bottom: 6px;
      font-size: var(--pd-text-label);
      color: var(--pd-text-muted);
    }
    .input {
      width: 100%;
    }
    .row {
      display: flex;
      justify-content: flex-end;
      gap: var(--pd-space-2);
      margin-top: var(--pd-space-4);
    }
  `,
})
export class CreateAppDialog {
  protected readonly ref = inject<DialogRef<string>>(DialogRef);
  protected readonly name = signal('');

  protected done(): void {
    const n = this.name().trim();
    if (n) this.ref.close(n);
  }
}
