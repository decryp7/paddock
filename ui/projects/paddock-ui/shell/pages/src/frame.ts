import { DOCUMENT } from '@angular/common';
import { afterNextRender, ChangeDetectionStrategy, Component, computed, DestroyRef, ElementRef, inject, Injector } from '@angular/core';
import { NavigationEnd, Router, RouterOutlet } from '@angular/router';
import { PdCopyMenuHost } from '@decryp7/paddock-ui';
import { cycleRegions, PdIcon, PdStatusBar, PdTitleBar, regionKey, regionTarget } from '@decryp7/paddock-ui/shell';
import { DialogHost } from '@decryp7/paddock-ui/shell/dialogs';
import { MENU_MODE, PdMenuBar } from '@decryp7/paddock-ui/shell/menu';
import { PdNotificationHost } from '@decryp7/paddock-ui/shell/notify';
import { PdRecoveryNotice, PdSafeModeBadge, RecoveryStore } from '@decryp7/paddock-ui/shell/startup';
import { PagesFrameStore } from './pages-frame.store';
import { PAGES_CONFIG } from './pages';
import { PdPagesSidebar } from './sidebar';

/**
 * The window of a manager app (Pitwall, Pitcrew, Spotter, Dyno; ADR 0012, plan S14): the title bar with the menu bar
 * (S13), the safe-mode badge and the unclean-shutdown notice (X3), the sidebar of contributed pages beside the routed page, the status bar with the contributed status items (the
 * service chip and the notifications of S4, the jobs of S9), the notification host and the dialog host. The sidebar is
 * expanded, a rail or over the page by the layout plan (`sidebarFit`); F6 cycles the regions. Use it as the root
 * component's whole template, with `provideShellPages(...)` in the window's providers.
 *
 * Slots: `[slot=title-end]` (the end of the title bar) and `[slot=sidebar]` (above the bottom pages; hidden in the rail).
 */
