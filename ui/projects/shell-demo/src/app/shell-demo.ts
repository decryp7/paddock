import { ChangeDetectionStrategy, Component, computed, inject } from '@angular/core';
import { SuEmptyState } from '@decryp7/paddock-ui';
import { CommandService, ContributionRegistry, ShellContextHost } from '@decryp7/paddock-ui/shell';
import { PdJobsStatus } from '@decryp7/paddock-ui/shell/jobs';
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
  imports: [SuEmptyState, PdJobsStatus, PdServiceStatus, PdNotificationsStatus, EnginePanel],
  template: `
    <main class="page">
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
      <sd-engine-panel />
      <section class="startup" aria-labelledby="sd-startup">
        <h2 id="sd-startup">Start-up contract</h2>
        <p class="context">The window reports ready after its first paint and answers liveness pings; this note is kept if the shell reloads the window to recover it.</p>
        <label class="note">
          <span>Note</span>
          <input #noteField type="text" data-testid="demo-note" [value]="note()" (input)="note.set(noteField.value)" />
        </label>
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
    .kinds { width: 100%; border-collapse: collapse; font-size: var(--pd-text-body); }
    .kinds caption { text-align: start; font-weight: var(--pd-weight-semibold); padding-block-end: var(--pd-space-2); }
    .kinds th, .kinds td { text-align: start; padding: var(--pd-space-1) var(--pd-space-2); border-bottom: 1px solid var(--pd-hairline); }
    .kinds th { color: var(--pd-text-muted); font-weight: var(--pd-weight-medium); }
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
  protected readonly kinds = this.registry.kinds.map((k) => ({ name: k.name, count: this.registry.items(k.name).length }));

  protected run(id: string): void {
    void this.commands.run(id);
  }
}
