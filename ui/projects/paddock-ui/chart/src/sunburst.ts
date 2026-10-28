import {
  afterNextRender,
  ChangeDetectionStrategy,
  Component,
  computed,
  contentChild,
  DestroyRef,
  Directive,
  effect,
  ElementRef,
  inject,
  Injector,
  input,
  model,
  output,
  signal,
  TemplateRef,
  untracked,
} from '@angular/core';
import { NgTemplateOutlet } from '@angular/common';
import { parseCssColor, readPaddockTokens, SuEmptyState, SuErrorState, PD_CLOCK_SOURCE } from '@decryp7/paddock-ui';
import { reducedMotion } from './chart';
import {
  arcPath,
  branchSlots,
  clampOr,
  contrastRatio,
  labelFits,
  labelPath,
  luminance,
  nextFocus,
  ringRadii,
  sizeOf,
  sunburstLayout,
  type PdArc,
  type PdSunburstColourBy,
  type PdSunburstNode,
  type PdSunburstPoint,
} from './sunburst-layout';

/** Replaces the default centre of `pd-sunburst`; the context's `$implicit` is the hovered or focused point, or the centre as a point. */
@Directive({ selector: 'ng-template[pdSunburstCentre]' })
export class PdSunburstCentre {
  readonly template = inject<TemplateRef<{ $implicit: PdSunburstPoint }>>(TemplateRef);
}

/** The size of the drawing in viewBox units (it scales with the box). */
const SIZE = 560;
const MID = SIZE / 2;
const OUTER = 276;
const INNER = 92;
/** How long a burst of live updates is coalesced (4 per second). */
const LIVE_MS = 250;
const GAP = 1.8;

let nextId = 0;

interface Shape {
  readonly arc: PdArc;
  readonly d: string;
  readonly fill: string;
  readonly aria: string;
}

interface LabelView {
  readonly id: string;
  readonly pathId: string;
  readonly d: string;
  readonly text: string;
  readonly ink: string;
}

/**
 * A drillable ring chart for hierarchies (docs/plans/pd-sunburst.md): the centre node in the middle, up to six rings of its
 * descendants around it, coloured by branch (the five largest children take ring-ordered chart colours, the rest are neutral).
 * Click or Enter on a folder makes it the centre (`centre` is two-way); the centre circle goes up one level. SVG, so every
 * segment is a focusable `treeitem`; the consumer's list or table is the accessible equivalent that carries every value.
 * Segments under `minAngle` fold into "n smaller items", the drawn count is capped, hidden space is hatched and not yet counted
 * space is a dashed ghost, so status is never colour alone.
 *
 * ```html
 * <pd-sunburst label="Disk usage of Macintosh HD" [root]="tree()" [format]="bytes" [scale]="usedBytes()" [(centre)]="centre" />
 * ```
 */
