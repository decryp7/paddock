import { CdkContextMenuTrigger, CdkMenu, CdkMenuItem } from '@angular/cdk/menu';
import { ChangeDetectionStrategy, Component, computed, effect, ElementRef, inject, signal, untracked } from '@angular/core';
import { PdPathText } from '@decryp7/paddock-ui';
import { CommandService, evaluate, PdIcon, ShellContextHost } from '@decryp7/paddock-ui/shell';
import type { ExplorerAction, ExplorerNode, ExplorerProviderDef } from '@decryp7/paddock-ui/workbench';
import { ExplorerStore, treeKey, type TreeRow } from './explorer.store';

/** The node a context menu was opened for. */
interface MenuTarget {
  readonly section: string;
  readonly node: ExplorerNode;
  readonly actions: readonly ExplorerAction[];
}

/** Indent per level, px. */
const INDENT = 12;

/**
 * The Explorer side bar (plan W4; the frame of Debrief's `shell/sidebar.ts`): one collapsible section per contributed
 * Explorer provider, each a lazy, paged tree (`ExplorerStore`). A WAI-ARIA tree with one tab stop: arrows move and
 * expand, Enter opens, Shift+F10 or the context-menu key opens the node's actions. Names are truncated in the middle with
 * the full value in a tooltip (rule 18); nodes a provider marks as draggable carry its drag data.
 */
