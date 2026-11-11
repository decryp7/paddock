import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { ContributionRegistry } from '@decryp7/paddock-ui/shell/contributions';
import { DemoPage, SECTION_STYLES } from './demo-page';

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
  { name: 'About, updates, what’s new', entry: 'shell/about', pkg: 'S11' },
  { name: 'Menus and keys', entry: 'shell/menu', pkg: 'S13' },
  { name: 'Pages frame', entry: 'shell/pages', pkg: 'S14' },
  { name: 'Theme and i18n', entry: 'shell/i18n', pkg: 'S15' },
  { name: 'Test kit', entry: 'shell/testing', pkg: 'S16' },
  { name: 'Local AI (Pitwall)', entry: 'shell/pitwall', pkg: 'X-pitwall' },
];

/** The demo's bottom page: what the shell is made of and what this window registered. */
@Component({
  selector: 'sd-about-page',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [DemoPage],
  template: `
    <sd-page heading="About the shell" subtitle="The reference app of the Paddock app shell, on the mock backend.">
      <table>
        <caption>Registered contributions</caption>
        <thead>
          <tr><th scope="col">Kind</th><th scope="col" class="num">Count</th></tr>
        </thead>
        <tbody>
          @for (kind of kinds; track kind.name) {
            <tr><td><code>{{ kind.name }}</code></td><td class="num">{{ kind.count }}</td></tr>
          }
        </tbody>
      </table>
      <table>
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
    </sd-page>
  `,
  styles: SECTION_STYLES,
})
export class AboutPage {
  private readonly registry = inject(ContributionRegistry);
  protected readonly parts = PARTS;
  protected readonly kinds = this.registry.kinds.map((k) => ({ name: k.name, count: this.registry.items(k.name).length }));
}