@Component({
  selector: 'pd-sunburst',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [NgTemplateOutlet, SuEmptyState, SuErrorState],
  host: {
    '[class.pd-reduce]': 'reduced()',
    '[attr.data-state]': 'shown()',
    '(keydown)': 'onKey($event)',
    '(contextmenu)': 'onContext($event, hoveredId())',
  },
  template: `
    @switch (shown()) {
      @case ('loading') {
        <div class="pd-wrap" role="status" [attr.aria-label]="loadingText" data-testid="sunburst-loading">
          <svg class="pd-svg" [attr.viewBox]="'0 0 ' + size + ' ' + size" aria-hidden="true" focusable="false">
            @for (r of skeleton(); track $index) {
              <path class="pd-skeleton" [attr.d]="r" fill-rule="evenodd" />
            }
          </svg>
        </div>
      }
      @case ('empty') {
        <pd-empty-state [heading]="emptyTitle()" [description]="emptyText()" data-testid="sunburst-empty">
          <div slot="action"><ng-content select="[pdSunburstEmptyAction]" /></div>
        </pd-empty-state>
      }
      @case ('error') {
        <pd-error-state [heading]="errorTitle" [cause]="errorText()" (retry)="retry.emit()" data-testid="sunburst-error" />
      }
      @default {
        <div class="pd-wrap" (mouseleave)="hoverOut()">
          <svg class="pd-svg" role="tree" [attr.aria-label]="label()" [attr.viewBox]="'0 0 ' + size + ' ' + size">
            <defs>
              <pattern [attr.id]="uid + '-hidden'" patternUnits="userSpaceOnUse" width="7" height="7" patternTransform="rotate(45)">
                <rect class="pd-pat-hidden" width="7" height="7" />
                <line class="pd-pat-line" x1="0" y1="0" x2="0" y2="7" />
              </pattern>
              <pattern [attr.id]="uid + '-pending'" patternUnits="userSpaceOnUse" width="9" height="9" patternTransform="rotate(45)">
                <rect class="pd-pat-pending" width="9" height="9" />
                <line class="pd-pat-line pd-pat-line--faint" x1="0" y1="0" x2="0" y2="9" />
              </pattern>
              @for (l of labels(); track l.id) {
                <path [attr.id]="l.pathId" [attr.d]="l.d" />
              }
            </defs>
            @for (key of [fadeKey()]; track key) {
              <g class="pd-arcs" [class.pd-fade]="animate()">
                @for (s of shapes(); track s.arc.id) {
                  <path
                    role="treeitem"
                    class="pd-arc"
                    [class.pd-arc--hidden]="s.arc.kind === 'hidden'"
                    [class.pd-arc--pending]="s.arc.kind === 'pending'"
                    [class.pd-arc--active]="s.arc.id === activeId()"
                    [attr.d]="s.d"
                    [attr.data-id]="s.arc.id"
                    [attr.aria-label]="s.aria"
                    [attr.aria-level]="s.arc.depth"
                    [attr.aria-setsize]="s.arc.setsize"
                    [attr.aria-posinset]="s.arc.posinset"
                    [attr.aria-expanded]="s.arc.expandable ? s.arc.open : null"
                    [attr.tabindex]="s.arc.id === tabStop() ? 0 : -1"
                    [style.fill]="s.fill"
                    [style.opacity]="opacity(s.arc)"
                    (click)="activateArc(s.arc)"
                    (mouseenter)="hover(s.arc.id)"
                    (focus)="focusArc(s.arc.id)"
                    (blur)="blurArc()"
                    (contextmenu)="onContext($event, s.arc.id)"
                  ></path>
                }
              </g>
            }
            @if (focusShape(); as f) {
              <path class="pd-focus-ring" [attr.d]="f" aria-hidden="true" />
            }
            @for (l of labels(); track l.id) {
              <text class="pd-label" aria-hidden="true" [style.fill]="l.ink" [style.opacity]="labelOpacity(l.id)">
                <textPath [attr.href]="'#' + l.pathId" startOffset="50%" text-anchor="middle">{{ l.text }}</textPath>
              </text>
            }
            <circle class="pd-centre-circle" [class.pd-centre-circle--up]="canGoUp()" [attr.cx]="mid" [attr.cy]="mid" [attr.r]="inner - 1" aria-hidden="true" (click)="goUp()" />
          </svg>
          <div class="pd-centre" [style.width.%]="centreWidth">
            @if (customCentre(); as tpl) {
              <ng-container [ngTemplateOutlet]="tpl.template" [ngTemplateOutletContext]="{ $implicit: centrePoint() }" />
            } @else {
              <span class="pd-centre-name" data-testid="sunburst-centre-name">{{ centrePoint().node.label }}</span>
              <span class="pd-centre-value">{{ valueParts().number }}@if (valueParts().unit) {<span class="pd-centre-unit"> {{ valueParts().unit }}</span>}</span>
              @if (shareText()) {
                <span class="pd-centre-line">{{ shareText() }}</span>
              }
              @if (centrePoint().node.detail) {
                <span class="pd-centre-line">{{ centrePoint().node.detail }}</span>
              }
              @if (canGoUp() && !activeId()) {
                <button type="button" class="pd-up" (click)="goUp()">
                  <svg viewBox="0 0 12 12" width="12" height="12" aria-hidden="true" focusable="false"><path d="M6 10V2M2.5 5.5 6 2l3.5 3.5" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" /></svg>
                  {{ upText() }}
                </button>
              }
            }
          </div>
          <p class="pd-sr" aria-live="polite" data-testid="sunburst-live">{{ announcement() }}</p>
        </div>
      }
    }
  `,
  styles: `
    :host { display: block; position: relative; min-width: 0; width: 100%; }
    .pd-wrap { position: relative; width: 100%; aspect-ratio: 1; }
    .pd-svg { display: block; width: 100%; height: 100%; overflow: visible; }
    .pd-arc { stroke: none; cursor: pointer; outline: none; transition: opacity 100ms linear; }
    .pd-arc--hidden, .pd-arc--pending { cursor: default; }
    .pd-arc--pending { stroke: var(--pd-border-strong); stroke-width: 1; stroke-dasharray: 3 3; }
    .pd-arc--active { stroke: var(--pd-text); stroke-width: 2; }
    .pd-pat-hidden { fill: var(--pd-surface-2); }
    .pd-pat-pending { fill: var(--pd-surface-1); }
    .pd-pat-line { stroke: var(--pd-text-subtle); stroke-width: 1.5; }
    .pd-pat-line--faint { opacity: 0.5; stroke-width: 1; }
    .pd-focus-ring { fill: none; stroke: var(--pd-accent); stroke-width: 2; pointer-events: none; }
    .pd-label { font-size: var(--pd-text-label); font-weight: var(--pd-weight-medium); pointer-events: none; font-family: var(--pd-font-sans); }
    .pd-centre-circle { fill: transparent; stroke: none; }
    .pd-centre-circle--up { cursor: pointer; }
    .pd-centre { position: absolute; left: 50%; top: 50%; transform: translate(-50%, -50%); display: flex; flex-direction: column; align-items: center; text-align: center; gap: 2px; pointer-events: none; color: var(--pd-text); }
    .pd-centre-name { font-size: var(--pd-text-body); font-weight: var(--pd-weight-semibold); max-width: 100%; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .pd-centre-value { font-size: var(--pd-text-display); line-height: 1.15; font-weight: var(--pd-weight-semibold); font-variant-numeric: tabular-nums; }
    .pd-centre-unit { font-size: var(--pd-text-body); font-weight: var(--pd-weight-regular, 400); color: var(--pd-text-muted); }
    .pd-centre-line { font-size: var(--pd-text-label); color: var(--pd-text-muted); max-width: 100%; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .pd-up { pointer-events: auto; display: inline-flex; align-items: center; gap: 4px; min-height: 24px; padding: 0 var(--pd-space-2); border: 0; background: transparent; color: var(--pd-accent-text, var(--pd-accent)); font: inherit; font-size: var(--pd-text-label); cursor: pointer; border-radius: var(--pd-radius-control); }
    .pd-up:hover { background: var(--pd-hover); }
    .pd-sr { position: absolute; width: 1px; height: 1px; overflow: hidden; clip-path: inset(50%); white-space: nowrap; margin: 0; }
    .pd-skeleton { fill: var(--pd-hover); animation: pd-sb-pulse 1.4s ease-in-out infinite; }
    .pd-fade { animation: pd-sb-in 180ms ease-out; }
    :host(.pd-reduce) .pd-fade { animation-duration: 100ms; }
    :host(.pd-reduce) .pd-skeleton { animation: none; }
    :host(.pd-reduce) .pd-arc { transition: none; }
    @keyframes pd-sb-in { from { opacity: 0; } to { opacity: 1; } }
    @keyframes pd-sb-pulse { 50% { opacity: 0.55; } }
    @media (prefers-reduced-motion: reduce) { .pd-skeleton { animation: none; } .pd-fade { animation-duration: 100ms; } .pd-arc { transition: none; } }
    :root[data-motion='reduce'] .pd-skeleton { animation: none; }
  `,
})
export class PdSunburst {
  private readonly host = inject<ElementRef<HTMLElement>>(ElementRef).nativeElement;
  private readonly injector = inject(Injector);
  private readonly clock = inject(PD_CLOCK_SOURCE);

