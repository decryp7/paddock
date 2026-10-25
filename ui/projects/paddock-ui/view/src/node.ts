import { NgComponentOutlet } from '@angular/common';
import { ChangeDetectionStrategy, Component, computed, effect, ErrorHandler, inject, input, isDevMode } from '@angular/core';
import { PdNeedsAiCard } from '@decryp7/paddock-ui/states';
import { PdGlyph, PdStatusGlyph } from '@decryp7/paddock-ui/icons';
import type { Node } from '../generated/view';
import { PdViewButton, PdViewLink } from './button';
import { PdViewChart } from './chart';
import { PdViewForm } from './form';
import { VIEW_NEEDS_AI, VIEW_OPEN_AI_SETTINGS } from './host';
import { diagramScene, graphScene, PdViewScene } from './scene';
import { childKey, glyphFor, shapeOf, toneOf, unknownKindName } from './model';
import { PdViewMatrix } from './matrix';
import { PdViewReview } from './review';
import { PdViewTable } from './table';
import { ViewRuntime } from './runtime';
import { PdViewTone, TONE_CSS, toneClass } from './tone';
import { PdViewTree } from './tree';

/** The dev-mode message when a host shows views and provides neither `VIEW_NEEDS_AI` nor `VIEW_OPEN_AI_SETTINGS` (rule 19). */
export const NEEDS_AI_WITHOUT_REPAIR =
  'pd-view drew a needsAi node but this app provides neither VIEW_NEEDS_AI nor VIEW_OPEN_AI_SETTINGS (rule 19: "needs local AI" must have a working repair). Provide one in the window that shows views.';

/**
 * One node of a view and everything under it: it picks the component for the node's kind (`shapeOf`) and draws it, and a stack draws
 * its children with this same component. All text is bound as text, never as markup; tones map to design tokens; a kind this version
 * does not know is shown as "Update the app to view this" for that node only, and the rest of the view still draws.
 */