@Component({
  selector: 'pd-shell-frame',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [
    RouterOutlet, PdTitleBar, PdMenuBar, PdIcon, PdPagesSidebar, PdStatusBar, PdNotificationHost, DialogHost, PdRecoveryNotice, PdSafeModeBadge, PdCopyMenuHost,
  ],
  providers: [PagesFrameStore, RecoveryStore],
  host: {
    '[attr.data-sidebar]': 'store.mode()',
    '[class.pd-drawer-open]': 'store.drawer()',
    '[class.pd-native-menu]': 'menuMode === "native"',
    '(document:keydown)': 'onKey($event)',
  },
  template: `
    <a class="pd-skip" href="#pd-main" (click)="skip($event)">{{ t.skip }}</a>
    <header class="pd-title" data-pd-region="title" tabindex="-1">
      <pd-title-bar [appName]="config.appName">
        <button type="button" slot="lead" class="pd-toggle" data-testid="sidebar-toggle" [attr.aria-label]="toggleLabel()" [attr.title]="toggleLabel()"
          [attr.aria-expanded]="store.labelled()" aria-controls="pd-pages-nav" (click)="toggle()">
          <pd-icon name="pd:sidebar" />
        </button>
        <pd-menu-bar />
        <ng-container ngProjectAs="[slot=end]"><pd-safe-mode-badge /><ng-content select="[slot=title-end]" /></ng-container>
      </pd-title-bar>
      <pd-recovery-notice />
    </header>
    <div class="pd-body">
      <nav id="pd-pages-nav" class="pd-sidebar" [attr.aria-label]="t.pages" data-pd-region="sidebar" tabindex="-1" [class.pd-drawer]="store.drawer()" [hidden]="store.mode() === 'overlay' && !store.drawer()">
        <pd-pages-sidebar [labelled]="store.labelled()" (navigated)="store.closeDrawer()">
          <ng-content select="[slot=sidebar]" />
        </pd-pages-sidebar>
      </nav>
      @if (store.drawer()) {
        <div class="pd-scrim" data-testid="sidebar-scrim" aria-hidden="true" (click)="closeDrawer(false)"></div>
      }
      <main id="pd-main" class="pd-page" tabindex="-1" data-pd-region="main" [attr.inert]="store.drawer() ? '' : null">
        <router-outlet />
      </main>
    </div>
    <footer class="pd-status" [attr.aria-label]="t.statusBar" data-pd-region="status" tabindex="-1" [attr.inert]="store.drawer() ? '' : null">
      <pd-status-bar />
    </footer>
    <pd-notification-host />
    <pd-dialog-host />
    <pd-copy-menu />
  `,
  styles: `
    :host { --pd-titlebar-height: 40px; container: pd-frame / inline-size; display: grid; grid-template-rows: auto minmax(0, 1fr) auto; height: 100vh; height: 100dvh; min-width: 0; overflow: hidden; background: var(--pd-bg); color: var(--pd-text); }
    .pd-title { min-width: 0; background: var(--pd-surface-1); border-bottom: 1px solid var(--pd-hairline); outline: none; }
    .pd-toggle { display: inline-flex; align-items: center; justify-content: center; width: 28px; height: 28px; padding: 0; border: 0; border-radius: var(--pd-radius-control); background: transparent; color: var(--pd-text-muted); cursor: pointer; }
    .pd-toggle:hover { background: var(--pd-hover); color: var(--pd-text); }
    .pd-toggle:focus-visible { outline: 2px solid var(--pd-focus-ring); outline-offset: 1px; }
    .pd-body { position: relative; display: grid; grid-template-columns: var(--pd-sidebar-width) minmax(0, 1fr); min-height: 0; min-width: 0; }
    :host([data-sidebar='rail']) .pd-body { grid-template-columns: var(--pd-sidebar-collapsed) minmax(0, 1fr); }
    :host([data-sidebar='overlay']) .pd-body { grid-template-columns: minmax(0, 1fr); }
    .pd-sidebar { min-height: 0; min-width: 0; background: var(--pd-surface-1); border-inline-end: 1px solid var(--pd-hairline); outline: none; }
    .pd-sidebar[hidden] { display: none; }
        /*
     * Over the page the sidebar is modal: it and its scrim cover everything below the title bar (the page and the status
     * bar, which are inert meanwhile); Escape, the scrim or a page closes it. The title bar stays live for its button and
     * keeps its hairline: the drawer starts below it and casts its shadow to the right only.
     */
    .pd-sidebar.pd-drawer { position: fixed; top: calc(var(--pd-titlebar-height) + 1px); bottom: 0; inset-inline-start: 0; z-index: var(--pd-z-overlay); width: min(var(--pd-sidebar-width), calc(100vw - var(--pd-space-8))); clip-path: inset(0 -32px 0 0); box-shadow: 16px 0 32px -16px var(--pd-scrim); border-inline-end-color: var(--pd-border-strong); animation: pd-drawer-in var(--pd-duration) var(--pd-ease-enter); }
    .pd-scrim { position: fixed; top: calc(var(--pd-titlebar-height) + 1px); inset-inline: 0; bottom: 0; z-index: calc(var(--pd-z-overlay) - 1); background: var(--pd-scrim); animation: pd-fade-in var(--pd-duration) var(--pd-ease-enter); }
    :host([data-sidebar='overlay']) .pd-page { grid-column: 1; }
    /* Pages lay out by container queries on the page area (container pd-page), not the window. */
    :host(.pd-native-menu) { --pd-titlebar-inset-start: 78px; }
    .pd-page { container: pd-page / inline-size; grid-row: 1; grid-column: 2; min-width: 0; min-height: 0; overflow-x: hidden; overflow-y: auto; outline: none; scrollbar-gutter: stable; background: var(--pd-bg); }
    .pd-status { min-width: 0; overflow-x: clip; background: var(--pd-surface-1); border-top: 1px solid var(--pd-hairline); outline: none; }
    .pd-title:focus-visible, .pd-sidebar:focus-visible, .pd-page:focus-visible, .pd-status:focus-visible { box-shadow: inset 0 0 0 2px var(--pd-focus-ring); }
    .pd-skip { position: fixed; inset-inline-start: var(--pd-space-3); top: -48px; z-index: var(--pd-z-toast); padding: var(--pd-space-2) var(--pd-space-3); border-radius: var(--pd-radius-control); background: var(--pd-accent-fill); color: var(--pd-on-accent); font-weight: var(--pd-weight-medium); }
    .pd-skip:focus { top: var(--pd-space-2); text-decoration: none; }
    @keyframes pd-drawer-in { from { transform: translateX(-12px); opacity: 0; } to { transform: none; opacity: 1; } }
    @keyframes pd-fade-in { from { opacity: 0; } to { opacity: 1; } }
    @media (prefers-reduced-motion: reduce) { .pd-sidebar.pd-drawer, .pd-scrim { animation: none; } }
  `,
})
export class PdShellFrame {
  /** The component's texts in the window's language (plan S15: `$localize`, extracted from the package build). */
  protected readonly t = {
    skip: $localize`:@@pd.pages.skip:Skip to page`,
    pages: $localize`:@@pd.pages.nav:Pages`,
    statusBar: $localize`:@@pd.pages.status-bar:Status bar`,
  };
  protected readonly store = inject(PagesFrameStore);
  protected readonly config = inject(PAGES_CONFIG);
  /** The macOS app has its menus in the native menu bar: the title bar keeps clear of the window buttons (78 px). */
  protected readonly menuMode = inject(MENU_MODE);
  private readonly el = inject<ElementRef<HTMLElement>>(ElementRef).nativeElement;
  private readonly doc = inject(DOCUMENT);
  private readonly injector = inject(Injector);

