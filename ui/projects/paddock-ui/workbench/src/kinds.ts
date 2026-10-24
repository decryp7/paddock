import { CONTRIBUTION_ID, defineContributionKind, isMessage, type ContributionKind, type ContributionLookup } from '@decryp7/paddock-ui/shell';
import type { ViewLayout } from '@decryp7/paddock-ui';
import {
  VIEW_AREAS,
  type ActivityItemDef,
  type ActivityTarget,
  type AssistantContextDef,
  type ExplorerProviderDef,
  type PaletteProviderDef,
  type PerspectiveDef,
  type SharedStateDef,
  type ViewDef,
} from './define';

/*
 * The workbench's contribution kinds (plan W1): added to the shell's one registry with `provideContributionKinds`, so
 * `assertContributionsValid` checks them with the shell's (rule 40: one lint over every instance).
 */

const issues = (...checks: (string | false | null | undefined)[]): string[] => checks.filter((c): c is string => typeof c === 'string');
const idProblem = (id: unknown): string | false =>
  (typeof id !== 'string' || !CONTRIBUTION_ID.test(id)) && `id ${JSON.stringify(id)} is not a namespaced id (pn.code)`;
const messageProblem = (field: string, value: unknown): string | false => !isMessage(value) && `${field} has no message id (use msg('<ns>.<name>', 'Text'))`;
const ICON = /^[a-z][a-z0-9-]*:[a-z0-9][a-z0-9-]*$/;
const iconProblem = (icon: unknown, field = 'icon'): string | false => (typeof icon !== 'string' || !ICON.test(icon)) && `has no ${field} (namespace:name)`;
const optionalIcon = (icon: unknown): string | false => icon !== undefined && iconProblem(icon);
const fnProblem = (value: unknown, field: string): string | false => typeof value !== 'function' && `has no ${field} function`;
const optionalFn = (value: unknown, field: string): string | false => value !== undefined && typeof value !== 'function' && `${field} is not a function`;
const numberProblem = (field: string, value: unknown, min: number, max: number): string | false =>
  (typeof value !== 'number' || !Number.isFinite(value) || value < min || value > max) && `${field} must be a number from ${min} to ${max}`;
const optionalNumber = (field: string, value: unknown, min: number, max: number): string | false => value !== undefined && numberProblem(field, value, min, max);
const MAX_PX = 8000;

/** Defects of a view's declared layout (ADR 0011): an area, sizes in range, `min <= pref`, a priority from 1 to 100. */
export function layoutProblems(layout: unknown): string[] {
  if (typeof layout !== 'object' || layout === null) return ['has no layout (area, min, pref, priority; ADR 0011)'];
  const l = layout as Partial<ViewLayout>;
  const size = (name: 'min' | 'pref'): string[] => {
    const s = l[name];
    if (typeof s !== 'object' || s === null) return [`layout.${name} is missing`];
    return issues(numberProblem(`layout.${name}.width`, s.width, 1, MAX_PX), numberProblem(`layout.${name}.height`, s.height, 1, MAX_PX));
  };
  const sizes = [...size('min'), ...size('pref')];
  return issues(
    !VIEW_AREAS.includes(l.area as never) && `layout.area ${JSON.stringify(l.area)} is not one of ${VIEW_AREAS.join(', ')}`,
    ...sizes,
    !sizes.length && (l.min as { width: number }).width > (l.pref as { width: number }).width && `layout min > pref: min width ${l.min?.width} exceeds pref ${l.pref?.width}`,
    !sizes.length && (l.min as { height: number }).height > (l.pref as { height: number }).height && `layout min > pref: min height ${l.min?.height} exceeds pref ${l.pref?.height}`,
    numberProblem('layout.priority', l.priority, 1, 100),
    l.edge !== undefined && (l.edge !== 'left' && l.edge !== 'right' ? `layout.edge must be left or right` : l.area !== 'side' && `layout.edge is for side views only`),
  );
}