@Component({
  selector: 'pd-view-node',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [PdStatusGlyph, NgComponentOutlet, PdGlyph, PdNeedsAiCard, PdViewButton, PdViewLink, PdViewTone, PdViewTable, PdViewTree, PdViewScene, PdViewForm, PdViewReview, PdViewMatrix, PdViewChart],
  host: { '[attr.data-kind]': 'shape().tag', '[attr.data-node-id]': 'node().id || null' },
  template: `
    @let s = shape();
    @switch (s.tag) {
      @case ('stack') {
        <div class="pd-view-stack" [class.pd-view-stack--row]="s.v.dir === 'row'">
          @for (c of s.v.children; track childKey(c, $index)) {
            <pd-view-node [node]="c" />
          }
        </div>
      }
      @case ('text') {
        <p class="pd-view-text" [class]="textClass(s.v.style, s.v.tone)" [attr.role]="s.v.style === 'heading' || s.v.style === 'subheading' ? 'heading' : null" [attr.aria-level]="s.v.style === 'heading' ? 3 : s.v.style === 'subheading' ? 4 : null">
          <pd-view-tone [tone]="s.v.tone" />{{ s.v.text }}
        </p>
      }
      @case ('table') {
        <pd-view-table [table]="s.v" [label]="title()" />
      }
      @case ('tree') {
        <pd-view-tree [tree]="s.v" [label]="title()" />
      }
      @case ('graph') {
        <pd-view-scene [scene]="graph(s.v)" [label]="title() || graphName" />
      }
      @case ('diagram') {
        <pd-view-scene [scene]="diagram(s.v)" [label]="title() || diagramName" />
      }
      @case ('form') {
        <pd-view-form [form]="s.v" [primary]="isPrimary()" />
      }
      @case ('reviewList') {
        <pd-view-review [list]="s.v" [label]="title()" />
      }
      @case ('matrix') {
        <pd-view-matrix [matrix]="s.v" [label]="title()" />
      }
      @case ('chart') {
        <pd-view-chart [chart]="s.v" />
      }
      @case ('badge') {
        <span class="pd-view-badge" [class]="toneClass(s.v.tone)"><pd-view-tone [tone]="s.v.tone" />{{ s.v.text }}</span>
      }
      @case ('progress') {
        <div class="pd-view-progress">
          <div class="pd-view-progress__row">
            <span class="pd-view-progress__label" [id]="progressId()">{{ s.v.label || progressName }}</span>
            <span class="pd-view-progress__value">{{ percent(s.v.value) }}%</span>
          </div>
          <div class="pd-view-progress__track" role="progressbar" aria-valuemin="0" aria-valuemax="100" [attr.aria-valuenow]="percent(s.v.value)" [attr.aria-labelledby]="progressId()">
            <div class="pd-view-progress__fill" [style.width.%]="percent(s.v.value)"></div>
          </div>
        </div>
      }
      @case ('empty') {
        <div class="pd-view-empty">
          <pd-glyph [name]="glyph(s.v.icon)" [size]="32" />
          <p class="pd-view-empty__title">{{ s.v.title }}</p>
          @if (s.v.hint) {
            <p class="pd-view-empty__hint">{{ s.v.hint }}</p>
          }
          @if (s.v.action; as a) {
            <pd-view-button [action]="a" [variant]="isPrimary() ? 'primary' : 'default'" />
          }
        </div>
      }
      @case ('needsAi') {
        @if (appNeedsAi; as live) {
          <ng-container *ngComponentOutlet="live; inputs: { feature: s.v.feature }" />
        } @else {
          <pd-needs-ai-card [title]="needsAiTitle(s.v.feature)" [message]="s.v.reason || needsAiDefault" icon="info" testid="view-needs-ai" [hint]="openAiSettings ? '' : needsAiElsewhere">
            @if (openAiSettings; as open) {
              <button type="button" class="pd-btn" [class.pd-btn--primary]="isPrimary()" data-testid="view-needs-ai-settings" [disabled]="runtime.disabled()" (click)="open()">{{ needsAiSettings }}</button>
            }
          </pd-needs-ai-card>
        }
      }
      @case ('link') {
        <pd-view-link [link]="s.v.target" [text]="s.v.label" />
      }
      @default {
        <p class="pd-view-unknown" role="note" data-testid="view-unknown" [attr.title]="s.name || null">
          <pd-status-glyph status="info" [size]="14" /><span>{{ updateApp }}@if (s.name) { <span class="pd-view-unknown__kind"> ({{ kindName(s.name) }})</span> }</span>
        </p>
      }
    }
  `,
  styles: `
    ${TONE_CSS}
    :host { display: block; min-width: 0; }
    .pd-view-stack { display: flex; flex-direction: column; gap: var(--pd-space-3); min-width: 0; }
    .pd-view-stack--row { flex-direction: row; flex-wrap: wrap; align-items: flex-start; gap: var(--pd-space-2); }
    /* Blocks (tables, charts, cards) share the width; short inline things pack at the gap. */
    .pd-view-stack--row > pd-view-node { flex: 1 1 14rem; min-width: 0; }
    .pd-view-stack--row > pd-view-node[data-kind='badge'], .pd-view-stack--row > pd-view-node[data-kind='link'] { flex: 0 1 auto; }
    .pd-view-stack--row > pd-view-node[data-kind='progress'] { flex: 0 1 14rem; }
    .pd-view-text { margin: 0; color: var(--pd-text-body); line-height: var(--pd-lh-body); overflow-wrap: break-word; white-space: pre-line; }
    .pd-view-text.pd-view-tone-info, .pd-view-text.pd-view-tone-good, .pd-view-text.pd-view-tone-warn, .pd-view-text.pd-view-tone-bad { color: var(--pd-view-tone); }
    /* The view's title is the one Title; a heading inside it is a Subtitle and a subheading is Body strong. */
    .pd-view-text--heading { font-size: var(--pd-text-subtitle); line-height: var(--pd-lh-subtitle); font-weight: var(--pd-weight-semibold); color: var(--pd-text); }
    .pd-view-text--subheading { font-size: var(--pd-text-body); line-height: var(--pd-lh-body); font-weight: var(--pd-weight-semibold); color: var(--pd-text); }
    .pd-view-text--caption { font-size: var(--pd-text-label); line-height: var(--pd-lh-label); color: var(--pd-text-muted); }
    /* Inline code: a mono face on a quiet surface; it wraps at spaces and breaks a word only when the word alone is wider than the line. */
    .pd-view-text--code { width: fit-content; max-width: 100%; box-sizing: border-box; padding: var(--pd-space-1) var(--pd-space-2); border-radius: var(--pd-radius-tag); background: var(--pd-surface-2); border: 1px solid var(--pd-hairline); color: var(--pd-text); font-family: var(--pd-font-mono); font-size: var(--pd-text-label); white-space: pre-wrap; }
    .pd-view-badge { display: inline-flex; align-items: center; gap: var(--pd-space-1); height: 20px; padding: 0 var(--pd-space-2); border-radius: var(--pd-radius-pill); background: var(--pd-view-tint); color: var(--pd-view-tone); font-size: var(--pd-text-label); font-weight: var(--pd-weight-medium); white-space: nowrap; max-width: 100%; vertical-align: top; }
    .pd-view-progress { display: grid; gap: var(--pd-space-1); }
    .pd-view-progress__row { display: flex; justify-content: space-between; gap: var(--pd-space-2); font-size: var(--pd-text-label); color: var(--pd-text-muted); }
    .pd-view-progress__value { font-variant-numeric: tabular-nums; }
    .pd-view-progress__track { height: 6px; border-radius: var(--pd-radius-pill); background: var(--pd-surface-3); box-shadow: inset 0 0 0 1px var(--pd-border-strong); overflow: hidden; }
    .pd-view-progress__fill { height: 100%; background: var(--pd-accent); border-radius: inherit; transition: width calc(var(--pd-duration) * var(--pd-motion-scale, 1)) var(--pd-ease-move); }
    @media (prefers-reduced-motion: reduce) { .pd-view-progress__fill { transition: none; } }
    .pd-view-empty { display: grid; justify-items: center; gap: var(--pd-space-2); padding: var(--pd-space-6) var(--pd-space-4); text-align: center; color: var(--pd-text-muted); }
    .pd-view-empty__title { margin: 0; font-size: var(--pd-text-subtitle); font-weight: var(--pd-weight-semibold); color: var(--pd-text); }
    .pd-view-empty__hint { margin: 0; max-width: 48ch; }
    .pd-view-unknown__kind { color: var(--pd-text-subtle); }
    .pd-view-unknown { display: flex; align-items: center; gap: var(--pd-space-2); margin: 0; padding: var(--pd-space-2) var(--pd-space-3); border: 1px dashed var(--pd-border-strong); border-radius: var(--pd-radius-control); color: var(--pd-text-muted); }
  `,
})
export class PdViewNode {
  readonly node = input.required<Node>();
  protected readonly runtime = inject(ViewRuntime);
  /** The view's title: the accessible name of the table, tree, list and drawing nodes. */
  protected readonly title = this.runtime.title;
  protected readonly shape = computed(() => shapeOf(this.node()));
  protected readonly childKey = childKey;
  protected readonly toneClass = toneClass;
  protected readonly graph = graphScene;
  protected readonly diagram = diagramScene;
  protected readonly graphName = $localize`:@@pd.view.graph:Graph`;
  protected readonly diagramName = $localize`:@@pd.view.diagram:Diagram`;
  protected readonly progressName = $localize`:@@pd.view.progress:Progress`;
  /** The app's own live "needs local AI" component (it has the window's Pitwall state), or null. */
  protected readonly appNeedsAi = inject(VIEW_NEEDS_AI);
  /** The app's "Open AI settings", or null (the card then says to set up local AI in the app's settings, and dev mode reports the missing wiring). The plugin's `setup` is never sent. */
  protected readonly openAiSettings = inject(VIEW_OPEN_AI_SETTINGS);
  protected readonly needsAiSettings = $localize`:@@pd.view.needs-ai.settings:Open AI settings`;
  protected readonly needsAiElsewhere = $localize`:@@pd.view.needs-ai.elsewhere:Set up local AI in this app's settings.`;
  protected readonly needsAiDefault = $localize`:@@pd.view.needs-ai:This needs local AI, which is not ready.`;
  protected readonly updateApp = $localize`:@@pd.view.unknown:Update the app to view this`;
  /**
   * Rule 19: every host that shows views provides `VIEW_NEEDS_AI` or `VIEW_OPEN_AI_SETTINGS`, so a "needs local AI" card always has
   * a working repair. In dev mode a `needsAi` node drawn with neither is reported (to the app's `ErrorHandler`, once per node) so the
   * missing host wiring is found while building, not by a user who sees a "not running" message with no button.
   */
  private readonly ruleNineteen = effect(() => {
    // Reporting to the error handler is a side effect on something outside the signal graph.
    if (this.shape().tag !== 'needsAi' || this.appNeedsAi || this.openAiSettings || !isDevMode()) return;
    this.errors.handleError(new Error(NEEDS_AI_WITHOUT_REPAIR));
  });
  private readonly errors = inject(ErrorHandler);
  private static next = 0;
  private readonly uid = ++PdViewNode.next;
  protected readonly progressId = () => `pd-view-progress-${this.uid}`;

  /** This node draws the view's one filled button (`ViewRuntime.primary`). */
  protected isPrimary(): boolean {
    return this.runtime.primary() === this.node();
  }
  protected kindName = unknownKindName;
  protected needsAiTitle(feature: string): string {
    return $localize`:@@pd.view.needs-ai.title:${feature}:feature: needs local AI`;
  }
  protected glyph(icon: unknown): string {
    return glyphFor(icon);
  }
  protected percent(v: number): number {
    return Math.round(Math.min(1, Math.max(0, Number.isFinite(v) ? v : 0)) * 100);
  }
  protected textClass(style: string, tone: unknown): string {
    return `pd-view-text--${style} ${toneClass(toneOf(tone))}`;
  }
}
