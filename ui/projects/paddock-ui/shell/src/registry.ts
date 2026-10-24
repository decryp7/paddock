import { inject, Injectable } from '@angular/core';
import type {
  CommandDef,
  DialogDef,
  HealthServiceDef,
  KeybindingDef,
  MenuItemDef,
  NotificationKindDef,
  PageDef,
  SettingsPageDef,
  StatusItemDef,
} from './define';
import { ContributionKind, ContributionLookup, SHELL_KINDS } from './kinds';
import { SHELL_CONTRIBUTION_KINDS, SHELL_CONTRIBUTIONS } from './tokens';

/**
 * Every contribution of the window, by kind (one of the root singletons ADR 0012 §4 allows). Contributions are static, so
 * the lists are plain arrays computed once. A duplicate identity keeps the first registration; the lint reports it.
 */
@Injectable({ providedIn: 'root' })
export class ContributionRegistry implements ContributionLookup {
  private readonly entries = inject(SHELL_CONTRIBUTIONS, { optional: true }) ?? [];
  private readonly extraKinds = (inject(SHELL_CONTRIBUTION_KINDS, { optional: true }) ?? []).flat();
  /** The shell's kinds, then the added ones; an added kind with a taken name is ignored (and reported by the lint). */
  readonly kinds: readonly ContributionKind[];
  private readonly lists = new Map<string, readonly unknown[]>();
  private readonly identities = new Map<string, Set<string>>();

  constructor() {
    const kinds = new Map<string, ContributionKind>();
    for (const k of [...SHELL_KINDS, ...this.extraKinds]) if (!kinds.has(k.name)) kinds.set(k.name, k);
    this.kinds = [...kinds.values()];
    for (const kind of this.kinds) {
      const seen = new Set<string>();
      const list: unknown[] = [];
      for (const item of this.raw(kind.name)) {
        const id = safeIdentity(kind, item);
        if (id === null || seen.has(id)) continue;
        seen.add(id);
        list.push(item);
      }
      this.lists.set(kind.name, Object.freeze(list));
      this.identities.set(kind.name, seen);
    }
  }

  /** Every registered item of a kind, duplicates included, in registration order (for the lint). */
  raw(kind: string): readonly unknown[] {
    return this.entries.filter((e) => e.kind === kind).flatMap((e) => e.items);
  }

  /** Batch keys that name no registered kind (a typo in `provideContributions`, or a kind whose provider is missing). */
  unknownKinds(): readonly string[] {
    return [...new Set(this.entries.map((e) => e.kind))].filter((k) => !this.lists.has(k));
  }

  /** The kinds added with `provideContributionKinds` whose name an earlier kind already had. */
  shadowedKinds(): readonly string[] {
    return this.extraKinds.filter((k) => !this.kinds.includes(k)).map((k) => k.name);
  }

  items<I = unknown>(kind: string): readonly I[] {
    return (this.lists.get(kind) ?? []) as readonly I[];
  }

  has(kind: string, identity: string): boolean {
    return this.identities.get(kind)?.has(identity) ?? false;
  }

  find<I = unknown>(kind: string, identity: string): I | undefined {
    const k = this.kinds.find((x) => x.name === kind);
    return k ? (this.items<I>(kind).find((i) => safeIdentity(k, i) === identity) as I | undefined) : undefined;
  }

  commands(): readonly CommandDef[] {
    return this.items('commands');
  }
  keybindings(): readonly KeybindingDef[] {
    return this.items('keybindings');
  }
  menus(): readonly MenuItemDef[] {
    return this.items('menus');
  }
  pages(): readonly PageDef[] {
    return this.items('pages');
  }
  statusItems(): readonly StatusItemDef[] {
    return this.items('statusItems');
  }
  settingsPages(): readonly SettingsPageDef[] {
    return this.items('settingsPages');
  }
  dialogs(): readonly DialogDef[] {
    return this.items('dialogs');
  }
  healthServices(): readonly HealthServiceDef[] {
    return this.items('healthServices');
  }
  notificationKinds(): readonly NotificationKindDef[] {
    return this.items('notificationKinds');
  }
}

/** A kind's identity of an item, or `null` when the item is unusable (not an object, or its identity throws). */
export function safeIdentity(kind: ContributionKind, item: unknown): string | null {
  if (typeof item !== 'object' || item === null) return null;
  try {
    return String((kind as ContributionKind<unknown>).identity(item));
  } catch {
    return null;
  }
}
