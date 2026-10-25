import { ChangeDetectionStrategy, Component, computed, ElementRef, inject, input, signal } from '@angular/core';
import { PdGlyph } from '@decryp7/paddock-ui/icons';
import type { Tree, TreeNode } from '../generated/view';
import { glyphFor, toneOf } from './model';
import { ViewRuntime } from './runtime';
import { PdViewTone, TONE_CSS, toneClass } from './tone';

/** One line of the shown tree. */
export interface TreeRow {
  readonly node: TreeNode;
  /** Index in the document's flat list. */
  readonly index: number;
  /** The next node is deeper: its children are in the list. */
  readonly hasChildren: boolean;
  readonly expanded: boolean;
  /** Position among the shown rows (1-based) and their count, for assistive technology. */
  readonly pos: number;
  /** Size of the set of siblings and place in it. */
  readonly setSize: number;
  readonly setPos: number;
}

/**
 * The rows of a flat, pre-order tree that are visible: a row is visible when every ancestor is expanded. A node with children in the
 * list is expanded unless the user collapsed it (a lazy tree starts collapsed: its expandable nodes are expanded only when chosen).
 */
export function visibleRows(tree: Tree, expanded: ReadonlySet<string>, collapsed: ReadonlySet<string>): readonly TreeRow[] {
  const nodes = tree.nodes;
  // Parents, sibling counts and places over the whole list (a node's siblings are all in the list whenever it is shown).
  const stack: number[] = [];
  const place: number[] = [];
  const sizes = new Map<number, number>();
  const parentOf: number[] = [];
  nodes.forEach((n, i) => {
    const parent = n.depth > 0 ? (stack[n.depth - 1] ?? -1) : -1;
    stack[n.depth] = i;
    parentOf[i] = parent;
    const count = (sizes.get(parent) ?? 0) + 1;
    sizes.set(parent, count);
    place[i] = count;
  });
  const rows: TreeRow[] = [];
  // The depth at or below which rows are hidden (a closed ancestor), or Infinity.
  let hiddenFrom = Infinity;
  nodes.forEach((n, index) => {
    if (n.depth >= hiddenFrom) return;
    hiddenFrom = Infinity;
    const hasChildren = (nodes[index + 1]?.depth ?? 0) > n.depth;
    const foldable = hasChildren || n.expandable;
    const isOpen = foldable && (tree.lazy ? expanded.has(n.id) : !collapsed.has(n.id));
    if (foldable && !isOpen) hiddenFrom = n.depth + 1;
    rows.push({ node: n, index, hasChildren, expanded: isOpen, pos: rows.length + 1, setSize: sizes.get(parentOf[index] ?? -1) ?? 1, setPos: place[index] ?? 1 });
  });
  return rows;
}

/**
 * A `Tree` node (flat pre-order list with depth). An expandable node of a lazy tree asks for its children with the action `expand`
 * (`node` = its id) the first time it opens; the plugin answers with a patch that puts them after it. A node with an action is a
 * button that sends it with `node`. Arrow keys move between rows, Right opens and Left closes, Home/End jump.
 */