/** Defects of an activity target: it must name a registered view, command or Explorer provider. */
export function targetProblems(run: unknown, lookup: ContributionLookup): string[] {
  if (typeof run !== 'object' || run === null) return ['has no run target ({ view }, { command }, { explorer } or { menu })'];
  const t = run as Partial<Record<'view' | 'command' | 'explorer' | 'menu', unknown>>;
  if ('menu' in t) return issues(fnProblem(t.menu, 'menu'));
  if (typeof t.view === 'string') return issues(!lookup.has('views', t.view) && `shows the missing view ${t.view}`);
  if (typeof t.command === 'string') return issues(!lookup.has('commands', t.command) && `runs the missing command ${t.command}`);
  if (typeof t.explorer === 'string') return issues(!lookup.has('explorerProviders', t.explorer) && `shows the missing Explorer section ${t.explorer}`);
  return ['has no run target ({ view }, { command }, { explorer } or { menu })'];
}

export const VIEWS = defineContributionKind<ViewDef>({
  name: 'views',
  label: 'view',
  identity: (v) => v.id,
  check: (v) =>
    issues(
      idProblem(v.id),
      messageProblem('title', v.title),
      iconProblem(v.icon),
      ...layoutProblems(v.layout),
      v.category !== undefined && messageProblem('category', v.category),
      optionalNumber('order', v.order, 0, 100_000),
      fnProblem(v.load, 'load'),
      optionalFn(v.providers, 'providers'),
      optionalFn(v.when, 'when'),
      v.autoOpen === true && v.when === undefined && `autoOpen needs a when (it opens when when starts to hold)`,
      v.fullHeight === true && v.layout?.area !== 'document' && `fullHeight is for document views only`,
    ),
});

export const ACTIVITY_ITEMS = defineContributionKind<ActivityItemDef>({
  name: 'activityItems',
  label: 'activity item',
  identity: (a) => a.id,
  check: (a, lookup) =>
    issues(
      idProblem(a.id),
      messageProblem('title', a.title),
      iconProblem(a.icon),
      numberProblem('order', a.order, 0, 100_000),
      a.section !== 'top' && a.section !== 'bottom' && `section must be top or bottom`,
      ...targetProblems(a.run, lookup),
      optionalFn(a.when, 'when'),
      optionalFn(a.badge, 'badge'),
      optionalFn(a.active, 'active'),
      optionalFn(a.pulse, 'pulse'),
      optionalFn(a.disabled, 'disabled'),
    ),
});

export const EXPLORER_PROVIDERS = defineContributionKind<ExplorerProviderDef>({
  name: 'explorerProviders',
  label: 'Explorer provider',
  identity: (e) => e.id,
  check: (e, lookup) =>
    issues(
      idProblem(e.id),
      messageProblem('title', e.title),
      optionalIcon(e.icon),
      numberProblem('order', e.order, 0, 100_000),
      fnProblem(e.roots, 'roots'),
      optionalFn(e.children, 'children'),
      optionalFn(e.open, 'open'),
      optionalFn(e.actions, 'actions'),
      optionalFn(e.run, 'run'),
      e.actions !== undefined && e.run === undefined && `has actions but no run function`,
      optionalFn(e.drag, 'drag'),
      optionalFn(e.changes, 'changes'),
      e.empty !== undefined && messageProblem('empty.text', e.empty?.text),
      e.empty?.command !== undefined && !lookup.has('commands', e.empty.command) && `empty state runs the missing command ${e.empty.command}`,
    ),
});

/** The prefix of the palette's own commands list (plus "Open <view>"); never a provider's. */
export const COMMANDS_PREFIX = '>';

/** A palette prefix: empty (plain text) or one symbol that is not a letter, digit or space. */
export const PALETTE_PREFIX = /^(?:[^\p{L}\p{N}\s])?$/u;

