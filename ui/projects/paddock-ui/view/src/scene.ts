import { ChangeDetectionStrategy, Component, computed, inject, input } from '@angular/core';
import type { Diagram, Graph } from '../generated/view';
import { layoutGraph, layoutSequence, type LayoutNode, type Scene } from './layout';
import { toneOf } from './model';
import { ViewRuntime } from './runtime';
import { TONE_CSS, toneClass } from './tone';

let nextId = 0;

/** The scene of a `Graph` node: layered (also for `force`) or radial. */
export function graphScene(g: Graph): Scene {
  const nodes: LayoutNode[] = g.nodes.map((n) => ({ id: n.id, label: n.label, tone: toneOf(n.tone), action: n.action }));
  return layoutGraph(nodes, g.edges, g.layout === 'radial');
}

/** The scene of a `Diagram` node: a sequence diagram has lifelines and messages in time order; the others are layered boxes with the element's group above and its detail under the label. */
export function diagramScene(d: Diagram): Scene {
  const nodes: LayoutNode[] = d.model.elements.map((e) => ({ id: e.id, label: e.label, detail: e.detail, caption: e.group }));
  return d.kind === 'sequence' ? layoutSequence(nodes, d.model.relations) : layoutGraph(nodes, d.model.relations);
}

/**
 * Draws a laid-out graph or diagram as SVG (text is bound as text, never markup). The drawing scrolls inside its own region when it
 * is wider than the pane; it has an accessible name and a text twin ("A to B: label") because a drawing is invisible to a screen
 * reader. A box with an action is a button that sends it with `node`.
 */