@Component({
  selector: 'pd-view-tree',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [PdGlyph, PdViewTone],
  template: `
    <ul class="pd-view-tree" role="tree" [attr.aria-label]="label() || defaultLabel" (keydown)="key($event)">
      @for (row of rows(); track row.node.id) {
        <li
          role="treeitem"
          class="pd-view-tree__row"
          [class]="rowClass(row)"
          [attr.aria-level]="row.node.depth + 1"
          [attr.aria-posinset]="row.setPos"
          [attr.aria-setsize]="row.setSize"
          [attr.aria-expanded]="row.hasChildren || row.node.expandable ? row.expanded : null"
          [attr.data-node]="row.node.id"
          [style.--pd-view-depth]="row.node.depth"
          [tabindex]="row.node.id === tabStop() ? 0 : -1"
          (focus)="active.set(row.node.id)"
        >
          @if (row.hasChildren || row.node.expandable) {
            <button type="button" class="pd-view-tree__twisty" tabindex="-1" [attr.aria-label]="(row.expanded ? collapse : expand) + ': ' + row.node.label" [disabled]="runtime.disabled()" (click)="toggle(row)">
              <pd-glyph [name]="row.expanded ? 'chevron-down' : 'chevron-right'" [size]="14" />
            </button>
          } @else {
            <span class="pd-view-tree__twisty" aria-hidden="true"></span>
          }
          @if (row.node.icon) {
            <pd-glyph [name]="glyph(row.node)" [size]="16" />
          }
          <pd-view-tone [tone]="row.node.tone" />
          @if (row.node.action; as a) {
            <button type="button" class="pd-view-tree__label pd-view-tree__label--act" tabindex="-1" [disabled]="runtime.disabled()" (click)="runtime.run(a, { node: row.node.id })">{{ row.node.label }}</button>
          } @else {
            <span class="pd-view-tree__label">{{ row.node.label }}</span>
          }
        </li>
      }
    </ul>
  `,
  styles: `
    ${TONE_CSS}
    :host { display: block; min-width: 0; }
    .pd-view-tree { list-style: none; margin: 0; padding: 0; }
    .pd-view-tree__row { display: flex; align-items: center; gap: var(--pd-space-1); min-height: var(--pd-row-height); padding-inline-start: calc(var(--pd-view-depth, 0) * var(--pd-space-4)); color: var(--pd-view-tone, var(--pd-text)); border-radius: var(--pd-radius-control); }
    .pd-view-tree__row.pd-view-tone-neutral { color: var(--pd-text); }
    .pd-view-tree__row:hover { background: var(--pd-hover); }
    .pd-view-tree__row:focus-visible { outline: 2px solid var(--pd-focus-ring); outline-offset: -2px; }
    .pd-view-tree__twisty { flex: none; display: inline-flex; align-items: center; justify-content: center; width: 24px; height: 24px; padding: 0; border: 0; background: none; color: var(--pd-text-muted); cursor: pointer; border-radius: var(--pd-radius-control); }
    .pd-view-tree__twisty:focus-visible { outline: 2px solid var(--pd-focus-ring); }
    .pd-view-tree__label { min-width: 0; overflow-wrap: anywhere; }
    .pd-view-tree__label--act { padding: 0; border: 0; background: none; font: inherit; color: var(--pd-accent); cursor: pointer; text-align: start; min-height: 24px; }
    .pd-view-tree__label--act:hover:not(:disabled) { text-decoration: underline; }
    .pd-view-tree__label--act:focus-visible { outline: 2px solid var(--pd-focus-ring); border-radius: 2px; }
  `,
})
export class PdViewTree {
  protected readonly runtime = inject(ViewRuntime);
  private readonly host = inject<ElementRef<HTMLElement>>(ElementRef);
  readonly tree = input.required<Tree>();
  readonly label = input('');
  protected readonly defaultLabel = $localize`:@@pd.view.tree.label:Tree`;
  protected readonly expand = $localize`:@@pd.view.tree.expand:Expand`;
  protected readonly collapse = $localize`:@@pd.view.tree.collapse:Collapse`;
  /** Lazy trees: the nodes the user opened. */
  private readonly opened = signal<ReadonlySet<string>>(new Set());
  /** Eager trees: the nodes the user closed. */
  private readonly closed = signal<ReadonlySet<string>>(new Set());
  protected readonly rows = computed(() => visibleRows(this.tree(), this.opened(), this.closed()));
  /** The row that holds the tab stop (roving tabindex): the focused one, else the first. */
  protected readonly active = signal('');
  protected readonly tabStop = computed(() => (this.rows().some((r) => r.node.id === this.active()) ? this.active() : (this.rows()[0]?.node.id ?? '')));

  protected glyph(n: TreeNode): string {
    return glyphFor(n.icon);
  }
  protected rowClass(row: TreeRow): string {
    return `pd-view-tree__row ${toneClass(toneOf(row.node.tone))}`;
  }

  /** Opens or closes a node; opening a lazy node whose children are not in the list yet asks the plugin for them. */
  protected toggle(row: TreeRow): void {
    const lazy = this.tree().lazy;
    const set = lazy ? this.opened() : this.closed();
    const next = new Set(set);
    if (next.has(row.node.id)) next.delete(row.node.id);
    else next.add(row.node.id);
    (lazy ? this.opened : this.closed).set(next);
    // Opening a lazy node (the set now has it) without children in the list: ask for them.
    if (lazy && next.has(row.node.id) && !row.hasChildren) void this.runtime.run(expandAction(), { node: row.node.id });
  }

  protected key(e: KeyboardEvent): void {
    const items = Array.from(this.host.nativeElement.querySelectorAll<HTMLElement>('[role=treeitem]'));
    const current = (e.target as HTMLElement | null)?.closest<HTMLElement>('[role=treeitem]') ?? null;
    const at = current ? items.indexOf(current) : -1;
    const row = at >= 0 ? this.rows()[at] : undefined;
    const focus = (i: number): void => {
      const el = items[Math.max(0, Math.min(items.length - 1, i))];
      if (el) {
        el.focus();
        e.preventDefault();
      }
    };
    switch (e.key) {
      case 'ArrowDown':
        return focus(at + 1);
      case 'ArrowUp':
        return focus(at - 1);
      case 'Home':
        return focus(0);
      case 'End':
        return focus(items.length - 1);
      case 'ArrowRight':
        if (row && !row.expanded && (row.hasChildren || row.node.expandable)) {
          this.toggle(row);
          e.preventDefault();
        } else if (row?.expanded) focus(at + 1);
        return;
      case 'ArrowLeft':
        if (row?.expanded) {
          this.toggle(row);
          e.preventDefault();
        } else if (row) {
          // To the parent: the nearest row above at a smaller depth.
          for (let k = at - 1; k >= 0; k--) {
            if ((this.rows()[k]?.node.depth ?? 0) < row.node.depth) return focus(k);
          }
        }
        return;
      case 'Enter':
      case ' ':
        if (row?.node.action && e.target === items[at]) {
          void this.runtime.run(row.node.action, { node: row.node.id });
          e.preventDefault();
        } else if (row && (row.hasChildren || row.node.expandable) && e.target === items[at]) {
          this.toggle(row);
          e.preventDefault();
        }
        return;
    }
  }
}

/** The host's own action `expand` (a lazy tree asks the plugin for the children of `node`). */
const expandAction = () => ({ id: 'expand', label: 'expand', confirm: null, args: null, writes: false });