  protected readonly toggleLabel = computed(() => {
    const mode = this.store.mode();
    if (mode === 'expanded') return $localize`:@@pd.pages.sidebar.collapse:Collapse sidebar`;
    if (this.store.fit() === 'expanded') return $localize`:@@pd.pages.sidebar.expand:Expand sidebar`;
    return this.store.drawer() ? $localize`:@@pd.pages.sidebar.hide:Hide pages` : $localize`:@@pd.pages.sidebar.show:Show pages`;
  });

  constructor() {
    const observer = typeof ResizeObserver === 'function' ? new ResizeObserver(() => this.store.measure(this.el.clientWidth)) : null;
    afterNextRender(() => {
      this.store.measure(this.el.clientWidth);
      observer?.observe(this.el);
    });
    const router = inject(Router);
    let first = true;
    const sub = router.events.subscribe((e) => {
      if (!(e instanceof NavigationEnd)) return;
      if (first) {
        first = false;
        return;
      }
      this.store.closeDrawer();
      this.focusPageAfterNavigation();
    });
    inject(DestroyRef).onDestroy(() => {
      observer?.disconnect();
      sub.unsubscribe();
    });
  }

  /** The title bar's button; a sidebar opened over the page takes focus (its current page). */
  protected toggle(): void {
    this.store.toggle();
    if (!this.store.drawer()) return;
    afterNextRender(
      () => {
        const nav = this.el.querySelector<HTMLElement>('#pd-pages-nav');
        if (nav) regionTarget(nav).focus();
      },
      { injector: this.injector },
    );
  }

  protected onKey(e: KeyboardEvent): void {
    if (e.defaultPrevented) return;
    if (e.key === 'Escape' && this.store.drawer()) {
      e.preventDefault();
      this.closeDrawer(true);
      return;
    }
    const direction = regionKey(e);
    if (direction === null) return;
    e.preventDefault();
    cycleRegions(this.el, direction, this.doc.activeElement);
  }

  protected closeDrawer(refocus: boolean): void {
    this.store.closeDrawer();
    if (refocus) this.el.querySelector<HTMLElement>('[data-testid="sidebar-toggle"]')?.focus();
  }

  protected skip(e: Event): void {
    e.preventDefault();
    this.el.querySelector<HTMLElement>('#pd-main')?.focus();
  }

  /** After a navigation from the sidebar (or with nothing focused), the page gets focus so a screen reader starts there. */
  private focusPageAfterNavigation(): void {
    const active = this.doc.activeElement;
    const fromSidebar = !active || active === this.doc.body || !!this.el.querySelector('#pd-pages-nav')?.contains(active);
    if (fromSidebar) this.el.querySelector<HTMLElement>('#pd-main')?.focus({ preventScroll: true });
  }
}