  /** The hierarchy; null shows the empty state (or the skeleton while `state` is `loading`). */
  readonly root = input<PdSunburstNode | null>(null);
  /** Id of the node in the middle (two-way); null or an unknown id is the root. */
  readonly centre = model<string | null>(null);
  /** The accessible name of the chart. */
  readonly label = input.required<string>();
  /** Levels drawn below the centre, 1 to 6. */
  readonly rings = input(4);
  /** Degrees below which a segment folds into "n smaller items" (at least 1). */
  readonly minAngle = input(1.5);
  /** Children per parent before folding. */
  readonly maxChildren = input(48);
  /** Segments drawn before `minAngle` rises (at most 2000). */
  readonly maxSegments = input(1000);
  /** A fixed denominator (a volume's used bytes) so segments only grow during a live fill; null: the centre's value. */
  readonly scale = input<number | null>(null);
  /** `branch` (five largest children coloured, the rest neutral) or a function giving a palette slot (0-7) or 'neutral'. */
  readonly colourBy = input<PdSunburstColourBy>('branch');
  /** Value text for the centre, labels and the accessible names. */
  readonly format = input.required<(value: number) => string>();
  readonly state = input<'ready' | 'loading' | 'empty' | 'error'>('ready');
  readonly emptyTitle = input('Nothing to show');
  readonly emptyText = input('');
  readonly errorText = input('');
  /** While true: no animation, and updates of `root` are coalesced to four per second. */
  readonly live = input(false);
  readonly motion = input<'auto' | 'reduce'>('auto');

