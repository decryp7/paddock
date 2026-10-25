import { ChangeDetectionStrategy, Component, computed, inject, input } from '@angular/core';
import { PD_ICON_PATHS, PdGlyph } from '@decryp7/paddock-ui/icons';
import type { Diagram, Graph } from '../generated/view';
import { layoutGraph, layoutSequence, MARK_W, type LayoutNode, type Scene, type SceneNode } from './layout';
import { TONE_GLYPH, toneOf, toneWord } from './model';
import { ViewRuntime } from './runtime';
import { PdViewScroll } from './scroll';
import { TONE_CSS, toneClass } from './tone';

let nextId = 0;

/** The scene of a `Graph` node: layered (also for `force`) or radial. */
export function graphScene(g: Graph): Scene {
  const nodes: LayoutNode[] = g.nodes.map((n) => ({ id: n.id, label: n.label, tone: toneOf(n.tone), action: n.action }));
  return layoutGraph(nodes, g.edges, g.layout === 'radial');
}

/** The scene of a `Diagram` node: a sequence diagram has lifelines and messages in time order; the others are layered boxes with the element's group above and its detail under the label. */
export function diagramScene(d: Diagram): Scene {
  const klass = d.kind === 'class';
  // A class shows its group as a stereotype («model») and its members in a compartment under the name.
  const nodes: LayoutNode[] = d.model.elements.map((e) => ({ id: e.id, label: e.label, detail: e.detail, caption: klass && e.group ? `«${e.group}»` : e.group, compartment: klass }));
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
  imports: [PdGlyph, PdViewScroll],
  template: `
    @let s = scene();
    <div class="pd-view-scene__frame">
    <div class="pd-view-scene__scroll" role="region" pdViewScroll [attr.aria-label]="label()">
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
            <text class="pd-view-scene__edge-label" [attr.x]="e.lx" [attr.y]="e.ly" [attr.text-anchor]="e.anchor">{{ e.label }}</text>
          }
        }
        @for (n of s.nodes; track n.id) {
          <g
            [class]="'pd-view-scene__node ' + cls(n.tone)"
            [class.pd-view-scene__node--act]="!!n.action"
            [attr.role]="n.action ? 'button' : null"
            [attr.tabindex]="n.action && !runtime.disabled() ? 0 : null"
            [attr.aria-label]="n.action ? n.action.label + ': ' + nodeName(n) : null"
            [attr.aria-disabled]="n.action && runtime.disabled() ? 'true' : null"
            (click)="act(n.id)"
            (keydown.enter)="act(n.id)"
            (keydown.space)="act(n.id); $event.preventDefault()"
          >
            <title>{{ nodeName(n) }}</title>
            <rect [attr.x]="n.x" [attr.y]="n.y" [attr.width]="n.w" [attr.height]="n.h" rx="6" />
            @if (mark(n); as m) {
              <g class="pd-view-scene__mark" [attr.transform]="'translate(' + (n.x + 8) + ' ' + (n.y + markY(n)) + ') scale(0.5833)'">
                <path [attr.d]="m" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" />
              </g>
            }
            @if (n.caption) {
              <text class="pd-view-scene__caption" [attr.x]="n.x + n.w / 2" [attr.y]="n.y + 14" text-anchor="middle">{{ clip(n.caption, n.w) }}</text>
            }
            <text class="pd-view-scene__label" [attr.x]="textX(n)" [attr.y]="n.y + (n.caption ? 30 : n.detail.length ? 22 : n.h / 2 + 5)" text-anchor="middle">{{ clip(n.label, n.w - markRoom(n)) }}</text>
            @if (n.compartment) {
              <line class="pd-view-scene__rule" [attr.x1]="n.x" [attr.x2]="n.x + n.w" [attr.y1]="n.y + (n.caption ? 38 : 30)" [attr.y2]="n.y + (n.caption ? 38 : 30)" />
            }
            @for (d of n.detail; track $index) {
              <text class="pd-view-scene__detail" [attr.x]="n.compartment ? n.x + 10 : n.x + n.w / 2" [attr.y]="n.y + (n.caption ? 48 : 40) + $index * 16" [attr.text-anchor]="n.compartment ? 'start' : 'middle'">{{ clip(d, n.w - (n.compartment ? 4 : 0)) }}</text>
            }
          </g>
        }
      </svg>
    </div>
    <details class="pd-view-scene__list">
      <summary><pd-glyph name="chevron-right" [size]="14" />{{ asList }}</summary>
      <ul data-testid="view-scene-nodes">
        @for (n of s.nodes; track n.id) {
          <li>
            {{ nodeName(n) }}
            @if (n.caption || n.detail.length) {
              <ul class="pd-view-scene__details">
                @if (n.caption) {
                  <li>{{ n.caption }}</li>
                }
                @for (d of n.detail; track $index) {
                  <li>{{ d }}</li>
                }
              </ul>
            }
          </li>
        }
      </ul>
      <ul data-testid="view-scene-edges">
        @for (e of s.edges; track $index) {
          <li>{{ edgeText(e.from, e.to, e.label) }}</li>
        }
      </ul>
    </details>
    </div>
  `,
  styles: `
    ${TONE_CSS}
    :host { display: block; min-width: 0; }
    .pd-view-scene__frame { border: 1px solid var(--pd-hairline); border-radius: var(--pd-radius-card); background: var(--pd-surface-1); overflow: hidden; max-width: 100%; }
    .pd-view-scene__scroll { overflow: auto; max-width: 100%; max-height: 70vh; }
    .pd-view-scene__scroll:focus-visible { outline: 2px solid var(--pd-focus-ring); outline-offset: -2px; }
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
    .pd-view-scene__rule { stroke: var(--pd-border-strong); stroke-width: 1; }
    .pd-view-scene__mark { color: var(--pd-view-tone); }
    .pd-view-scene__list { border-top: 1px solid var(--pd-hairline); padding: 0 var(--pd-space-3); font-size: var(--pd-text-label); color: var(--pd-text-muted); }
    .pd-view-scene__list summary { display: flex; align-items: center; gap: var(--pd-space-1); cursor: pointer; min-height: 32px; list-style: none; }
    .pd-view-scene__list summary::-webkit-details-marker { display: none; }
    .pd-view-scene__list summary pd-glyph { transition: transform var(--pd-duration-micro) var(--pd-ease-move); }
    .pd-view-scene__list[open] summary pd-glyph { transform: rotate(90deg); }
    @media (prefers-reduced-motion: reduce) { .pd-view-scene__list summary pd-glyph { transition: none; } }
    .pd-view-scene__list summary:focus-visible { outline: 2px solid var(--pd-focus-ring); outline-offset: 2px; border-radius: 2px; }
    .pd-view-scene__list ul { margin: var(--pd-space-1) 0; padding-inline-start: var(--pd-space-4); overflow-wrap: break-word; }
    .pd-view-scene__list ul ul.pd-view-scene__details { margin: 0; color: var(--pd-text-subtle); }
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

  /** The path data of a node's status glyph (the tone's own shape), or '' for a neutral node. */
  protected mark(n: SceneNode): string {
    const glyph = TONE_GLYPH[toneOf(n.tone)];
    return glyph ? ((PD_ICON_PATHS as Readonly<Record<string, string>>)[glyph] ?? '') : '';
  }
  protected markRoom(n: SceneNode): number {
    return this.mark(n) ? MARK_W : 0;
  }
  protected markY(n: SceneNode): number {
    return n.caption ? 18.5 : n.detail.length ? 10.5 : n.h / 2 - 6.5;
  }
  /** Where the label is centred: after the mark when the box has one. */
  protected textX(n: SceneNode): number {
    return n.x + n.w / 2 + this.markRoom(n) / 2;
  }
  /** The name of a box for assistive technology and the list: the label and the word of its status ("Publish — Problem"), so a status is never colour alone. */
  protected nodeName(n: SceneNode): string {
    const word = toneWord(toneOf(n.tone));
    return word ? `${n.label} — ${word}` : n.label;
  }
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
