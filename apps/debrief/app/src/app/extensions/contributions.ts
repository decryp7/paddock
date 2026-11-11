import { EnvironmentProviders, inject, InjectionToken, Injector, makeEnvironmentProviders, Provider, runInInjectionContext } from '@angular/core';
import {
  CONTRIBUTION_ID,
  defineContributionKind,
  defineKeybinding,
  SHELL_CONTRIBUTION_KINDS,
  SHELL_CONTRIBUTIONS,
  type CommandDef,
  type ContributionEntry,
  type ContributionKind,
  type KeybindingDef,
  type Message,
} from '@decryp7/paddock-ui/shell/contributions';
import type { DebriefContext } from './context';
import { missingTokens } from '../theme/theme-tokens';
import type { ThemeDef } from '../theme/theme.types';
import type { SharedStateDef } from '../popout/shared-state';
import type { DebriefActivityItem, DebriefCommand, DebriefFieldFormatter, DebriefInspectorSection, DebriefView } from './tokens';

/*
 * Debrief's contributions go through the shell's one registry (`SHELL_CONTRIBUTIONS`, workbench.md §4.3 M1): commands and
 * their keys as the shell's `commands` and `keybindings` kinds, and Debrief's own kinds (views, inspector sections, field
 * formatters, activity items) added to the same registry with `SHELL_CONTRIBUTION_KINDS`, never a second mechanism. The
 * old `DEBRIEF_*` multi-provider tokens are gone. M3 replaces Debrief's views and activity items with the workbench's
 * `defineView` and `defineActivityItem`.
 */

/** A Debrief command as the shell's registry holds it: the shell's `CommandDef` plus Debrief's two extras. */
export interface DebriefCommandDef extends CommandDef<DebriefContext> {
  /** The key the command declares (`Mod+Shift+P`); also registered as a `keybindings` contribution. */
  readonly keybinding?: string;
  /** The view this command only shows (the palette lists one command per view). */
  readonly view?: string;
}

/** The batch keys of Debrief's own kinds in the shell's registry. */
export const DEBRIEF_KIND = Object.freeze({
  views: 'debriefViews',
  inspectorSections: 'debriefInspectorSections',
  fieldFormatters: 'debriefFieldFormatters',
  activityItems: 'debriefActivityItems',
  themes: 'debriefThemes',
  sharedState: 'debriefSharedState',
} as const);

/** What each Debrief batch key holds (the authoring types). */
export interface DebriefBatchItems {
  readonly commands: DebriefCommand;
  readonly views: DebriefView;
  readonly inspectorSections: DebriefInspectorSection;
  readonly fieldFormatters: DebriefFieldFormatter;
  readonly activityItems: DebriefActivityItem;
  readonly themes: ThemeDef;
  readonly sharedState: SharedStateDef;
}
export type DebriefBatchKey = keyof DebriefBatchItems;

const issues = (...checks: (string | false | null | undefined)[]): string[] => checks.filter((c): c is string => typeof c === 'string');
const textProblem = (field: string, value: unknown): string | false => (typeof value !== 'string' || !value.trim()) && `has no ${field}`;
const fnProblem = (field: string, value: unknown): string | false => typeof value !== 'function' && `has no ${field} function`;