  /** The pointer or focus moved to a segment, or left them. */
  readonly hoverChange = output<PdSunburstPoint | null>();
  /** Enter or click on a segment that cannot be drilled (hidden space, a ghost, a leaf). */
  readonly activate = output<PdSunburstPoint>();
  /** The drilled node has more children that are not loaded. */
  readonly loadChildren = output<string>();
  /** A folded or `other` segment was activated: the ids it stands for. */
  readonly openOther = output<{ parent: string; ids: string[] }>();
  /** Right-click, Shift+F10 or the Menu key; the consumer opens its menu (nothing is built in). */
  readonly contextMenu = output<{ point: PdSunburstPoint; x: number; y: number }>();
  /** The segment cap raised the minimum angle. */
  readonly folded = output<{ segments: number; minAngle: number }>();
  /** Retry was pressed in the error state. */
  readonly retry = output<void>();

  protected readonly customCentre = contentChild(PdSunburstCentre);
  protected readonly size = SIZE;
  protected readonly mid = MID;
  protected readonly inner = INNER;
  protected readonly centreWidth = ((INNER * 2) / SIZE) * 100 * 0.86;
  protected readonly uid = `pd-sb-${nextId++}`;
  protected readonly loadingText = $localize`:@@pd.ui.loading:Loading`;
  protected readonly errorTitle = $localize`:@@pd.ui.sunburst.error:Could not draw the chart`;

  private readonly appearance = signal(0);
  private readonly shownRoot = signal<PdSunburstNode | null>(null);
  protected readonly hoveredId = signal<string | null>(null);
  private readonly focusedId = signal<string | null>(null);
  private readonly hasFocus = signal(false);
  protected readonly announcement = signal('');
  private lastEmitted: string | null = null;
  private lastApplied = Number.NEGATIVE_INFINITY;
  private timer: ReturnType<typeof setTimeout> | null = null;

  protected readonly reduced = computed(() => {
    this.appearance();
    return this.motion() === 'reduce' || reducedMotion();
  });
  protected readonly animate = computed(() => !this.live());

  protected readonly shown = computed<'ready' | 'loading' | 'empty' | 'error'>(() => {
    const s = this.state();
    if (s !== 'ready') return s;
    return this.shownRoot() ? 'ready' : 'empty';
  });

  private readonly palette = computed(() => {
    this.appearance();
    return readPaddockTokens(this.host);
  });

  protected readonly layout = computed(() => {
    const root = this.shownRoot();
    if (!root) return null;
    const rings = this.rings();
    return sunburstLayout(root, this.centre(), {
      rings,
      minAngle: this.minAngle(),
      maxChildren: this.maxChildren(),
      maxSegments: this.maxSegments(),
      scale: this.scale(),
      colourBy: this.colourBy(),
      order: branchSlots(this.palette().palette),
    });
  });

  private readonly radii = computed(() => ringRadii(Math.round(clampOr(this.rings(), 1, 6, 4)), INNER, OUTER));
  private readonly arcsById = computed(() => new Map((this.layout()?.arcs ?? []).map((a) => [a.id, a])));

  protected readonly skeleton = computed(() => this.radii().map((r) => arcPath(MID, MID, r.r0, r.r1, 0, 360, GAP)));

