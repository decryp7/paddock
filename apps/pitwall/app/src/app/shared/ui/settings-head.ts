import { ChangeDetectionStrategy, Component, input } from '@angular/core';

/**
 * The header of a Settings page that Pitwall draws itself (Connected apps, Chats, Tray popup): the title, with its help line
 * under it. One component, so the pages cannot drift apart, and its own class names, so the app-wide `.page-head` (a flex
 * row for a page's title and actions) never reaches it (round-5 review: the Chats page had picked it up).
 */
@Component({
  selector: 'pw-settings-head',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <h2 class="title" [attr.id]="titleId()">{{ heading() }}</h2>
    @if (help()) {
      <p class="help" data-testid="settings-head-help">{{ help() }}</p>
    }
  `,
  styles: `
    :host { display: block; }
    .title { margin: 0; font-size: var(--pd-text-subtitle); font-weight: var(--pd-weight-semibold); }
    .help { margin: var(--pd-space-1) 0 0; color: var(--pd-text-muted); max-width: 80ch; }
  `,
})
export class SettingsHead {
  /** The page's title. */
  readonly heading = input.required<string>();
  /** What the page is for, in one or two sentences. */
  readonly help = input<string | null>(null);
  /** An id for the title, when the page labels a region by it. */
  readonly titleId = input<string | null>(null);
}
