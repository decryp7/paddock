import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';
import { SuEmptyState } from '@decryp7/paddock-ui';
import { CommandService, ContributionRegistry, ShellContextHost, type DialogDef } from '@decryp7/paddock-ui/shell';
import { DialogHost, DialogService } from '@decryp7/paddock-ui/shell/dialogs';
import { PdJobsStatus } from '@decryp7/paddock-ui/shell/jobs';
import { PdContextMenu, PdMenuBar } from '@decryp7/paddock-ui/shell/menu';
import { DemoShortcuts } from './demo-shortcuts';
import type { DemoContext } from './demo-context';
import { PdNotificationsStatus, PdServiceStatus } from '@decryp7/paddock-ui/shell/notify';
import { EnginePanel } from './engine-panel';
import { DemoNote } from './demo-note';

/** One part of the shell and the work package of docs/plans/workbench.md that brings it to this app. */
interface ShellPart {
  readonly name: string;
  readonly entry: string;
  readonly pkg: string;
}

const PARTS: readonly ShellPart[] = [
  { name: 'Contributions and context', entry: 'shell', pkg: 'S2' },
  { name: 'Notifications and service health', entry: 'shell/notify', pkg: 'S4' },
  { name: 'Dialogs as windows', entry: 'shell/dialogs', pkg: 'S6' },
  { name: 'Start-up contract', entry: 'shell/startup', pkg: 'S7' },
  { name: 'Jobs', entry: 'shell/jobs', pkg: 'S9' },
  { name: 'Settings', entry: 'shell/settings', pkg: 'S10' },
  { name: 'About, Updates, What’s New', entry: 'shell/about', pkg: 'S11' },
  { name: 'Menus and keys', entry: 'shell/menu', pkg: 'S13' },
  { name: 'Pages frame', entry: 'shell/pages', pkg: 'S14' },
  { name: 'Theme and i18n', entry: 'shell/i18n', pkg: 'S15' },
  { name: 'Test kit', entry: 'shell/testing', pkg: 'S16' },
];

/**
 * The reference app of the Paddock app shell (ADR 0012 §7) on the pages frame, against the mock backend. It is the first
 * consumer of every shell change: each work package of the plan adds its part here, with its e2e.
 */