@Component({
  selector: 'pd-explorer',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [PdIcon, PdPathText, CdkContextMenuTrigger, CdkMenu, CdkMenuItem],
  host: { class: 'pd-explorer' },
  template: `
    @for (section of store.sections(); track section.id) {
      <section class="pd-ex-section" [attr.data-explorer-section]="section.id">
        <div class="pd-ex-head">
          <button type="button" class="pd-ex-title" [attr.aria-expanded]="!store.isSectionCollapsed(section.id)" (click)="store.toggleSection(section.id)">
            <pd-icon [name]="store.isSectionCollapsed(section.id) ? 'pd:chevron-right' : 'pd:chevron-down'" />
            <span>{{ section.title.text }}</span>
          </button>
          <button type="button" class="pd-ex-icon-btn" [attr.aria-label]="refreshLabel(section)" [attr.title]="refreshLabel(section)" (click)="store.refresh(section.id)">
            <pd-icon name="pd:refresh" />
          </button>
        </div>
        @if (!store.isSectionCollapsed(section.id)) {
          @if (isEmpty(section)) {
            <div class="pd-ex-empty" data-testid="explorer-empty">
              <p>{{ section.empty?.text?.text ?? t.empty }}</p>
              @if (section.empty?.command; as command) {
                <button type="button" class="pd-ex-cta" (click)="runCommand(command)">{{ commandTitle(command) }}</button>
              }
            </div>
          } @else {
            <div
              role="tree"
              class="pd-ex-tree"
              [attr.aria-label]="section.title.text"
              [cdkContextMenuTriggerFor]="menu"
              #trigger="cdkContextMenuTriggerFor"
              (keydown)="onKey($event, section.id, trigger)"
            >
              @for (row of rowsOf(section.id); track row.key) {
                @switch (row.kind) {
                  @case ('node') {
                    <div
                      role="treeitem"
                      class="pd-ex-row"
                      [attr.data-node-id]="row.node.id"
                      [attr.aria-level]="row.depth + 1"
                      [attr.aria-expanded]="row.node.expandable ? row.expanded : null"
                      [attr.aria-selected]="store.selected() === row.key"
                      [attr.aria-disabled]="row.node.problem ? 'true' : null"
                      [attr.tabindex]="focusKey() === row.key ? 0 : -1"
                      [attr.draggable]="draggable(row) ? 'true' : null"
                      [class.pd-selected]="store.selected() === row.key"
                      [class.pd-problem]="!!row.node.problem"
                      [style.padding-inline-start.px]="8 + row.depth * indent"
                      [attr.title]="row.node.problem ?? null"
                      (click)="click(row)"
                      (dblclick)="store.open(row.section, row.node)"
                      (contextmenu)="target.set(menuTarget(row))"
                      (dragstart)="dragStart($event, row)"
                    >
                      <span class="pd-ex-twisty" aria-hidden="true" (click)="toggle(row, $event)">
                        @if (row.node.expandable) {
                          <pd-icon [name]="row.expanded ? 'pd:chevron-down' : 'pd:chevron-right'" />
                        }
                      </span>
                      @if (row.node.icon) {
                        <pd-icon class="pd-ex-node-icon" [name]="row.node.icon" />
                      }
                      @if (row.node.truncate === 'end') {
                        <span class="pd-ex-label">{{ row.node.label }}</span>
                      } @else {
                        <pd-path-text class="pd-ex-path" [value]="row.node.label" [full]="row.node.tooltip ?? null" [plain]="true" />
                      }
                      @if (row.node.description) {
                        <span class="pd-ex-desc">{{ row.node.description }}</span>
                      }
                      @if (row.node.badge) {
                        <span class="pd-ex-badge" [attr.aria-label]="row.node.badgeLabel ?? null" [attr.title]="row.node.badgeLabel ?? null">{{ row.node.badge }}</span>
                      }
                    </div>
                  }
                  @case ('more') {
                    <button type="button" class="pd-ex-more" [style.padding-inline-start.px]="28 + row.depth * indent" data-testid="explorer-more" (click)="store.more(row.section, row.parent)">
                      {{ moreLabel(row.remaining) }}
                    </button>
                  }
                  @case ('status') {
                    @if (row.status === 'loading') {
                      <div class="pd-ex-status" role="status" [style.padding-inline-start.px]="28 + row.depth * indent" data-testid="explorer-loading">{{ t.loading }}</div>
                    } @else {
                      <div class="pd-ex-status pd-ex-error" [style.padding-inline-start.px]="28 + row.depth * indent" data-testid="explorer-error">
                        <span>{{ row.status.error }}</span>
                        <button type="button" class="pd-ex-cta" (click)="store.retry(row.section, row.parent)">{{ t.retry }}</button>
                      </div>
                    }
                  }
                }
              }
            </div>
          }
        }
      </section>
    } @empty {
      <p class="pd-ex-empty">{{ t.none }}</p>
    }
    <ng-template #menu>
      <div cdkMenu class="pd-ex-menu" [attr.aria-label]="t.actions" data-testid="explorer-menu">
        @for (action of target()?.actions ?? []; track action.id; let i = $index) {
          @if (i > 0 && action.group !== (target()?.actions ?? [])[i - 1]?.group) {
            <div class="pd-ex-sep" role="separator"></div>
          }
          <button type="button" cdkMenuItem class="pd-ex-mi" [class.pd-danger]="action.danger" [cdkMenuItemDisabled]="!!action.disabled" [attr.title]="action.disabled ?? null" (cdkMenuItemTriggered)="runAction(action)">
            @if (action.icon) {
              <pd-icon [name]="action.icon" />
            }
            <span>{{ action.label }}</span>
          </button>
        } @empty {
          <div class="pd-ex-mi pd-ex-none">{{ t.noActions }}</div>
        }
      </div>
    </ng-template>
  `,
  styles: `
    :host { display: flex; flex-direction: column; min-width: 0; min-height: 0; overflow-x: hidden; overflow-y: auto; font-size: var(--pd-text-body); color: var(--pd-text); container: pd-explorer / inline-size; }
    .pd-ex-head { display: flex; align-items: center; gap: var(--pd-space-1); min-width: 0; padding: var(--pd-space-1) var(--pd-space-2); position: sticky; top: 0; background: var(--pd-surface-1); z-index: 1; }
    .pd-ex-title { flex: 1; display: flex; align-items: center; gap: var(--pd-space-1); min-width: 0; height: 24px; padding: 0 var(--pd-space-1); border: 0; border-radius: var(--pd-radius-control); background: transparent; color: var(--pd-text-muted); font: inherit; font-size: var(--pd-text-label); font-weight: var(--pd-weight-semibold); text-transform: uppercase; letter-spacing: 0.04em; cursor: pointer; text-align: start; }
    .pd-ex-title span { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .pd-ex-icon-btn { display: inline-flex; align-items: center; justify-content: center; width: 24px; height: 24px; padding: 0; border: 0; border-radius: var(--pd-radius-control); background: transparent; color: var(--pd-text-muted); cursor: pointer; }
    .pd-ex-icon-btn:hover, .pd-ex-title:hover { background: var(--pd-hover); color: var(--pd-text); }
    button:focus-visible, .pd-ex-row:focus-visible { outline: 2px solid var(--pd-focus-ring); outline-offset: -2px; }
    .pd-ex-tree { display: flex; flex-direction: column; min-width: 0; padding-bottom: var(--pd-space-2); }
    .pd-ex-row { display: flex; align-items: center; gap: var(--pd-space-1); min-width: 0; height: 24px; padding-inline-end: var(--pd-space-2); cursor: default; outline: none; }
    .pd-ex-row:hover { background: var(--pd-hover); }
    .pd-ex-row.pd-selected { background: var(--pd-accent-tint); box-shadow: inset 2px 0 0 var(--pd-accent); }
    .pd-ex-row.pd-problem { color: var(--pd-text-subtle); }
    .pd-ex-twisty { flex: none; display: inline-flex; width: 16px; height: 16px; align-items: center; justify-content: center; color: var(--pd-text-muted); }
    .pd-ex-node-icon { flex: none; }
    .pd-ex-path, .pd-ex-label { flex: 1 1 auto; min-width: 0; }
    .pd-ex-label { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .pd-ex-desc { flex: 0 1 auto; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--pd-text-muted); font-size: var(--pd-text-label); }
    .pd-ex-badge { flex: none; min-width: 16px; padding: 0 4px; border-radius: var(--pd-radius-pill); background: var(--pd-surface-3); color: var(--pd-text-muted); font-size: var(--pd-text-caption); text-align: center; }
    .pd-ex-more { align-self: stretch; height: 24px; border: 0; background: transparent; color: var(--pd-accent); font: inherit; font-size: var(--pd-text-label); text-align: start; cursor: pointer; }
    .pd-ex-status { display: flex; align-items: center; gap: var(--pd-space-2); min-height: 24px; color: var(--pd-text-muted); font-size: var(--pd-text-label); }
    .pd-ex-error { color: var(--pd-danger); flex-wrap: wrap; }
    .pd-ex-empty { margin: 0; padding: var(--pd-space-2) var(--pd-space-4); color: var(--pd-text-muted); font-size: var(--pd-text-label); }
    .pd-ex-empty p { margin: 0 0 var(--pd-space-2); }
    .pd-ex-cta { height: 24px; padding: 0 var(--pd-space-2); border: 1px solid var(--pd-border-control); border-radius: var(--pd-radius-control); background: var(--pd-surface-2); color: var(--pd-text); font: inherit; font-size: var(--pd-text-label); cursor: pointer; }
    .pd-ex-menu { display: flex; flex-direction: column; min-width: 180px; padding: var(--pd-space-1); border: 1px solid var(--pd-border-strong); border-radius: var(--pd-radius-popover); background: var(--pd-surface-2); box-shadow: var(--pd-shadow-2); }
    .pd-ex-mi { display: flex; align-items: center; gap: var(--pd-space-2); height: 28px; padding: 0 var(--pd-space-2); border: 0; border-radius: var(--pd-radius-control); background: transparent; color: var(--pd-text); font: inherit; text-align: start; cursor: pointer; }
    .pd-ex-mi:hover:not([aria-disabled='true']), .pd-ex-mi:focus { background: var(--pd-hover); outline: none; }
    .pd-ex-mi[aria-disabled='true'], .pd-ex-none { color: var(--pd-text-subtle); cursor: default; }
    .pd-ex-mi.pd-danger { color: var(--pd-danger); }
    .pd-ex-sep { height: 1px; margin: var(--pd-space-1) 0; background: var(--pd-hairline); }
  `,
})
export class PdExplorer {
  protected readonly store = inject(ExplorerStore);
  private readonly commands = inject(CommandService);
  private readonly el = inject<ElementRef<HTMLElement>>(ElementRef).nativeElement;
  protected readonly indent = INDENT;
  protected readonly target = signal<MenuTarget | null>(null);
  protected readonly t = {
    empty: $localize`:@@pd.wb.explorer.empty:Nothing here yet.`,
    none: $localize`:@@pd.wb.explorer.none:Nothing to explore.`,
    loading: $localize`:@@pd.wb.explorer.loading:Loading…`,
    retry: $localize`:@@pd.wb.explorer.retry:Try again`,
    actions: $localize`:@@pd.wb.explorer.actions:Actions`,
    noActions: $localize`:@@pd.wb.explorer.no-actions:No actions`,
  };

