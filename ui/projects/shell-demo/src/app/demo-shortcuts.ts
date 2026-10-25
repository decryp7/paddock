import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';
import { CommandService } from '@decryp7/paddock-ui/shell';
import { describeConflict, formatKey, Keymap, recordKey } from '@decryp7/paddock-ui/shell/menu';

/**
 * The demo's keyboard shortcuts (plan S13): every command with its effective key, Change (records the next key press),
 * Remove and Reset. The overrides persist; a key another command already uses is refused with its name. The Settings
 * page of the shell (plan S10) will host the same thing.
 */
@Component({
  selector: 'sd-shortcuts',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <table class="keys">
      <caption>Keyboard shortcuts</caption>
      <thead>
        <tr><th scope="col">Command</th><th scope="col">Key</th><th scope="col"><span class="sr">Actions</span></th></tr>
      </thead>
      <tbody>
        @for (row of rows(); track row.id) {
          <tr [attr.data-command]="row.id">
            <td [id]="'sd-key-' + row.id">{{ row.title }}</td>
            <td class="key" data-testid="shortcut-key">{{ recording() === row.id ? 'Press a key…' : row.key || '—' }}</td>
            <td class="actions">
              <button type="button" class="btn" [attr.aria-describedby]="'sd-key-' + row.id" [attr.aria-pressed]="recording() === row.id"
                (click)="record(row.id, $event)" (keydown)="onKey($event, row.id)" (blur)="stop()">Change key</button>
              <button type="button" class="btn" [attr.aria-describedby]="'sd-key-' + row.id" [disabled]="!row.overridden" (click)="keymap.reset(row.id)">Reset key</button>
            </td>
          </tr>
        }
      </tbody>
    </table>
    <p class="message" role="status" data-testid="shortcut-message">{{ message() }}</p>
    @for (c of conflicts(); track $index) {
      <p class="conflict" role="alert">{{ c }}</p>
    }
  `,
  styles: `
    :host { display: flex; flex-direction: column; gap: var(--pd-space-2); min-width: 0; }
    .keys { width: 100%; border-collapse: collapse; font-size: var(--pd-text-body); }
    .keys caption { text-align: start; font-weight: var(--pd-weight-semibold); padding-block-end: var(--pd-space-2); }
    .keys th, .keys td { text-align: start; padding: var(--pd-space-1) var(--pd-space-2); border-bottom: 1px solid var(--pd-hairline); overflow-wrap: anywhere; }
    .keys th { color: var(--pd-text-muted); font-weight: var(--pd-weight-medium); }
    .key { white-space: nowrap; }
    .actions { display: flex; flex-wrap: wrap; gap: var(--pd-space-1); }
    .btn { min-height: var(--pd-button-height); padding: 0 var(--pd-space-2); border-radius: var(--pd-radius-control); border: 1px solid var(--pd-border-control); background: var(--pd-surface-2); color: var(--pd-text); font: inherit; }
    .btn[aria-pressed='true'] { border-color: var(--pd-accent); }
    .btn:disabled { color: var(--pd-text-subtle); }
    .message, .conflict { margin: 0; min-height: 1lh; overflow-wrap: anywhere; }
    .conflict { color: var(--pd-danger); }
    .sr { position: absolute; width: 1px; height: 1px; overflow: hidden; clip-path: inset(50%); white-space: nowrap; }
  `,
})
export class DemoShortcuts {
  protected readonly keymap = inject(Keymap);
  private readonly commands = inject(CommandService);
  protected readonly recording = signal<string | null>(null);
  protected readonly message = signal('');

  protected readonly rows = computed(() => {
    const overrides = this.keymap.overrides();
    return this.commands.available().map((c) => {
      const key = this.keymap.keyFor(c.id);
      return { id: c.id, title: c.title.text, key: key ? formatKey(key, this.keymap.platform) : '', overridden: Object.hasOwn(overrides, c.id) };
    });
  });
  protected readonly conflicts = computed(() => this.keymap.conflicts().map(describeConflict));

  protected record(id: string, click: Event): void {
    // Safari and WKWebView (macOS) do not focus a button on click, and the keys go to the focused button: focus it here.
    if (click.currentTarget instanceof HTMLElement) click.currentTarget.focus();
    this.recording.set(id);
    this.message.set('');
    this.keymap.suspend();
  }

  protected stop(): void {
    this.recording.set(null);
    this.keymap.resume();
  }

  protected onKey(e: KeyboardEvent, id: string): void {
    if (this.recording() !== id) return;
    e.preventDefault();
    e.stopPropagation();
    const r = recordKey(e, this.keymap.platform);
    if (r.kind === 'wait') return;
    if (r.kind === 'invalid') {
      this.message.set('That key cannot be used alone: add Ctrl, Alt or Cmd (or use F1-F24).');
      return;
    }
    const title = this.rows().find((row) => row.id === id)?.title ?? id;
    if (r.kind === 'binding') {
      const result = this.keymap.bind(id, r.binding);
      if (!result.ok && result.reason === 'conflict') {
        const names = [...result.commands.map((b) => this.commands.find(b.command)?.title.text ?? b.command), ...result.reserved.map((x) => x.description)];
        this.message.set(`${formatKey(r.binding, this.keymap.platform)} is already used by ${names.join(', ')}; ${title} keeps its key.`);
      } else if (result.ok) this.message.set(`${title} is now ${formatKey(r.binding, this.keymap.platform)}.`);
      else this.message.set('That is not a key binding.');
    } else if (r.kind === 'clear') {
      this.keymap.bind(id, null);
      this.message.set(`${title} has no key now.`);
    }
    this.stop();
  }
}