@Component({
  selector: 'sd-root',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [SuEmptyState, PdJobsStatus, PdServiceStatus, PdNotificationsStatus, EnginePanel, DialogHost, PdMenuBar, PdContextMenu, DemoShortcuts],
  template: `
    <!-- The title bar region the pages frame (plan S14) will own: the in-app menu bar (hidden in the macOS app). -->
    <header class="title-bar">
      <pd-menu-bar />
    </header>
    <main class="page" [class.compact]="compact()">
      <h1>Paddock shell demo</h1>
      <pd-empty-state
        heading="The shell arrives here part by part"
        description="Each part below is built into the shell by its work package and shown in this app."
      />
      <section class="contributions" aria-labelledby="sd-contributions">
        <h2 id="sd-contributions">Contributions</h2>
        <p class="context" data-testid="app-context">App context: {{ contextLoaded() ? 'loaded' : 'not loaded yet' }}</p>
        <div class="commands" role="group" aria-label="Commands">
          @for (command of available(); track command.id) {
            <button type="button" class="cmd" [attr.data-command]="command.id" [disabled]="!commands.isEnabled(command)" (click)="run(command.id)">
              {{ command.title.text }}
            </button>
          }
        </div>
        <table class="kinds">
          <caption>Registered contributions</caption>
          <thead>
            <tr><th scope="col">Kind</th><th scope="col">Count</th></tr>
          </thead>
          <tbody>
            @for (kind of kinds; track kind.name) {
              <tr><td><code>{{ kind.name }}</code></td><td>{{ kind.count }}</td></tr>
            }
          </tbody>
        </table>
      </section>
      <section class="menus" aria-labelledby="sd-menus">
        <h2 id="sd-menus">Menus and keys</h2>
        <p class="context">The menu bar above and the greetings' context menu come from the contributions; the keys below can be changed and are kept.</p>
        <pd-context-menu menu="sd.greetings" label="Greeting actions">
          <p class="greetings" data-testid="greetings">Greetings: {{ greetings() }}</p>
        </pd-context-menu>
        <sd-shortcuts />
      </section>
      <sd-engine-panel />
      <section class="startup" aria-labelledby="sd-startup">
        <h2 id="sd-startup">Start-up contract</h2>
        <p class="context">The window reports ready after its first paint and answers liveness pings; this note is kept if the shell reloads the window to recover it.</p>
        <label class="note">
          <span>Note</span>
          <input #noteField type="text" data-testid="demo-note" [value]="note()" (input)="note.set(noteField.value)" />
        </label>
      </section>
      <section class="dialogs" aria-labelledby="sd-dialogs">
        <h2 id="sd-dialogs">Dialogs</h2>
        <p class="context">One of each kind. In the desktop build each is an OS window; in the browser build they open in the page.</p>
        <div class="commands" role="group" aria-label="Open a dialog">
          @for (d of dialogs; track d.id) {
            <button type="button" class="cmd" [attr.data-dialog-open]="d.id" (click)="openDialog(d)">
              {{ d.title.text }} <span class="kind">({{ d.kind }}{{ d.requiredChoice ? ', required choice' : '' }})</span>
            </button>
          }
        </div>
        <p class="context" aria-live="polite" data-testid="dialog-answer">{{ answer() }}</p>
      </section>
      <table class="parts">
        <caption>Shell parts</caption>
        <thead>
          <tr><th scope="col">Part</th><th scope="col">Entry point</th><th scope="col">Package</th></tr>
        </thead>
        <tbody>
          @for (part of parts; track part.entry) {
            <tr><td>{{ part.name }}</td><td><code>&#64;decryp7/paddock-ui/{{ part.entry }}</code></td><td>{{ part.pkg }}</td></tr>
          }
        </tbody>
      </table>
    </main>
    <!-- The status bar the pages frame (plan S14) will own: the service chip (S4), the jobs (S9) and the notifications (S4). -->
    <footer class="status-bar" aria-label="Status bar">
      <pd-service-status />
      <pd-jobs-status />
      <pd-notifications-status />
    </footer>
    <pd-dialog-host />
  `,
  styles: `
    :host { display: flex; flex-direction: column; min-height: 100vh; }
    .status-bar { position: sticky; bottom: 0; display: flex; justify-content: flex-end; align-items: center; gap: var(--pd-space-2); flex-wrap: wrap; min-width: 0; margin-top: auto; padding: var(--pd-space-1) var(--pd-space-4); background: var(--pd-surface-1); border-top: 1px solid var(--pd-hairline); }
    .page { width: 100%; max-width: var(--pd-content-max); margin: 0 auto; padding: var(--pd-space-6) var(--pd-space-4); display: flex; flex-direction: column; gap: var(--pd-space-4); }
    h1 { margin: 0; font-size: var(--pd-text-title); }
    .parts { width: 100%; border-collapse: collapse; font-size: var(--pd-text-body); }
    .parts caption { text-align: start; font-weight: var(--pd-weight-semibold); padding-block-end: var(--pd-space-2); }
    .parts th, .parts td { text-align: start; padding: var(--pd-space-1) var(--pd-space-2); border-bottom: 1px solid var(--pd-hairline); }
    .parts th { color: var(--pd-text-muted); font-weight: var(--pd-weight-medium); }
    .parts code { overflow-wrap: anywhere; }
    .contributions { display: flex; flex-direction: column; gap: var(--pd-space-3); }
    .contributions h2 { margin: 0; font-size: var(--pd-text-subtitle); }
    .context { margin: 0; color: var(--pd-text-muted); }
    .commands { display: flex; flex-wrap: wrap; gap: var(--pd-space-2); }
    .cmd { min-height: var(--pd-button-height); padding: 0 var(--pd-space-3); border-radius: var(--pd-radius-control); border: 1px solid var(--pd-border-control); background: var(--pd-surface-2); color: var(--pd-text); font-weight: var(--pd-weight-medium); }
    .cmd:disabled { color: var(--pd-text-subtle); }
    .dialogs { display: flex; flex-direction: column; gap: var(--pd-space-3); }
    .dialogs h2 { margin: 0; font-size: var(--pd-text-subtitle); }
    .kind { color: var(--pd-text-muted); font-weight: var(--pd-weight-regular, 400); }
    .kinds { width: 100%; border-collapse: collapse; font-size: var(--pd-text-body); }
    .kinds caption { text-align: start; font-weight: var(--pd-weight-semibold); padding-block-end: var(--pd-space-2); }
    .kinds th, .kinds td { text-align: start; padding: var(--pd-space-1) var(--pd-space-2); border-bottom: 1px solid var(--pd-hairline); }
    .kinds th { color: var(--pd-text-muted); font-weight: var(--pd-weight-medium); }
    .title-bar { display: flex; align-items: center; min-width: 0; padding: var(--pd-space-1) var(--pd-space-2); background: var(--pd-surface-1); border-bottom: 1px solid var(--pd-hairline); }
    .page.compact { gap: var(--pd-space-2); padding-block: var(--pd-space-3); }
    .greetings { margin: 0; padding: var(--pd-space-2); border: 1px dashed var(--pd-border-control); border-radius: var(--pd-radius-control); }
    .menus { display: flex; flex-direction: column; gap: var(--pd-space-2); }
    .menus h2 { margin: 0; font-size: var(--pd-text-subtitle); }
    .startup { display: flex; flex-direction: column; gap: var(--pd-space-2); }
    .startup h2 { margin: 0; font-size: var(--pd-text-subtitle); }
    .note { display: flex; flex-direction: column; gap: var(--pd-space-1); max-width: 40ch; }
    .note input { min-height: var(--pd-button-height); padding: 0 var(--pd-space-2); border-radius: var(--pd-radius-control); border: 1px solid var(--pd-border-control); background: var(--pd-surface-1); color: var(--pd-text); font: inherit; min-width: 0; }
  `,
})
export class ShellDemo {
  protected readonly parts = PARTS;
  protected readonly commands = inject(CommandService);
  private readonly registry = inject(ContributionRegistry);
  private readonly host = inject(ShellContextHost);
  protected readonly note = inject(DemoNote).note;
  protected readonly available = this.commands.available;
  protected readonly contextLoaded = computed(() => this.host.context() !== null);
  /** The View > Compact layout toggle and the greetings count, read through the app context (not in the first-paint bundle). */
  protected readonly compact = computed(() => (this.host.context() as Partial<DemoContext> | null)?.compact?.() ?? false);
  protected readonly greetings = computed(() => (this.host.context() as Partial<DemoContext> | null)?.greetings?.() ?? 0);
  protected readonly kinds = this.registry.kinds.map((k) => ({ name: k.name, count: this.registry.items(k.name).length }));

  private readonly dialogService = inject(DialogService);
  protected readonly dialogs = this.registry.dialogs();
  protected readonly answer = signal('No dialog answered yet.');

  protected openDialog(d: DialogDef): void {
    const inputs = d.id === 'sd.rename' ? { current: 'Shell demo' } : {};
    void this.dialogService.open(d.id, { inputs }).closed.then((r) => this.answer.set(`${d.title.text}: ${r === undefined ? 'dismissed' : JSON.stringify(r)}`));
  }

  protected run(id: string): void {
    void this.commands.run(id);
  }
}