  protected readonly shapes = computed<readonly Shape[]>(() => {
    const layout = this.layout();
    if (!layout) return [];
    const radii = this.radii();
    const fmt = this.format();
    return layout.arcs.map((arc) => {
      const r = radii[arc.depth - 1] as { r0: number; r1: number };
      return { arc, d: arcPath(MID, MID, r.r0, r.r1, arc.a0, arc.a1, GAP), fill: this.fillOf(arc), aria: this.ariaOf(arc, fmt) };
    });
  });

  protected readonly fadeKey = computed(() => this.layout()?.centre.id ?? '');
  protected readonly activeId = computed(() => this.hoveredId() ?? (this.hasFocus() ? this.focusedId() : null));
  protected readonly tabStop = computed(() => {
    const arcs = this.layout()?.arcs ?? [];
    const f = this.focusedId();
    return f && this.arcsById().has(f) ? f : (arcs[0]?.id ?? null);
  });
  protected readonly canGoUp = computed(() => (this.layout()?.trail.length ?? 1) > 1);
  protected readonly upText = computed(() => {
    const trail = this.layout()?.trail ?? [];
    return $localize`:@@pd.ui.sunburst.up:Up to ${trail[trail.length - 2]?.label ?? ''}:parent:`;
  });

  /** The ids from the active segment up to the first ring: these stay at full opacity. */
  private readonly lit = computed(() => {
    const id = this.activeId();
    if (!id) return null;
    const set = new Set<string>();
    const byId = this.arcsById();
    for (let at = byId.get(id); at; at = at.parent === null ? undefined : byId.get(at.parent)) set.add(at.id);
    return set;
  });

  protected readonly focusShape = computed(() => {
    const id = this.hasFocus() ? this.focusedId() : null;
    const arc = id ? this.arcsById().get(id) : undefined;
    const r = arc ? this.radii()[arc.depth - 1] : undefined;
    return arc && r ? arcPath(MID, MID, r.r0, r.r1, arc.a0, arc.a1, -6) : null;
  });

  protected readonly labels = computed<readonly LabelView[]>(() => {
    const layout = this.layout();
    if (!layout) return [];
    const radii = this.radii();
    const fmt = this.format();
    const tokens = this.palette();
    const surface = parseCssColor(tokens.surface1);
    const text = parseCssColor(tokens.text);
    const out: LabelView[] = [];
    for (const arc of layout.arcs) {
      if (arc.depth > 2 || arc.kind !== 'item') continue;
      const r = radii[arc.depth - 1] as { r0: number; r1: number };
      const rMid = (r.r0 + r.r1) / 2;
      const length = ((arc.a1 - arc.a0) * Math.PI * rMid) / 180;
      const words = arc.depth === 1 ? `${arc.label}  ${fmt(arc.value)}` : arc.label;
      if (!labelFits(length, r.r1 - r.r0, words)) continue;
      const ink = this.inkFor(arc, tokens, surface, text);
      if (!ink) continue;
      out.push({ id: arc.id, pathId: `${this.uid}-l${out.length}`, d: labelPath(MID, MID, rMid, arc.a0, arc.a1), text: words, ink });
    }
    return out;
  });

  protected readonly centrePoint = computed<PdSunburstPoint>(() => {
    const layout = this.layout();
    const active = this.activeId();
    const arc = active ? this.arcsById().get(active) : undefined;
    if (arc) return this.pointOf(arc);
    const trail = layout?.trail ?? [];
    const centre = layout?.centre ?? { id: '', label: '', value: 0 };
    const parent = trail[trail.length - 2];
    const parentValue = parent ? Math.max(parent.value, 0) : 0;
    return { node: centre, path: [], share: parentValue > 0 ? Math.min(1, Math.max(0, centre.value) / parentValue) : 1 };
  });

  protected readonly valueParts = computed(() => {
    const text = this.format()(sizeOf(this.centrePoint().node.value));
    const m = /^(.*\S)\s+(\S+)$/.exec(text);
    return m ? { number: m[1] as string, unit: m[2] as string } : { number: text, unit: '' };
  });

  protected readonly shareText = computed(() => {
    const active = this.activeId();
    const arc = active ? this.arcsById().get(active) : undefined;
    if (arc) return `${percent(arc.value / (arc.parentValue || 1))} of ${arc.parentLabel}`;
    const trail = this.layout()?.trail ?? [];
    const parent = trail[trail.length - 2];
    return parent ? `${percent(this.centrePoint().share)} of ${parent.label}` : '';
  });

