import { computed, inject, Injectable, type Signal } from '@angular/core';
import { CommandService, evaluate, SHELL_PORTS } from '@decryp7/paddock-ui/shell';
import { formatKey, Keymap } from '@decryp7/paddock-ui/shell/menu';
import { isOffered, WORKBENCH_PORTS, WorkbenchRegistry, type ActivityItemDef, type ActivityMenuEntry, type ActivityTarget } from '@decryp7/paddock-ui/workbench';

/** Height of one activity button, the gap between two, and the bar's vertical padding, px (ported from Debrief). */
export const ACTIVITY_BUTTON_PX = 36;
export const ACTIVITY_GAP_PX = 6;
export const ACTIVITY_PADDING_PX = 16;

/**
 * How many of the `top` buttons fit in a bar `height` px tall that also holds `bottom` buttons. When not all fit, one slot
 * goes to the More button. An unknown height (0, before layout) shows every button. Pure.
 */
export function fittingTopItems(top: number, bottom: number, height: number): number {
  if (!(height > 0)) return top;
  const slots = Math.floor((height - ACTIVITY_PADDING_PX + ACTIVITY_GAP_PX) / (ACTIVITY_BUTTON_PX + ACTIVITY_GAP_PX)) - bottom;
  if (top <= slots) return top;
  return Math.max(0, slots - 1);
}

/** A badge as drawn: the text, at most four characters (`999+`). */
export function badgeText(value: string | number | null | undefined): string | null {
  if (value === null || value === undefined || value === '') return null;
  if (typeof value === 'number') {
    if (!Number.isFinite(value) || value <= 0) return null;
    return value > 999 ? '999+' : String(Math.floor(value));
  }
  return String(value).slice(0, 4);
}

/**
 * The activity bar's items and their live state (plan W4; Debrief's `extensions/activity-items.ts`): visibility by
 * `when` and by whether the item's view or Explorer section is offered, badges, active, pulse, disabled, fly-out
 * menus; every live function is evaluated defensively (a throwing one hides that item or drops that extra, never the
 * bar). Running an item goes through `WORKBENCH_PORTS` and the command service.
 */
@Injectable()
export class ActivityItems {
  private readonly registry = inject(WorkbenchRegistry);
  private readonly ports = inject(WORKBENCH_PORTS);
  private readonly commands = inject(CommandService);
  private readonly keymap = inject(Keymap);
  private readonly shell = inject(SHELL_PORTS);

  private readonly visible = computed(() => {
    const ctx = this.registry.context();
    const views = new Set(this.registry.availableViews().map((v) => v.id));
    const sections = new Set(this.registry.availableExplorerProviders().map((p) => p.id));
    const commands = new Set(this.commands.available().map((c) => c.id));
    return this.registry.activityItems.filter((item) => {
      if (!isOffered(item.when, ctx)) return false;
      const run = item.run;
      if ('view' in run) return views.has(run.view);
      if ('explorer' in run) return sections.has(run.explorer);
      if ('command' in run) return commands.has(run.command);
      return true;
    });
  });

  readonly top: Signal<readonly ActivityItemDef[]> = computed(() => this.visible().filter((i) => i.section !== 'bottom'));
  readonly bottom: Signal<readonly ActivityItemDef[]> = computed(() => this.visible().filter((i) => i.section === 'bottom'));

  hasMenu(item: ActivityItemDef): boolean {
    return 'menu' in item.run;
  }

  /** The fly-out's entries, built now (a throwing builder gives none). */
  menu(item: ActivityItemDef): readonly ActivityMenuEntry[] {
    if (!('menu' in item.run)) return [];
    const entries = evaluate(item.run.menu, this.registry.context(), [] as readonly ActivityMenuEntry[]);
    return Array.isArray(entries) ? entries.slice(0, 50) : [];
  }

  /** Highlighted: `active`, else its view is open or its Explorer section is shown. */
  isActive(item: ActivityItemDef): boolean {
    const ctx = this.registry.context();
    if (item.active) return evaluate(item.active, ctx, false) === true;
    const run = item.run;
    if ('view' in run) return this.ports.openPanels().some((p) => p === run.view || p.startsWith(`${run.view}#`));
    if ('explorer' in run) return this.ports.explorerShown() === run.explorer;
    return false;
  }

  badge(item: ActivityItemDef): string | null {
    return badgeText(evaluate(item.badge, this.registry.context(), null));
  }

  badgeLabel(item: ActivityItemDef): string | null {
    return evaluate(item.badgeLabel, this.registry.context(), null);
  }

  pulse(item: ActivityItemDef): boolean {
    return evaluate(item.pulse, this.registry.context(), false) === true;
  }

  disabledReason(item: ActivityItemDef): string | null {
    const reason = evaluate(item.disabled, this.registry.context(), null);
    return typeof reason === 'string' && reason ? reason : null;
  }

  /** The key shown for the item: its own, else its command's bound key. */
  keybinding(item: Pick<ActivityItemDef, 'keybinding' | 'run'> | ActivityMenuEntry): string | null {
    const own = item.keybinding;
    const run = (item as { run?: unknown }).run;
    const command = typeof run === 'object' && run !== null && 'command' in run ? (run as { command: string }).command : null;
    const key = own ?? (command ? this.keymap.keyFor(command) : undefined);
    return key ?? null;
  }

  /** The key as shown in the tooltip (`⌘B`, `Ctrl+B`). */
  keyHint(item: Pick<ActivityItemDef, 'keybinding' | 'run'> | ActivityMenuEntry): string {
    const key = this.keybinding(item);
    return key ? formatKey(key, this.keymap.platform) : '';
  }

  /** Runs a target. A toggle of a shown Explorer section hides the side bar; a shown view's item focuses it. */
  async run(target: ActivityTarget): Promise<boolean> {
    try {
      if ('view' in target) return await this.ports.showView(target.view);
      if ('explorer' in target) {
        // The item of the section on show closes the side bar (a toggle, as in other IDEs).
        if (this.ports.explorerShown() === target.explorer) this.ports.hideExplorer();
        else this.ports.showExplorer(target.explorer);
        return true;
      }
      return await this.commands.run(target.command, target.args);
    } catch (e) {
      this.shell.notify({ level: 'error', message: e instanceof Error ? e.message : String(e) });
      return false;
    }
  }

  /** Activates a button (not a fly-out): nothing while disabled. */
  activate(item: ActivityItemDef): Promise<boolean> {
    if (this.disabledReason(item) || 'menu' in item.run) return Promise.resolve(false);
    return this.run(item.run);
  }
}