/** Debrief's kinds, checked by the shell's lint (`assertContributionsValid`) like the shell's own. */
export const DEBRIEF_KINDS: readonly ContributionKind[] = Object.freeze([
  defineContributionKind<DebriefView>({
    name: DEBRIEF_KIND.views,
    label: 'Debrief view',
    identity: (v) => v.id,
    check: (v) => issues(textProblem('id', v.id), textProblem('title', v.title), textProblem('icon', v.icon), fnProblem('component', v.component)),
  }),
  defineContributionKind<DebriefInspectorSection>({
    name: DEBRIEF_KIND.inspectorSections,
    label: 'inspector section',
    identity: (s) => s.id,
    check: (s) => issues(textProblem('id', s.id), textProblem('title', s.title), (typeof s.order !== 'number' || !Number.isFinite(s.order)) && 'order is not a number'),
  }),
  defineContributionKind<DebriefFieldFormatter>({
    name: DEBRIEF_KIND.fieldFormatters,
    label: 'field formatter',
    identity: (f) => (typeof f.field === 'string' ? f.field : `/${f.field.source}/${f.field.flags}`),
    check: (f) => issues(fnProblem('format', f.format)),
  }),
  defineContributionKind<DebriefActivityItem>({
    name: DEBRIEF_KIND.activityItems,
    label: 'activity item',
    identity: (a) => a.id,
    check: (a) => issues(textProblem('id', a.id), textProblem('label', a.label), (typeof a.order !== 'number' || !Number.isFinite(a.order)) && 'order is not a number'),
  }),
  // Debrief's theme packs (ADR 0018): full token sets per scheme and density plus traits, which the shell's `defineTheme`
  // (`--pd-*` overrides only) cannot hold; see `theme/themes.contributions.ts`.
  defineContributionKind<ThemeDef>({
    name: DEBRIEF_KIND.themes,
    label: 'theme',
    identity: (t) => t.id,
    check: (t) => issues(textProblem('id', t.id), textProblem('name', t.name), ...missingTokens(t).map((m) => `is missing ${m}`)),
  }),
  // The state a window group keeps in step (`popout/shared-state.ts`); the shell has no `defineSharedState` yet.
  defineContributionKind<SharedStateDef>({
    name: DEBRIEF_KIND.sharedState,
    label: 'shared state',
    identity: (g) => g.id,
    check: (g) =>
      issues(
        !CONTRIBUTION_ID.test(g.id) && `id ${JSON.stringify(g.id)} is not a namespaced id (db.shared.display)`,
        !['display', 'summary', 'full'].includes(g.scope) && 'scope must be display, summary or full',
        fnProblem('values', g.values),
      ),
  }),
] as readonly ContributionKind<never>[] as readonly ContributionKind[]);

/** Debrief's kinds in the shell's registry (once per window; repeating it is harmless). */
export function provideDebriefKinds(): Provider {
  return { provide: SHELL_CONTRIBUTION_KINDS, multi: true, useValue: DEBRIEF_KINDS };
}

/**
 * A message read live (titles of toggles change), with a contribution id as its message id. Debrief's texts are
 * `$localize`d where they are written, so the text is already in the window's language; the id is for the shell's lint
 * and later renderers (the shell's `msg` looks translations up by its own id, which Debrief's locales do not have).
 */
export function localMessage(id: string, read: () => string): Message {
  return Object.freeze({
    id,
    get text(): string {
      return read();
    },
  });
}

/**
 * The message id of a command's title (or `<id>.<suffix>`): the command id in the contribution id shape, camelCase parts
 * spelled with hyphens (`window.pinAll` gives `window.pin-all`) and a camelCase namespace run together (`knownIssues.check`
 * gives `knownissues.check`), since 66 of Debrief's command ids predate that shape. Pure.
 */
export function messageId(commandId: string, suffix?: string): string {
  const [ns = '', ...rest] = commandId.split('.');
  const kebab = [ns.toLowerCase(), ...rest.map((part) => part.replace(/([a-z0-9])([A-Z])/g, '$1-$2').toLowerCase())].join('.');
  return suffix ? `${kebab}.${suffix}` : kebab;
}

/** A Debrief command as the shell's `CommandDef` (frozen; title and category stay live). Pure. */
export function toShellCommand(c: DebriefCommand): DebriefCommandDef {
  const def: DebriefCommandDef & { category?: Message } = {
    id: c.id,
    title: localMessage(messageId(c.id), () => c.title),
    ...(c.category !== undefined ? { category: localMessage(messageId(c.id, 'category'), () => c.category ?? '') } : {}),
    ...(c.when ? { when: (ctx) => c.when?.(ctx) ?? true } : {}),
    ...(c.keybinding ? { keybinding: c.keybinding } : {}),
    ...(c.view ? { view: c.view } : {}),
    run: (ctx, args) => c.run(ctx, args),
  };
  return Object.freeze(def);
}

/** The keys Debrief's commands declare, as the shell's `keybindings` contributions. Pure. */
export function keybindingsOf(commands: readonly DebriefCommandDef[]): KeybindingDef[] {
  return commands.flatMap((c) => (c.keybinding ? [defineKeybinding({ command: c.id, key: c.keybinding })] : []));
}

/** The registry's command as Debrief's readers (palette, menus, overrides) see it: titles as live strings. Pure. */
export function asDebriefCommand(def: CommandDef<DebriefContext> & { keybinding?: string; view?: string }): DebriefCommand {
  return {
    id: def.id,
    get title() {
      return def.title.text;
    },
    get category() {
      return def.category?.text;
    },
    ...(def.keybinding ? { keybinding: def.keybinding } : {}),
    ...(def.view ? { view: def.view } : {}),
    ...(def.when ? { when: def.when } : {}),
    run: (ctx, args) => def.run(ctx, args),
  };
}