  constructor() {
    // effect-state: copies the input into `shownRoot` through the live coalescer (a timer is the side effect).
    effect(() => {
      const next = this.root();
      const live = this.live();
      untracked(() => this.applyRoot(next, live));
    });
    effect(() => {
      const f = this.layout()?.folded;
      if (f) untracked(() => this.folded.emit(f));
    });
    let first = true;
    // effect-state: the live region's text is written once per change of the centre (not per live update), never on the first draw.
    effect(() => {
      this.fadeKey();
      untracked(() => {
        const centre = this.layout()?.centre;
        if (first) {
          first = false;
          return;
        }
        if (centre) this.announcement.set([centre.label, this.format()(sizeOf(centre.value)), centre.detail].filter(Boolean).join(', '));
      });
    });
    const observer = new MutationObserver(() => this.appearance.update((n) => n + 1));
    observer.observe(document.documentElement, { attributes: true, attributeFilter: ['data-theme', 'data-accent', 'data-contrast', 'data-motion'] });
    inject(DestroyRef).onDestroy(() => {
      observer.disconnect();
      if (this.timer) clearTimeout(this.timer);
    });
  }

  /** Makes the enclosing node the centre (the root is `null`). The consumer's keyboard command (Alt+Up) calls this. */
  goUp(): void {
    const trail = this.layout()?.trail ?? [];
    if (trail.length < 2) return;
    this.centre.set(trail.length === 2 ? null : (trail[trail.length - 2] as PdSunburstNode).id);
  }

  /** Live updates are applied at most every 250 ms, the last one always. */
  private applyRoot(next: PdSunburstNode | null, live: boolean): void {
    if (this.timer) {
      clearTimeout(this.timer);
      this.timer = null;
    }
    const now = this.clock();
    const wait = this.lastApplied + LIVE_MS - now;
    if (!live || wait <= 0 || this.shownRoot() === null || next === null) {
      this.lastApplied = now;
      this.shownRoot.set(next);
      return;
    }
    this.timer = setTimeout(() => {
      this.timer = null;
      this.lastApplied = this.clock();
      this.shownRoot.set(next);
    }, wait);
  }

  protected opacity(arc: PdArc): number | null {
    const lit = this.lit();
    return lit && !lit.has(arc.id) ? 0.32 : null;
  }

  /** Labels of faded segments are hidden: their ink was chosen for the full-strength fill. */
  protected labelOpacity(id: string): number | null {
    const lit = this.lit();
    return lit && !lit.has(id) ? 0 : null;
  }

  private fillOf(arc: PdArc): string {
    if (arc.kind === 'hidden') return `url(#${this.uid}-hidden)`;
    if (arc.kind === 'pending') return `url(#${this.uid}-pending)`;
    const base = `var(--pd-chart-${arc.slot >= 0 ? Math.min(arc.slot, 7) + 1 : 8})`;
    return arc.tone === 1 ? `color-mix(in srgb, ${base}, var(--pd-text) 12%)` : base;
  }

  /** The ink of a label on the segment's fill (the text or the surface token, whichever reads better), or null below 4.5:1. */
  private inkFor(arc: PdArc, tokens: ReturnType<typeof readPaddockTokens>, surface: ReturnType<typeof parseCssColor>, text: ReturnType<typeof parseCssColor>): string | null {
    const base = parseCssColor(tokens.palette[arc.slot >= 0 ? Math.min(arc.slot, 7) : 7] ?? '');
    if (!base || !surface || !text) return null;
    const mixed = arc.tone === 1 ? { r: base.r + (text.r - base.r) * 0.12, g: base.g + (text.g - base.g) * 0.12, b: base.b + (text.b - base.b) * 0.12 } : base;
    const fill = luminance(mixed.r, mixed.g, mixed.b);
    const withText = contrastRatio(fill, luminance(text.r, text.g, text.b));
    const withSurface = contrastRatio(fill, luminance(surface.r, surface.g, surface.b));
    const best = Math.max(withText, withSurface);
    if (best < 4.5) return null;
    return withText >= withSurface ? 'var(--pd-text)' : 'var(--pd-surface-1)';
  }

  private ariaOf(arc: PdArc, fmt: (v: number) => string): string {
    const parts = [arc.label, fmt(arc.value), `${percent(arc.value / (arc.parentValue || 1))} of ${arc.parentLabel}`];
    if (arc.node.detail) parts.push(arc.node.detail);
    return parts.join(', ');
  }

