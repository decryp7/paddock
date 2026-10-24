import { computed, DestroyRef, effect, inject, Injectable, InjectionToken, signal, untracked } from '@angular/core';
import { ActivatedRouteSnapshot, NavigationEnd, Router } from '@angular/router';
import { ContributionRegistry, evaluate, ShellContextHost, type PageDef } from '@decryp7/paddock-ui/shell';

/** The route data key that names a route's page. */
export const PAGE_DATA = 'pdPage';

/** A route path: lower-case parts of letters, digits and hyphens, joined by `/`. */
const PATH = /^[a-z0-9]+(?:-[a-z0-9]+)*(?:\/[a-z0-9]+(?:-[a-z0-9]+)*)*$/;

/** What `provideShellPages` tells the pages: the app's name (window titles) and path overrides by page id. */
export interface PagesConfig {
  readonly appName: string;
  readonly paths: Readonly<Record<string, string>>;
}

export const PAGES_CONFIG = new InjectionToken<PagesConfig>('PADDOCK_PAGES_CONFIG', { providedIn: 'root', factory: () => ({ appName: 'App', paths: {} }) });

/**
 * The route path of a page: its override when that is a valid path, else its id without the namespace, dots as
 * slashes (`pw.models` is `models`, `pw.models.library` is `models/library`).
 */
export function pagePath(id: string, paths: Readonly<Record<string, string>> = {}): string {
  const override = paths[id];
  if (override !== undefined && PATH.test(override)) return override;
  const parts = id.split('.');
  return (parts.length > 1 ? parts.slice(1) : parts).join('/');
}

/** Pages in sidebar order: top before bottom, then by `order`, then by id. */
export function pageOrder(a: PageDef, b: PageDef): number {
  const section = (p: PageDef) => (p.section === 'bottom' ? 1 : 0);
  return section(a) - section(b) || a.order - b.order || (a.id < b.id ? -1 : a.id > b.id ? 1 : 0);
}

/** A page badge as text: a number above 0 or a non-blank string; anything else draws none. */
export function badgeText(value: unknown): string | null {
  if (typeof value === 'number') return Number.isFinite(value) && value > 0 ? String(value) : null;
  if (typeof value === 'string') return value.trim() ? value.trim() : null;
  return null;
}

/** The page of the deepest active route, or null. */
export function pageOfSnapshot(root: ActivatedRouteSnapshot): string | null {
  let at: ActivatedRouteSnapshot | null = root;
  let page: string | null = null;
  while (at) {
    const id = at.data[PAGE_DATA];
    if (typeof id === 'string') page = id;
    at = at.firstChild;
  }
  return page;
}

/**
 * The pages of the window (`definePage` contributions) as the frame shows them: in order, those whose `when` holds,
 * their badges, the current page, and navigation by id (`ShellCore.showPage`). A page whose `when` turns false while it
 * is shown gives way to the first page.
 */
@Injectable({ providedIn: 'root' })
export class ShellPages {
  private readonly registry = inject(ContributionRegistry);
  private readonly host = inject(ShellContextHost);
  private readonly router = inject(Router);
  private readonly config = inject(PAGES_CONFIG);
  private readonly shownPage = signal<string | null>(null);

  /** Every page, in sidebar order. */
  readonly all: readonly PageDef[] = [...this.registry.pages()].sort(pageOrder);
  /** The pages offered now (`when` holds; until the app's context is loaded every page is offered). */
  readonly visible = computed(() => {
    const ctx = this.host.context();
    return this.all.filter((p) => !p.when || evaluate(p.when, ctx, ctx === null));
  });
  readonly top = computed(() => this.visible().filter((p) => p.section !== 'bottom'));
  readonly bottom = computed(() => this.visible().filter((p) => p.section === 'bottom'));
  /** Each page's badge text (absent: none). */
  readonly badges = computed(() => {
    const ctx = this.host.context();
    const out: Record<string, string> = {};
    for (const p of this.visible()) {
      const text = badgeText(evaluate(p.badge, ctx, null));
      if (text !== null) out[p.id] = text;
    }
    return out as Readonly<Record<string, string>>;
  });
  /** The page shown now (null before the first navigation, or on a route that is not a page). */
  readonly current = this.shownPage.asReadonly();

  constructor() {
    const sub = this.router.events.subscribe((e) => {
      if (e instanceof NavigationEnd) this.shownPage.set(pageOfSnapshot(this.router.routerState.snapshot.root));
    });
    inject(DestroyRef).onDestroy(() => sub.unsubscribe());
    // Navigation is a side effect outside the signal graph: a page that is no longer offered gives way to the first one.
    effect(() => {
      const current = this.current();
      if (!current || this.host.context() === null || this.visible().some((p) => p.id === current)) return;
      untracked(() => void this.router.navigateByUrl(this.defaultUrl()));
    });
  }

  path(id: string): string {
    return pagePath(id, this.config.paths);
  }

  /** The URL of the first page offered (else the first without a condition, else the first page). */
  defaultUrl(): string {
    const page = this.top()[0] ?? this.visible()[0] ?? this.all.find((p) => !p.when) ?? this.all[0];
    return page ? `/${this.path(page.id)}` : '/';
  }

  /** Shows a page by id; false (with a warning) when there is no such page. */
  show(id: string): boolean {
    if (!this.all.some((p) => p.id === id)) {
      console.warn(`[paddock-pages] there is no page ${id}`);
      return false;
    }
    void this.router.navigateByUrl(`/${this.path(id)}`);
    return true;
  }
}