  /** The row that takes Tab: the selected one, else the first node. */
  protected readonly focusKey = computed(() => {
    const nodes = this.store.rows().filter((r) => r.kind === 'node');
    const selected = this.store.selected();
    return nodes.some((r) => r.key === selected) ? selected : (nodes[0]?.key ?? null);
  });

  constructor() {
    this.store.ensureRoots();
    const host = inject(ShellContextHost);
    const seen = new Map<string, unknown>();
    // Reloading a section is a side effect (a provider call, usually IPC) of a tracked `changes` value; the first value
    // of each section is only remembered (its roots are loading already).
    effect(() => {
      const ctx = host.context();
      for (const section of this.store.sections()) {
        if (!section.changes) continue;
        const value = evaluate(section.changes, ctx, undefined);
        const had = seen.has(section.id);
        const before = seen.get(section.id);
        seen.set(section.id, value);
        if (had && !Object.is(before, value)) untracked(() => void this.store.refresh(section.id));
      }
    });
  }

  protected rowsOf(section: string): TreeRow[] {
    return this.store.rows().filter((r) => r.section === section);
  }

  protected isEmpty(section: ExplorerProviderDef): boolean {
    const list = this.store.lists()[treeKey(section.id, '')];
    return list?.status === 'loaded' && list.nodes.length === 0;
  }