export const PALETTE_PROVIDERS = defineContributionKind<PaletteProviderDef>({
  name: 'paletteProviders',
  label: 'palette provider',
  identity: (p) => p.id,
  check: (p) =>
    issues(
      idProblem(p.id),
      messageProblem('title', p.title),
      (typeof p.prefix !== 'string' || !PALETTE_PREFIX.test(p.prefix)) && `prefix ${JSON.stringify(p.prefix)} must be empty or one symbol (@, #, :)`,
      p.prefix === COMMANDS_PREFIX && `prefix ${COMMANDS_PREFIX} is the palette's commands (choose another symbol)`,
      p.placeholder !== undefined && messageProblem('placeholder', p.placeholder),
      numberProblem('order', p.order, 0, 100_000),
      fnProblem(p.search, 'search'),
      fnProblem(p.accept, 'accept'),
      optionalNumber('debounceMs', p.debounceMs, 0, 2000),
    ),
  checkAll: (items) => {
    const seen = new Map<string, string>();
    const out: string[] = [];
    for (const p of items) {
      if (typeof p.prefix !== 'string') continue;
      const first = seen.get(p.prefix);
      if (first === undefined) seen.set(p.prefix, p.id);
      else out.push(`palette prefix conflict: ${first} and ${p.id} both answer ${p.prefix ? JSON.stringify(p.prefix) : 'plain text'}`);
    }
    return out;
  },
});

export const PERSPECTIVES = defineContributionKind<PerspectiveDef>({
  name: 'perspectives',
  label: 'perspective',
  identity: (p) => p.id,
  check: (p, lookup) =>
    issues(
      idProblem(p.id),
      messageProblem('title', p.title),
      iconProblem(p.icon),
      p.description !== undefined && messageProblem('description', p.description),
      numberProblem('order', p.order, 0, 100_000),
      (!Array.isArray(p.views) || !p.views.length) && `opens no views`,
      ...(Array.isArray(p.views) ? p.views.map((v) => !lookup.has('views', v) && `opens the missing view ${v}`) : []),
      Array.isArray(p.views) && new Set(p.views).size !== p.views.length && `lists a view twice`,
      p.focus !== undefined && Array.isArray(p.views) && !p.views.includes(p.focus) && `focus ${p.focus} is not one of its views`,
      optionalFn(p.when, 'when'),
    ),
});

export const SHARED_STATE = defineContributionKind<SharedStateDef>({
  name: 'sharedState',
  label: 'shared state',
  identity: (s) => s.id,
  check: (s) =>
    issues(
      idProblem(s.id),
      s.scope !== 'full' && s.scope !== 'display' && `scope must be full or display`,
      fnProblem(s.read, 'read'),
      fnProblem(s.parse, 'parse'),
      fnProblem(s.apply, 'apply'),
      optionalNumber('throttleMs', s.throttleMs, 0, 10_000),
    ),
});

export const ASSISTANT_CONTEXT = defineContributionKind<AssistantContextDef>({
  name: 'assistantContext',
  label: 'assistant context',
  identity: (a) => a.id,
  check: (a) => issues(idProblem(a.id), messageProblem('label', a.label), optionalIcon(a.icon), fnProblem(a.value, 'value'), numberProblem('order', a.order, 0, 100_000)),
});

/** The workbench's kinds, in registry order (after the shell's). */
export const WORKBENCH_KINDS: readonly ContributionKind[] = [
  VIEWS,
  ACTIVITY_ITEMS,
  EXPLORER_PROVIDERS,
  PALETTE_PROVIDERS,
  PERSPECTIVES,
  SHARED_STATE,
  ASSISTANT_CONTEXT,
] as readonly ContributionKind<never>[] as readonly ContributionKind[];

/** The target of an activity item, when it is not a fly-out menu. */
export function activityTarget(item: Pick<ActivityItemDef, 'run'>): ActivityTarget | null {
  return 'menu' in item.run ? null : item.run;
}