@Component({
  selector: 'pd-view-scene',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @let s = scene();
    <div class="pd-view-scene__scroll" role="region" tabindex="0" [attr.aria-label]="label()">
      <svg class="pd-view-scene__svg" [attr.viewBox]="'0 0 ' + s.width + ' ' + s.height" [attr.width]="s.width" [attr.height]="s.height" role="group" [attr.aria-label]="summary()" focusable="false">
        <defs>
          <marker [attr.id]="arrowId" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="8" markerHeight="8" orient="auto-start-reverse">
            <path d="M 0 0 L 10 5 L 0 10 z" class="pd-view-scene__arrow" />
          </marker>
        </defs>
        @for (l of s.lifelines; track $index) {
          <line class="pd-view-scene__life" [attr.x1]="l.x" [attr.x2]="l.x" [attr.y1]="l.y1" [attr.y2]="l.y2" />
        }
        @for (e of s.edges; track $index) {
          <path class="pd-view-scene__edge" [attr.d]="e.path" fill="none" [attr.marker-end]="'url(#' + arrowId + ')'" />
          @if (e.label) {
            <text class="pd-view-scene__edge-label" [attr.x]="e.lx" [attr.y]="e.ly" text-anchor="middle">{{ e.label }}</text>
          }
        }
        @for (n of s.nodes; track n.id) {
          <g
            [class]="'pd-view-scene__node ' + cls(n.tone)"
            [class.pd-view-scene__node--act]="!!n.action"
            [attr.role]="n.action ? 'button' : null"
            [attr.tabindex]="n.action && !runtime.disabled() ? 0 : null"
            [attr.aria-label]="n.action ? n.action.label + ': ' + n.label : null"
            [attr.aria-disabled]="n.action && runtime.disabled() ? 'true' : null"
            (click)="act(n.id)"
            (keydown.enter)="act(n.id)"
            (keydown.space)="act(n.id); $event.preventDefault()"
          >
            <title>{{ n.label }}</title>
            <rect [attr.x]="n.x" [attr.y]="n.y" [attr.width]="n.w" [attr.height]="n.h" rx="6" />
            @if (n.caption) {
              <text class="pd-view-scene__caption" [attr.x]="n.x + n.w / 2" [attr.y]="n.y + 14" text-anchor="middle">{{ clip(n.caption, n.w) }}</text>
            }
            <text class="pd-view-scene__label" [attr.x]="n.x + n.w / 2" [attr.y]="n.y + (n.caption ? 30 : n.detail.length ? 22 : n.h / 2 + 5)" text-anchor="middle">{{ clip(n.label, n.w) }}</text>
            @for (d of n.detail; track $index) {
              <text class="pd-view-scene__detail" [attr.x]="n.x + n.w / 2" [attr.y]="n.y + (n.caption ? 48 : 40) + $index * 16" text-anchor="middle">{{ clip(d, n.w) }}</text>
            }
          </g>
        }
      </svg>
    </div>
    <details class="pd-view-scene__list">
      <summary>{{ asList }}</summary>
      <ul>
        @for (n of s.nodes; track n.id) {
          <li>{{ n.label }}</li>
        }
      </ul>
      <ul>
        @for (e of s.edges; track $index) {
          <li>{{ edgeText(e.from, e.to, e.label) }}</li>
        }
      </ul>
    </details>
  `,
  styles: `
    ${TONE_CSS}
    :host { display: block; min-width: 0; }
    .pd-view-scene__scroll { overflow: auto; max-width: 100%; max-height: 70vh; border: 1px solid var(--pd-hairline); border-radius: var(--pd-radius-control); background: var(--pd-surface-1); }
    .pd-view-scene__scroll:focus-visible { outline: 2px solid var(--pd-focus-ring); outline-offset: 1px; }
    .pd-view-scene__svg { display: block; max-width: none; margin-inline: auto; font-family: var(--pd-font-sans); }
    .pd-view-scene__node rect { fill: var(--pd-surface-2); stroke: var(--pd-view-tone, var(--pd-border-strong)); stroke-width: 1.5; }
    .pd-view-scene__node.pd-view-tone-neutral rect { stroke: var(--pd-border-strong); }
    .pd-view-scene__node--act { cursor: pointer; }
    .pd-view-scene__node--act:hover rect { fill: var(--pd-hover); }
    .pd-view-scene__node--act:focus-visible { outline: none; }
    .pd-view-scene__node--act:focus-visible rect { stroke: var(--pd-focus-ring); stroke-width: 3; }
    .pd-view-scene__label { fill: var(--pd-text); font-size: 13px; font-weight: 500; }
    .pd-view-scene__caption { fill: var(--pd-text-muted); font-size: 11px; }
    .pd-view-scene__detail { fill: var(--pd-text-muted); font-size: 12px; }
    .pd-view-scene__edge { stroke: var(--pd-text-muted); stroke-width: 1.5; }
    .pd-view-scene__edge-label { fill: var(--pd-text-muted); font-size: 12px; paint-order: stroke; stroke: var(--pd-surface-1); stroke-width: 4px; }
    .pd-view-scene__arrow { fill: var(--pd-text-muted); }
    .pd-view-scene__life { stroke: var(--pd-border-strong); stroke-dasharray: 4 4; }
    .pd-view-scene__list { margin-top: var(--pd-space-2); font-size: var(--pd-text-label); color: var(--pd-text-muted); }
    .pd-view-scene__list summary { cursor: pointer; min-height: 24px; }
    .pd-view-scene__list summary:focus-visible { outline: 2px solid var(--pd-focus-ring); }
    .pd-view-scene__list ul { margin: var(--pd-space-1) 0; padding-inline-start: var(--pd-space-4); }
  `,
})
export class PdViewScene {
  protected readonly runtime = inject(ViewRuntime);
  readonly scene = input.required<Scene>();
  /** The accessible name of the drawing. */
  readonly label = input.required<string>();
  protected readonly arrowId = `pd-view-arrow-${++nextId}`;
  protected readonly asList = $localize`:@@pd.view.scene.list:Show as a list`;
  protected readonly summary = computed(() => {
    const s = this.scene();
    return $localize`:@@pd.view.scene.summary:${this.label()}:label:, ${s.nodes.length}:nodes: items and ${s.edges.length}:edges: connections`;
  });

  protected cls(tone: string): string {
    return toneClass(tone);
  }
  /** Shortens text that does not fit its box (about 7 px a character); the full text is the box's tooltip. */
  protected clip(text: string, width: number): string {
    const max = Math.max(4, Math.floor((width - 16) / 7));
    return text.length > max ? `${text.slice(0, max - 1)}…` : text;
  }
  protected edgeText(from: string, to: string, label: string): string {
    const names = new Map(this.scene().nodes.map((n) => [n.id, n.label]));
    const a = names.get(from) ?? from;
    const b = names.get(to) ?? to;
    return label ? $localize`:@@pd.view.scene.edge-label:${a}:from: to ${b}:to: (${label}:label:)` : $localize`:@@pd.view.scene.edge:${a}:from: to ${b}:to:`;
  }
  protected act(id: string): void {
    const n = this.scene().nodes.find((x) => x.id === id);
    if (n?.action) void this.runtime.run(n.action, { node: id });
  }
}