  protected refreshLabel(section: ExplorerProviderDef): string {
    return $localize`:@@pd.wb.explorer.refresh:Refresh ${section.title.text}:section:`;
  }

  protected moreLabel(remaining: number | null): string {
    return remaining === null ? $localize`:@@pd.wb.explorer.more:Show more` : $localize`:@@pd.wb.explorer.more-of:Show more (${remaining}:count: left)`;
  }

  protected commandTitle(id: string): string {
    return this.commands.find(id)?.title.text ?? id;
  }

  protected runCommand(id: string): void {
    void this.commands.run(id);
  }

  protected draggable(row: Extract<TreeRow, { kind: 'node' }>): boolean {
    return this.store.dragData(row.section, row.node) !== null;
  }

  protected dragStart(e: DragEvent, row: Extract<TreeRow, { kind: 'node' }>): void {
    const data = this.store.dragData(row.section, row.node);
    if (!data || !e.dataTransfer) return;
    e.dataTransfer.setData(data.type, data.data);
    e.dataTransfer.effectAllowed = 'copyLink';
  }

  protected menuTarget(row: Extract<TreeRow, { kind: 'node' }>): MenuTarget {
    this.store.select(row.key);
    return { section: row.section, node: row.node, actions: this.store.actions(row.section, row.node) };
  }

  protected runAction(action: ExplorerAction): void {
    const t = this.target();
    if (t && !action.disabled) void this.store.run(t.section, action.id, t.node);
  }

  protected click(row: Extract<TreeRow, { kind: 'node' }>): void {
    this.store.select(row.key);
  }

  protected toggle(row: Extract<TreeRow, { kind: 'node' }>, e: Event): void {
    e.stopPropagation();
    this.store.select(row.key);
    if (row.expanded) this.store.collapse(row.section, row.node);
    else this.store.expand(row.section, row.node);
  }

  protected onKey(e: KeyboardEvent, section: string, trigger: CdkContextMenuTrigger): void {
    const nodes = this.rowsOf(section).filter((r): r is Extract<TreeRow, { kind: 'node' }> => r.kind === 'node');
    if (!nodes.length) return;
    const at = Math.max(
      0,
      nodes.findIndex((r) => r.key === this.store.selected()),
    );
    const row = nodes[at] as Extract<TreeRow, { kind: 'node' }>;
    const go = (i: number): void => {
      const next = nodes[Math.max(0, Math.min(nodes.length - 1, i))] as Extract<TreeRow, { kind: 'node' }>;
      this.store.select(next.key);
      queueMicrotask(() => this.el.querySelector<HTMLElement>(`[data-explorer-section="${CSS.escape(section)}"] [data-node-id="${CSS.escape(next.node.id)}"]`)?.focus());
    };
    switch (e.key) {
      case 'ArrowDown':
        go(at + 1);
        break;
      case 'ArrowUp':
        go(at - 1);
        break;
      case 'Home':
        go(0);
        break;
      case 'End':
        go(nodes.length - 1);
        break;
      case 'ArrowRight':
        if (row.node.expandable && !row.expanded) this.store.expand(section, row.node);
        else if (row.expanded) go(at + 1);
        break;
      case 'ArrowLeft':
        if (row.expanded) this.store.collapse(section, row.node);
        else if (row.parentKey) go(nodes.findIndex((r) => r.key === row.parentKey));
        break;
      case 'Enter':
        void this.store.open(section, row.node);
        break;
      case 'ContextMenu':
      case 'F10':
        if (e.key === 'F10' && !e.shiftKey) return;
        this.openMenuFor(row, e.target as HTMLElement | null, trigger);
        break;
      default:
        return;
    }
    e.preventDefault();
  }

  /** Opens the actions of a row from the keyboard, at the row. */
  private openMenuFor(row: Extract<TreeRow, { kind: 'node' }>, from: HTMLElement | null, trigger: CdkContextMenuTrigger): void {
    this.target.set(this.menuTarget(row));
    const rect = from?.getBoundingClientRect();
    trigger.open({ x: (rect?.left ?? 0) + 24, y: rect?.bottom ?? 0 });
  }
}
