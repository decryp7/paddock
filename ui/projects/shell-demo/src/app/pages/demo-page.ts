import { ChangeDetectionStrategy, Component, computed, inject, input } from '@angular/core';
import { ShellContextHost } from '@decryp7/paddock-ui/shell';
import type { DemoContext } from '../demo-context';

/**
 * The frame of every demo page (UI rule 16): 24 px gutter, title 18/600 with one muted subtitle line, content up to
 * 1440 px. View > Compact layout tightens it (read through the app context, so it is not in the first-paint bundle).
 */
@Component({
  selector: 'sd-page',
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: { '[class.compact]': 'compact()' },
  template: `
    <header class="head">
      <h1>{{ heading() }}</h1>
      <p class="subtitle">{{ subtitle() }}</p>
    </header>
    <ng-content />
  `,
  styles: `
    :host { container: sd-page / inline-size; display: flex; flex-direction: column; gap: var(--pd-space-6); width: 100%; max-width: var(--pd-content-max); margin: 0 auto; padding: var(--pd-space-6); }
    :host(.compact) { gap: var(--pd-space-3); padding-block: var(--pd-space-3); }
    .head { display: flex; flex-direction: column; gap: var(--pd-space-1); }
    h1 { margin: 0; font-size: var(--pd-text-title); }
    .subtitle { margin: 0; color: var(--pd-text-muted); font-size: var(--pd-text-label); }
    @container sd-page (max-width: 480px) { :host { padding: var(--pd-space-4); } }
  `,
})
export class DemoPage {
  readonly heading = input.required<string>();
  readonly subtitle = input.required<string>();
  private readonly host = inject(ShellContextHost);
  protected readonly compact = computed(() => (this.host.context() as Partial<DemoContext> | null)?.compact?.() ?? false);
}

/** Shared section styles of the demo pages (one heading, hairline tables, ghost buttons). */
export const SECTION_STYLES = `
  :host { display: contents; }
  section { display: flex; flex-direction: column; gap: var(--pd-space-3); min-width: 0; }
  h2 { margin: 0; font-size: var(--pd-text-subtitle); }
  .context { margin: 0; color: var(--pd-text-muted); }
  .commands { display: flex; flex-wrap: wrap; gap: var(--pd-space-2); }
  .cmd { min-height: var(--pd-button-height); padding: 0 var(--pd-space-3); border-radius: var(--pd-radius-control); border: 1px solid var(--pd-border-control); background: var(--pd-surface-2); color: var(--pd-text); font-weight: var(--pd-weight-medium); cursor: pointer; }
  .cmd:hover:not(:disabled) { background: var(--pd-surface-3); }
  .cmd:focus-visible { outline: 2px solid var(--pd-focus-ring); outline-offset: 1px; }
  .cmd:disabled { color: var(--pd-text-subtle); cursor: default; }
  table { width: 100%; border-collapse: collapse; font-size: var(--pd-text-body); }
  caption { text-align: start; font-weight: var(--pd-weight-semibold); padding-block-end: var(--pd-space-2); }
  th, td { text-align: start; padding: var(--pd-space-1) var(--pd-space-2); border-bottom: 1px solid var(--pd-hairline); }
  th { color: var(--pd-text-muted); font-weight: var(--pd-weight-medium); }
  td.num, th.num { text-align: end; font-variant-numeric: tabular-nums; white-space: nowrap; }
  code { overflow-wrap: anywhere; font-family: var(--pd-font-mono); font-size: var(--pd-text-label); }
`;
