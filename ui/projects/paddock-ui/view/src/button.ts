import { ChangeDetectionStrategy, Component, computed, inject, input } from '@angular/core';
import { PdGlyph } from '@decryp7/paddock-ui/icons';
import type { ActionRef, Link } from '../generated/view';
import { ViewRuntime } from './runtime';
import { PdViewWrites, writesText } from './writes';

/**
 * An `ActionRef` as a button: its label is the text, a `writes` action carries the "Changes data" badge, a `confirm` question is
 * asked by the host first. Choosing it sends the action through the view's runtime with the host's `extra` keys (`row`, `node`, ...).
 */
@Component({
  selector: 'pd-view-button',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [PdViewWrites],
  template: `
    <button
      type="button"
      class="pd-btn"
      [class.pd-btn--primary]="variant() === 'primary'"
      [class.pd-btn--ghost]="variant() === 'ghost'"
      [class.pd-btn--sm]="small()"
      [disabled]="runtime.disabled()"
      [attr.aria-label]="describedAs()"
      (click)="press()"
    >
      {{ action().label }}
      @if (action().writes) {
        <pd-view-writes />
      }
    </button>
  `,
  styles: `
    :host { display: inline-flex; min-width: 0; }
    /* A long label (with its "Changes data" badge) wraps inside the button instead of widening the pane. */
    button { max-width: 100%; white-space: normal; flex-wrap: wrap; }
  `,
})
export class PdViewButton {
  protected readonly runtime = inject(ViewRuntime);
  protected readonly writes = writesText();
  readonly action = input.required<ActionRef>();
  /** The host's own keys for the `ViewAction` (`row`, `node`, `finding`, ...). */
  readonly extra = input<Readonly<Record<string, unknown>>>({});
  readonly variant = input<'default' | 'primary' | 'ghost'>('default');
  readonly small = input(false);
  /** Asked before the action is sent: `false` keeps it unsent (a form with a problem). */
  readonly guard = input<(() => boolean) | null>(null);
  /** Names the button for assistive technology when its label alone is not enough ("Open: Report.pdf"). */
  readonly context = input('');
  protected press(): void {
    if (this.guard()?.() === false) return;
    void this.runtime.run(this.action(), this.extra());
  }
  protected readonly describedAs = computed(() => {
    const c = this.context();
    if (!c) return null;
    return this.action().writes ? `${this.action().label}: ${c}, ${this.writes}` : `${this.action().label}: ${c}`;
  });
}

/** A `Link` as a text button: an action runs like any other, a web address goes to the host, which confirms and opens it. */
@Component({
  selector: 'pd-view-link',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [PdGlyph, PdViewWrites],
  template: `
    <button type="button" class="pd-view-link" [disabled]="runtime.disabled()" [attr.aria-label]="name()" (click)="runtime.follow(link(), extra())">
      <span class="pd-view-link__text">{{ shown() }}</span>
      @if (external()) {
        <pd-glyph name="external" [size]="14" />
      }
      @if (writing()) {
        <pd-view-writes />
      }
    </button>
  `,
  styles: `
    :host { display: inline-flex; min-width: 0; max-width: 100%; }
    .pd-view-link { display: inline-flex; align-items: center; gap: var(--pd-space-1); min-width: 0; max-width: 100%; min-height: 24px; padding: 0; border: 0; background: none; font: inherit; color: var(--pd-accent); cursor: pointer; text-align: start; }
    .pd-view-link:hover:not(:disabled) .pd-view-link__text { text-decoration: underline; }
    .pd-view-link:focus-visible { outline: 2px solid var(--pd-focus-ring); outline-offset: 2px; border-radius: 2px; }
    .pd-view-link:disabled { color: var(--pd-text-subtle); cursor: default; }
    .pd-view-link__text { overflow-wrap: break-word; }
  `,
})
export class PdViewLink {
  protected readonly runtime = inject(ViewRuntime);
  protected readonly writes = writesText();
  readonly link = input.required<Link>();
  readonly extra = input<Readonly<Record<string, unknown>>>({});
  /** The text to show instead of the link's own label (a table cell shows its text and the link acts on it). */
  readonly text = input('');
  protected readonly external = computed(() => 'openInBrowser' in this.link());
  protected readonly writing = computed(() => {
    const l = this.link();
    return 'action' in l && l.action.writes === true;
  });
  private readonly own = computed(() => {
    const l = this.link();
    return 'action' in l ? l.action.label : l.openInBrowser.label;
  });
  protected readonly shown = computed(() => this.text() || this.own());
  /** The link's own label names it; a cell's text is shown, and the label says what it does. */
  protected readonly name = computed(() => {
    const t = this.text();
    const base = t && t !== this.own() ? `${this.own()}: ${t}` : this.own();
    return this.external() ? `${base} (${$localize`:@@pd.view.opens-browser:opens in your browser`})` : base;
  });
}
