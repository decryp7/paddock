import { ChangeDetectionStrategy, Component } from '@angular/core';
import { SuEmptyState } from '@decryp7/paddock-ui';

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
  imports: [SuEmptyState],
  template: `
    <main class="page">
      <h1>Paddock shell demo</h1>
      <pd-empty-state
        heading="The shell arrives here part by part"
        description="Each part below is built into the shell by its work package and shown in this app."
      />
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
  `,
  styles: `
    .page { max-width: var(--pd-content-max); margin: 0 auto; padding: var(--pd-space-6) var(--pd-space-4); display: flex; flex-direction: column; gap: var(--pd-space-4); }
    h1 { margin: 0; font-size: var(--pd-text-title); }
    .parts { width: 100%; border-collapse: collapse; font-size: var(--pd-text-body); }
    .parts caption { text-align: start; font-weight: var(--pd-weight-semibold); padding-block-end: var(--pd-space-2); }
    .parts th, .parts td { text-align: start; padding: var(--pd-space-1) var(--pd-space-2); border-bottom: 1px solid var(--pd-hairline); }
    .parts th { color: var(--pd-text-muted); font-weight: var(--pd-weight-medium); }
    .parts code { overflow-wrap: anywhere; }
  `,
})
export class ShellDemo {
  protected readonly parts = PARTS;
}