/** One registry entry whose items are made on first read, in the root injector (`items` runs once). */
function lazyEntry(kind: string, items: () => readonly unknown[]): ContributionEntry {
  let cache: readonly unknown[] | undefined;
  return Object.freeze({
    kind,
    get items(): readonly unknown[] {
      return (cache ??= Object.freeze([...items()]));
    },
  });
}

/** The shell kind name of a Debrief batch key (`commands` is the shell's own). */
function kindName(key: DebriefBatchKey): string {
  return key === 'commands' ? 'commands' : DEBRIEF_KIND[key];
}

/**
 * Registers Debrief contributions made by a factory that injects what it needs (a commands class's `commands`, a view
 * list built from a store). The factory runs once, in the root injector, when a reader first asks for that kind: a
 * registration no longer runs when another kind is read (so a command class that injects the view registry does not
 * form a cycle with it). These registrations are eager in the bundle (the factory's classes are imported statically);
 * a feature made lazy registers static data with `provideDebriefContributions` instead.
 */
export function contributeFrom<K extends DebriefBatchKey>(key: K, factory: () => readonly DebriefBatchItems[K][]): Provider[] {
  if (key !== 'commands') {
    return [
      provideDebriefKinds(),
      {
        provide: SHELL_CONTRIBUTIONS,
        multi: true,
        useFactory: () => {
          const injector = inject(Injector);
          return lazyEntry(kindName(key), () => runInInjectionContext(injector, factory));
        },
      },
    ];
  }
  const batch = new InjectionToken<() => readonly DebriefCommandDef[]>('debrief commands batch');
  return [
    provideDebriefKinds(),
    {
      provide: batch,
      useFactory: () => {
        const injector = inject(Injector);
        let cache: readonly DebriefCommandDef[] | undefined;
        return () => (cache ??= runInInjectionContext(injector, () => (factory() as readonly DebriefCommand[]).map(toShellCommand)));
      },
    },
    { provide: SHELL_CONTRIBUTIONS, multi: true, useFactory: () => lazyEntry('commands', inject(batch)) },
    {
      provide: SHELL_CONTRIBUTIONS,
      multi: true,
      useFactory: () => {
        const commands = inject(batch);
        return lazyEntry('keybindings', () => keybindingsOf(commands()));
      },
    },
  ];
}

/** A batch of Debrief contributions as static data (every part optional). */
export type DebriefContributions = { readonly [K in DebriefBatchKey]?: readonly DebriefBatchItems[K][] };

/** The providers of a static batch: one registry entry per kind, plus the commands' keys. */
export function debriefContributionProviders(batch: DebriefContributions): Provider[] {
  const out: Provider[] = [];
  for (const [key, items] of Object.entries(batch) as [DebriefBatchKey, readonly unknown[] | undefined][]) {
    if (!items?.length) continue;
    if (key === 'commands') {
      const defs = (items as readonly DebriefCommand[]).map(toShellCommand);
      out.push({ provide: SHELL_CONTRIBUTIONS, multi: true, useValue: Object.freeze({ kind: 'commands', items: Object.freeze(defs) }) });
      const keys = keybindingsOf(defs);
      if (keys.length) out.push({ provide: SHELL_CONTRIBUTIONS, multi: true, useValue: Object.freeze({ kind: 'keybindings', items: Object.freeze(keys) }) });
    } else out.push({ provide: SHELL_CONTRIBUTIONS, multi: true, useValue: Object.freeze({ kind: kindName(key), items: Object.freeze([...items]) }) });
  }
  return out;
}

/** Registers a static batch of Debrief contributions (and Debrief's kinds). */
export function provideDebriefContributions(batch: DebriefContributions): EnvironmentProviders {
  return makeEnvironmentProviders([provideDebriefKinds(), ...debriefContributionProviders(batch)]);
}

/**
 * Every item of one kind, flattened over its registry entries, in registration order (duplicates kept: readers keep the
 * first). Debrief's readers use this instead of the shell's `ContributionRegistry`, which reads every kind at once.
 */
export function contributedItems<T>(kind: string): T[] {
  const entries = inject(SHELL_CONTRIBUTIONS, { optional: true }) ?? [];
  return entries.filter((e) => e.kind === kind).flatMap((e) => e.items as readonly T[]);
}
