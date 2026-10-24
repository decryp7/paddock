import { computed, inject, Injectable, InjectionToken, type Signal } from '@angular/core';
import { ContributionRegistry, evaluate, ShellContextHost, type ShellContext } from '@decryp7/paddock-ui/shell';
import type {
  ActivityItemDef,
  AssistantContextDef,
  ExplorerProviderDef,
  PaletteProviderDef,
  PerspectiveDef,
  SharedStateDef,
  ViewDef,
} from './define';

/** Separates a view id from an instance key in a panel id: `pn.code#src/main.rs`. */
export const INSTANCE_SEPARATOR = '#';
/** Longest instance key (a path); longer keys are refused by `panelId`. */
export const MAX_INSTANCE_KEY = 1024;

/** A panel's identity: which view it renders, and which instance (null for the view's one panel). */
export interface PanelRef {
  /** The dock panel id: the view id, or `<view>#<key>`. */
  readonly panel: string;
  readonly view: string;
  readonly key: string | null;
}

/** The panel id of a view's instance (`pn.code#src/main.rs`), or its one panel when `key` is null. */
export function panelId(view: string, key: string | null = null): string {
  if (key === null) return view;
  if (!key || key.length > MAX_INSTANCE_KEY) throw new Error(`instance key of ${view} must be 1 to ${MAX_INSTANCE_KEY} characters`);
  return `${view}${INSTANCE_SEPARATOR}${key}`;
}

/** Splits a panel id; the key is everything after the first `#` (keys may contain `#`). */
export function parsePanelId(panel: string): PanelRef {
  const at = panel.indexOf(INSTANCE_SEPARATOR);
  if (at <= 0 || at === panel.length - 1) return { panel, view: panel, key: null };
  return { panel, view: panel.slice(0, at), key: panel.slice(at + 1) };
}

/**
 * The panel a view component is rendered in (its id, view and instance key): injected by the dock and by a pop-out
 * window, so a view with `instances` knows which one it is.
 */
export const VIEW_PANEL = new InjectionToken<PanelRef>('PADDOCK_VIEW_PANEL');

const byOrder = <D extends { readonly order?: number; readonly id: string }>(a: D, b: D): number => (a.order ?? 1000) - (b.order ?? 1000) || a.id.localeCompare(b.id);

/**
 * The workbench's contributions, typed and ordered, with the live parts (`when`) evaluated over the window's context.
 * A root singleton over the shell's `ContributionRegistry` (ADR 0012 §4 allows the registries there).
 */
@Injectable({ providedIn: 'root' })
export class WorkbenchRegistry {
  private readonly registry = inject(ContributionRegistry);
  private readonly host = inject(ShellContextHost);

  readonly views: readonly ViewDef[] = [...this.registry.items<ViewDef>('views')].sort(byOrder);
  readonly activityItems: readonly ActivityItemDef[] = [...this.registry.items<ActivityItemDef>('activityItems')].sort(byOrder);
  readonly explorerProviders: readonly ExplorerProviderDef[] = [...this.registry.items<ExplorerProviderDef>('explorerProviders')].sort(byOrder);
  readonly paletteProviders: readonly PaletteProviderDef[] = [...this.registry.items<PaletteProviderDef>('paletteProviders')].sort(byOrder);
  readonly perspectives: readonly PerspectiveDef[] = [...this.registry.items<PerspectiveDef>('perspectives')].sort(byOrder);
  readonly sharedState: readonly SharedStateDef[] = this.registry.items<SharedStateDef>('sharedState');
  readonly assistantContext: readonly AssistantContextDef[] = [...this.registry.items<AssistantContextDef>('assistantContext')].sort(byOrder);

  /** The window's context (null until the app's part is loaded). */
  readonly context: Signal<ShellContext | null> = this.host.context;

  /**
   * Views offered now: `when` holds. Before the app's context is loaded only views without `when` are offered (a view
   * that depends on the workspace is not shown before the workspace is known); a throwing `when` hides that view.
   */
  readonly availableViews = computed(() => {
    const ctx = this.host.context();
    return this.views.filter((v) => isOffered(v.when, ctx));
  });

  readonly availablePerspectives = computed(() => {
    const ctx = this.host.context();
    return this.perspectives.filter((p) => isOffered(p.when, ctx));
  });

  readonly availableExplorerProviders = computed(() => {
    const ctx = this.host.context();
    return this.explorerProviders.filter((p) => isOffered(p.when, ctx));
  });

  readonly availablePaletteProviders = computed(() => {
    const ctx = this.host.context();
    return this.paletteProviders.filter((p) => isOffered(p.when, ctx));
  });

  /** The view by id; a panel id `<view>#<key>` finds its view when the view allows instances. */
  findView(id: string): ViewDef | undefined {
    const exact = this.views.find((v) => v.id === id);
    if (exact) return exact;
    const ref = parsePanelId(id);
    if (ref.key === null) return undefined;
    return this.views.find((v) => v.id === ref.view && v.instances === true);
  }

  /** Whether a view (or a panel of it) is offered now. */
  isViewOffered(id: string): boolean {
    const view = this.findView(id);
    return !!view && isOffered(view.when, this.host.context());
  }

  findPerspective(id: string): PerspectiveDef | undefined {
    return this.perspectives.find((p) => p.id === id);
  }

  findExplorerProvider(id: string): ExplorerProviderDef | undefined {
    return this.explorerProviders.find((p) => p.id === id);
  }

  /** The provider that answers a query: the one whose prefix the query starts with, else the plain-text one. */
  paletteProviderFor(query: string): { provider: PaletteProviderDef; query: string } | null {
    const providers = this.availablePaletteProviders();
    const first = query.charAt(0);
    const prefixed = first ? providers.find((p) => p.prefix === first) : undefined;
    if (prefixed) return { provider: prefixed, query: query.slice(1).trimStart() };
    const plain = providers.find((p) => p.prefix === '');
    return plain ? { provider: plain, query } : null;
  }
}

/** A `when` that hides the item until the context is loaded, and when it throws. */
export function isOffered<T extends object>(when: ((ctx: ShellContext<T>) => boolean) | undefined, ctx: ShellContext<T> | null): boolean {
  if (!when) return true;
  return evaluate(when, ctx, false);
}