  private pointOf(arc: PdArc): PdSunburstPoint {
    const byId = this.arcsById();
    const path: string[] = [];
    for (let at: PdArc | undefined = arc; at; at = at.parent === null ? undefined : byId.get(at.parent)) path.unshift(at.id);
    return { node: arc.node, path, share: arc.parentValue > 0 ? Math.min(1, arc.value / arc.parentValue) : 0 };
  }

  protected hover(id: string): void {
    this.hoveredId.set(id);
    this.emitActive();
  }

  protected hoverOut(): void {
    if (this.hoveredId() === null) return;
    this.hoveredId.set(null);
    this.emitActive();
  }

  protected focusArc(id: string): void {
    this.focusedId.set(id);
    this.hasFocus.set(true);
    this.emitActive();
  }

  protected blurArc(): void {
    this.hasFocus.set(false);
    this.emitActive();
  }

  private emitActive(): void {
    const id = this.activeId();
    if (id === this.lastEmitted) return;
    this.lastEmitted = id;
    const arc = id ? this.arcsById().get(id) : undefined;
    this.hoverChange.emit(arc ? this.pointOf(arc) : null);
  }

  /** Drills into a folder, opens a folded segment as a list, or reports a segment that cannot be drilled. */
  protected activateArc(arc: PdArc): void {
    if (arc.kind === 'folded') {
      this.openOther.emit({ parent: arc.parentNodeId, ids: [...arc.foldedIds] });
    } else if (arc.kind === 'other') {
      this.openOther.emit({ parent: arc.parentNodeId, ids: [arc.id] });
    } else if (arc.kind === 'item' && arc.expandable) {
      this.centre.set(arc.id);
      if (arc.node.hasMore) this.loadChildren.emit(arc.id);
      this.hoveredId.set(null);
      this.focusedId.set(null);
      this.emitActive();
    } else {
      this.activate.emit(this.pointOf(arc));
    }
  }

  protected onKey(event: KeyboardEvent): void {
    const id = this.focusedId();
    const arc = id ? this.arcsById().get(id) : undefined;
    if (!arc || event.ctrlKey || event.metaKey || event.altKey) return;
    if (event.key === 'ContextMenu' || (event.key === 'F10' && event.shiftKey)) {
      event.preventDefault();
      // A segment in the model is always in the DOM (the same render), so the element exists.
      const box = (this.pathElement(arc.id) as SVGElement).getBoundingClientRect();
      this.contextMenu.emit({ point: this.pointOf(arc), x: box.left + box.width / 2, y: box.top + box.height / 2 });
      return;
    }
    if (event.key === 'Enter' || event.key === ' ') {
      event.preventDefault();
      const drills = arc.kind === 'item' && arc.expandable;
      this.activateArc(arc);
      if (drills) afterNextRender(() => this.focusFirst(), { injector: this.injector });
      return;
    }
    const target = nextFocus(this.layout()?.arcs ?? [], arc.id, event.key);
    if (target) {
      event.preventDefault();
      this.focusedId.set(target);
      afterNextRender(() => this.pathElement(target)?.focus(), { injector: this.injector });
    }
  }

  private focusFirst(): void {
    const first = this.layout()?.arcs[0];
    if (first) this.pathElement(first.id)?.focus();
  }

  protected onContext(event: MouseEvent, id: string | null): void {
    const arc = id ? this.arcsById().get(id) : undefined;
    if (!arc) return;
    event.preventDefault();
    this.contextMenu.emit({ point: this.pointOf(arc), x: event.clientX, y: event.clientY });
  }

  private pathElement(id: string): SVGElement | undefined {
    return Array.from(this.host.querySelectorAll<SVGElement>('path[role=treeitem]')).find((el) => el.getAttribute('data-id') === id);
  }
}

/** "55%", "0.4%": a whole percent from 10 up, one decimal below, and "<0.1%" for the tiniest. */
function percent(share: number): string {
  const p = Math.min(1, Math.max(0, Number.isFinite(share) ? share : 0)) * 100;
  if (p > 0 && p < 0.1) return '<0.1%';
  return `${p >= 10 ? Math.round(p) : Math.round(p * 10) / 10}%`;
}
