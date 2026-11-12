import { EnvironmentProviders, makeEnvironmentProviders, Provider } from '@angular/core';
import { provideShellCore } from './commands';
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
import type { ContributionKind } from './kinds';
import { SHELL_CONTRIBUTION_KINDS, SHELL_CONTRIBUTIONS } from './tokens';

/**
 * Everything an app (or a feature of it) contributes to the shell. `T` is the app's part of the context
 * (`ShellContext<T>`), so `run: (ctx) => ctx.models.refresh()` is type-checked. All parts are optional.
 */
export interface ContributionBatch<T extends object = object> {
  readonly commands?: readonly CommandDef<T>[];
  readonly keybindings?: readonly KeybindingDef[];
  readonly menus?: readonly MenuItemDef<T>[];
  readonly pages?: readonly PageDef<T>[];
  readonly statusItems?: readonly StatusItemDef<T>[];
  readonly settingsPages?: readonly SettingsPageDef<T>[];
  readonly dialogs?: readonly DialogDef<string, unknown, string>[];
  readonly healthServices?: readonly HealthServiceDef[];
  readonly notificationKinds?: readonly NotificationKindDef[];
}

/** The batch of one kind as providers; how other layers (the workbench) contribute through the same registry. */
export function contributionProviders(kind: string, items: readonly unknown[]): Provider[] {
  return items.length ? [{ provide: SHELL_CONTRIBUTIONS, multi: true, useValue: Object.freeze({ kind, items: Object.freeze([...items]) }) }] : [];
}

/**
 * Registers a batch of contributions: one line per feature in the window's providers, typically
 * `provideContributions<AppContext>(MODELS_CONTRIBUTIONS)` with the batch in a `*.contributions.ts` file that imports
 * only `define*` and types (plan §9). It includes the shell core (`provideShellCore`), so `ctx.runCommand` works.
 */
export function provideContributions<T extends object = object>(batch: ContributionBatch<T>): EnvironmentProviders {
  return makeEnvironmentProviders([
    provideShellCore(),
    ...Object.entries(batch).flatMap(([kind, items]) => contributionProviders(kind, (items as readonly unknown[] | undefined) ?? [])),
  ]);
}

/** Adds contribution kinds to the registry (the workbench's views, activity items, ...). */
export function provideContributionKinds(...kinds: readonly ContributionKind[]): EnvironmentProviders {
  return makeEnvironmentProviders([{ provide: SHELL_CONTRIBUTION_KINDS, multi: true, useValue: Object.freeze([...kinds]) }]);
}
