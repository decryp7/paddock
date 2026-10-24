import { NgComponentOutlet } from '@angular/common';
import {
  ChangeDetectionStrategy,
  Component,
  computed,
  DestroyRef,
  ElementRef,
  EnvironmentInjector,
  inject,
  InjectionToken,
  Injector,
  input,
  resource,
  signal,
  type Type,
} from '@angular/core';
import { loadPart, type SettingsPageDef } from '@decryp7/paddock-ui/shell';
import { DIALOG_REF, type DialogContent } from '@decryp7/paddock-ui/shell/dialogs';
import { fieldText, matchesQuery, settingsNav, type SettingsNavItem, SettingsStore } from '@decryp7/paddock-ui/shell/settings';
import { PdSettingsSection, PdSettingsSectionPage, SETTINGS_PAGE } from './settings-section';
import { SettingsDrafts } from './settings-drafts';

/** The settings pages a Settings window lists (the app's `settingsPages` contributions plus the shell's). */
export const SETTINGS_WINDOW_PAGES = new InjectionToken<readonly SettingsPageDef[]>('PADDOCK_SETTINGS_WINDOW_PAGES', {
  factory: () => [],
});

/** Restarts the app (keeping its session) so restart-only settings take effect; without it the bar has no button. */
export const SETTINGS_RESTART = new InjectionToken<() => void | Promise<void>>('PADDOCK_SETTINGS_RESTART');

/** A search hit: the fields of one section, or a page that matched by its title. */
interface Hit {
  readonly id: string;
  readonly title: string;
  readonly keys: readonly string[] | null;
}

interface LoadedPage {
  readonly component: Type<unknown>;
  readonly injector: Injector;
  readonly environment: EnvironmentInjector;
}

/**
 * The Settings window (a modeless `tool` dialog, ADR 0009): the pages by group in the navigation, a search across every
 * setting, the restart bar, the start-up problem, and the selected page loaded with its own providers. Edits apply by
 * themselves (a switch or a choice at once, typed values on Enter or leaving the control); one that cannot be applied
 * stays unsaved, is marked in the navigation and is asked about before the window closes.
 */
@Component({
  selector: 'pd-settings-window',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [NgComponentOutlet, PdSettingsSection],
  host: { class: 'pd-settings-window' },
  template: `
    <div class="top">
      <input
        #search
        class="search"
        type="search"
        [placeholder]="t.search"
        [attr.aria-label]="t.search"
        data-testid="settings-search"
        [value]="store.query()"
        (input)="store.setQuery($any($event.target).value)"
        (keydown.escape)="clearSearch($event)"
      />
      <label class="advanced">
        <input type="checkbox" role="switch" [checked]="store.showAdvanced()" (change)="store.setShowAdvanced($any($event.target).checked)" />
        {{ t.showAdvanced }}
      </label>
    </div>

    @if (store.problem(); as p) {
      <div class="banner warn" role="status" data-testid="settings-problem">{{ p.message }}</div>
    }
    @if (store.restartPending()) {
      <div class="banner restart" role="status" data-testid="settings-restart">
        <span>{{ restartText() }}</span>
        @if (restart) {
          <button type="button" class="btn primary" (click)="restart()">{{ t.restartNow }}</button>
        }
      </div>
    }
    @if (asking()) {
      <div class="banner warn" role="alertdialog" aria-labelledby="pd-settings-unsaved" data-testid="settings-unsaved">
        <span id="pd-settings-unsaved">{{ unsavedText() }}</span>
        <button type="button" class="btn" (click)="discardAndClose()">{{ t.discard }}</button>
        <button type="button" class="btn primary" (click)="keepEditing()">{{ t.keepEditing }}</button>
      </div>
    }
    @if (store.actionError(); as e) {
      <div class="banner err" role="alert">
        <span>{{ e }}</span>
        <button type="button" class="btn" (click)="store.dismissNotice()">{{ t.dismiss }}</button>
      </div>
    }

    <div class="main">
      <nav class="nav" [attr.aria-label]="t.pages" (keydown)="navKey($event)">
        @for (g of nav(); track g.id) {
          <h2 class="group" [id]="'pd-settings-group-' + g.id">{{ g.title }}</h2>
          <ul class="items" [attr.aria-labelledby]="'pd-settings-group-' + g.id">
            @for (item of g.items; track item.id) {
              <li>
                <button type="button" class="item" [attr.aria-current]="current()?.id === item.id && !searching() ? 'page' : null" [attr.data-page]="item.id" (click)="select(item.id)">
                  <span class="name">{{ item.title }}</span>
                  @if (unsaved().includes(item.id)) {
                    <span class="dot" role="img" [attr.aria-label]="t.unsaved" [title]="t.unsaved"></span>
                  }
                </button>
              </li>
            }
          </ul>
        }
      </nav>

      <section class="content" [attr.aria-label]="searching() ? t.results : (current()?.title ?? t.settings)" data-testid="settings-content">
        @if (store.requestError(); as e) {
          <div class="banner err" role="alert">
            <span>{{ loadFailed(e) }}</span>
            <button type="button" class="btn" (click)="store.load()">{{ t.retry }}</button>
          </div>
        } @else if (!store.isLoaded() && !store.overview()) {
          <p class="muted" role="status">{{ t.loadingSettings }}</p>
        } @else if (searching()) {
          @for (hit of hits(); track hit.id) {
            @if (hit.keys) {
              <pd-settings-section [section]="hit.id" [keys]="hit.keys" />
            } @else {
              <p class="page-hit"><button type="button" class="link" (click)="select(hit.id)">{{ hit.title }}</button></p>
            }
          } @empty {
            <p class="muted" role="status">{{ noMatch(store.query()) }}</p>
          }
        } @else if (loaded.error()) {
          <div class="banner err" role="alert">
            <span>{{ t.pageFailed }}</span>
            <button type="button" class="btn" (click)="loaded.reload()">{{ t.retry }}</button>
          </div>
        } @else if (loaded.value(); as p) {
          <ng-container *ngComponentOutlet="p.component; injector: p.injector; environmentInjector: p.environment" />
        } @else {
          <p class="muted" role="status">{{ t.loading }}</p>
        }
      </section>
    </div>

    <div class="foot">
      <span class="status" role="status" aria-live="polite">{{ store.notice() ?? '' }}</span>
      <span class="spacer"></span>
      <button type="button" class="btn" data-dialog-close (click)="close()">{{ t.close }}</button>
    </div>
  `,
  styles: `
    :host { display: flex; flex-direction: column; gap: var(--pd-space-2); height: 100%; min-height: 0; container-type: inline-size; }
    .top { display: flex; align-items: center; gap: var(--pd-space-3); flex-wrap: wrap; }
    .search { flex: 1 1 12rem; min-width: 0; min-height: var(--pd-button-height); padding: 0 var(--pd-space-2); border-radius: var(--pd-radius-control); border: 1px solid var(--pd-border-control); background: var(--pd-surface-1); color: var(--pd-text); font: inherit; }
    .advanced { display: inline-flex; align-items: center; gap: var(--pd-space-1); color: var(--pd-text-muted); white-space: nowrap; }
    .advanced input { accent-color: var(--pd-accent); }
    .banner { display: flex; align-items: center; gap: var(--pd-space-2); flex-wrap: wrap; padding: var(--pd-space-2); border-radius: var(--pd-radius-control); border: 1px solid var(--pd-hairline); background: var(--pd-surface-2); color: var(--pd-text); }
    .banner > span { flex: 1 1 12rem; min-width: 0; }
    .banner.warn { border-color: var(--pd-warning, var(--pd-border-control)); }
    .banner.err { border-color: var(--pd-danger-text, var(--pd-border-control)); }
    .banner.restart { border-color: var(--pd-accent); }
    .main { flex: 1; min-height: 0; display: flex; gap: var(--pd-space-4); }
    .nav { flex: none; width: 13rem; overflow: auto; border-inline-end: 1px solid var(--pd-hairline); padding-inline-end: var(--pd-space-2); }
    .group { margin: var(--pd-space-3) 0 var(--pd-space-1); padding: 0 var(--pd-space-2); font-size: var(--pd-text-caption); font-weight: var(--pd-weight-semibold); color: var(--pd-text-muted); }
    .group:first-child { margin-top: 0; }
    .items { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 2px; }
    .item { display: flex; align-items: center; gap: var(--pd-space-1); width: 100%; min-height: 30px; padding: 0 var(--pd-space-2); border: 0; border-radius: var(--pd-radius-control); background: transparent; color: var(--pd-text); font: inherit; text-align: start; cursor: pointer; }
    .item:hover { background: var(--pd-surface-2); }
    .item[aria-current='page'] { background: var(--pd-surface-2); box-shadow: inset 2px 0 0 var(--pd-accent); font-weight: var(--pd-weight-medium); }
    .item:focus-visible, .btn:focus-visible, .link:focus-visible, .search:focus-visible { outline: 2px solid var(--pd-accent); outline-offset: 1px; }
    .name { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .dot { flex: none; width: 8px; height: 8px; border-radius: 50%; background: var(--pd-accent); }
    .content { flex: 1; min-width: 0; overflow: auto; display: flex; flex-direction: column; gap: var(--pd-space-4); padding-inline-end: var(--pd-space-1); }
    .muted { color: var(--pd-text-muted); }
    .page-hit { margin: 0; }
    .link { border: 0; padding: 0; background: none; color: var(--pd-text); font: inherit; text-decoration: underline; cursor: pointer; }
    .btn { min-height: var(--pd-button-height); padding: 0 var(--pd-space-3); border-radius: var(--pd-radius-control); border: 1px solid var(--pd-border-control); background: var(--pd-surface-2); color: var(--pd-text); font: inherit; font-weight: var(--pd-weight-medium); cursor: pointer; }
    .btn.primary { background: var(--pd-accent); border-color: var(--pd-accent); color: var(--pd-on-accent, #fff); }
    .foot { display: flex; align-items: center; gap: var(--pd-space-2); padding-top: var(--pd-space-2); border-top: 1px solid var(--pd-hairline); }
    .status { min-width: 0; color: var(--pd-text-muted); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .spacer { flex: 1; }
    @container (max-width: 560px) {
      .main { flex-direction: column; }
      .nav { width: auto; max-height: 40%; border-inline-end: 0; border-bottom: 1px solid var(--pd-hairline); padding: 0 0 var(--pd-space-2); }
    }
  `,
})
export class PdSettingsWindow implements DialogContent {
  /** The component's texts in the window's language (plan S15: `$localize`, extracted from the package build). */
  protected readonly t = {
    search: $localize`:@@pd.settings.search:Search settings`,
    showAdvanced: $localize`:@@pd.settings.show-advanced:Show advanced`,
    restartNow: $localize`:@@pd.settings.restart-now:Restart now`,
    discard: $localize`:@@pd.settings.unsaved.discard:Discard changes`,
    keepEditing: $localize`:@@pd.settings.unsaved.keep:Keep editing`,
    dismiss: $localize`:@@pd.settings.dismiss:Dismiss`,
    pages: $localize`:@@pd.settings.pages:Settings pages`,
    unsaved: $localize`:@@pd.settings.unsaved:Unsaved changes`,
    results: $localize`:@@pd.settings.results:Search results`,
    settings: $localize`:@@pd.settings.title:Settings`,
    retry: $localize`:@@pd.settings.retry:Retry`,
    loadingSettings: $localize`:@@pd.settings.loading:Loading settings…`,
    pageFailed: $localize`:@@pd.settings.page-failed:This page could not be opened.`,
    loading: $localize`:@@pd.settings.loading-page:Loading…`,
    close: $localize`:@@pd.settings.close:Close`,
  };
  protected readonly store = inject(SettingsStore);
  private readonly drafts = inject(SettingsDrafts);
  private readonly pages = inject(SETTINGS_WINDOW_PAGES);
  private readonly environment = inject(EnvironmentInjector);
  private readonly injector = inject(Injector);
  private readonly host = inject<ElementRef<HTMLElement>>(ElementRef);
  private readonly ref = inject(DIALOG_REF, { optional: true });
  protected readonly restart = inject(SETTINGS_RESTART, { optional: true });

  /** Open at this page (a page or section id); the dialog's input. */
  readonly page = input<string | null>(null);

  /** "Discard changes / Keep editing" is showing. */
  protected readonly asking = signal(false);

  // Every page is listed: a Settings window boots with its own small provider set and no app context (rule 34), so a
  // page's `when` is not evaluated here; a page that depends on a condition says so in its own content.
  protected readonly nav = computed(() => settingsNav(this.pages, this.store.sections()));
  private readonly items = computed(() => this.nav().flatMap((g) => g.items));
  /** The page shown: the one chosen, else the dialog's input, else the first. */
  protected readonly current = computed<SettingsNavItem | null>(() => {
    const items = this.items();
    const want = this.store.selected() ?? this.page();
    return items.find((i) => i.id === want) ?? items[0] ?? null;
  });
  protected readonly searching = computed(() => this.store.query().trim().length > 0);
  protected readonly unsaved = computed(() => this.drafts.dirtySections());
  protected readonly unsavedTitles = computed(() => this.unsaved().map((id) => this.items().find((i) => i.id === id)?.title ?? id).join(', '));
  protected readonly unsavedText = computed(() => $localize`:@@pd.settings.unsaved.ask:Some changes are not saved: ${this.unsavedTitles()}:pages:.`);
  protected loadFailed(cause: string): string {
    return $localize`:@@pd.settings.load-failed:The settings could not be loaded: ${cause}:cause:`;
  }
  protected noMatch(query: string): string {
    return $localize`:@@pd.settings.no-match:No settings match “${query}:query:”.`;
  }
  protected readonly restartText = computed(() => {
    const n = this.store.restartChanges().length;
    if (n === 0) return $localize`:@@pd.settings.restart.some:Some changes take effect after the app restarts.`;
    return n === 1
      ? $localize`:@@pd.settings.restart.one:1 change takes effect after the app restarts (${this.store.restartChanges()[0]}:setting:).`
      : $localize`:@@pd.settings.restart.other:${n}:count: changes take effect after the app restarts.`;
  });

  /** Every field that matches the search (advanced ones too), by section, then pages whose title matches. */
  protected readonly hits = computed<readonly Hit[]>(() => {
    const q = this.store.query();
    const sectionHits = this.store
      .sections()
      .map((s) => ({ id: s.id, title: s.title, keys: s.fields.filter((f) => matchesQuery(fieldText(s, f), q)).map((f) => f.key) }))
      .filter((h) => h.keys.length > 0);
    const withSection = new Set(this.store.sections().map((s) => s.id));
    const pageHits = this.items()
      .filter((i) => !withSection.has(i.id) && matchesQuery(i.title, q))
      .map((i) => ({ id: i.id, title: i.title, keys: null }));
    return [...sectionHits, ...pageHits];
  });

  /** The current page's component, loaded with its own providers (rule 34, plan §9). */
  private readonly currentId = computed(() => this.current()?.id ?? null);
  protected readonly loaded = resource<LoadedPage | null, string | null>({
    // By id: a change of the values does not reload the page.
    params: () => this.currentId(),
    loader: async ({ params }) => {
      const item = this.items().find((i) => i.id === params);
      if (!item) return null;
      const page: SettingsPageDef = item.page ?? { id: item.id, group: 'advanced', title: { id: 'pd.settings.section', text: item.title }, order: 0 };
      // A page without a component of its own is its section in the generic form.
      const part = await loadPart({ load: page.load ?? (() => Promise.resolve(PdSettingsSectionPage)), providers: page.providers }, this.environment, `settings page ${page.id}`);
      this.replaceEnvironment(part.injector);
      return {
        component: part.component as Type<unknown>,
        environment: part.injector,
        injector: Injector.create({ providers: [{ provide: SETTINGS_PAGE, useValue: page }], parent: this.injector }),
      };
    },
  });

  private pageEnvironment: EnvironmentInjector | null = null;

  constructor() {
    inject(DestroyRef).onDestroy(() => this.replaceEnvironment(null));
  }

  /** A page's own providers live in an injector of their own; the previous page's goes when the next one loads. */
  private replaceEnvironment(next: EnvironmentInjector | null): void {
    if (this.pageEnvironment && this.pageEnvironment !== next && this.pageEnvironment !== this.environment) this.pageEnvironment.destroy();
    this.pageEnvironment = next;
  }

  protected select(id: string): void {
    this.store.select(id);
  }

  /** Escape in a filled search clears it (and does not close the window). */
  protected clearSearch(e: Event): void {
    if (!this.store.query()) return;
    e.preventDefault();
    e.stopPropagation();
    this.store.setQuery('');
  }

  /** Up/Down, Home/End move between the navigation's pages. */
  protected navKey(e: KeyboardEvent): void {
    if (!['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(e.key)) return;
    const buttons = [...this.host.nativeElement.querySelectorAll<HTMLButtonElement>('.nav .item')];
    const at = buttons.indexOf(document.activeElement as HTMLButtonElement);
    const last = buttons.length - 1;
    const next = e.key === 'Home' ? 0 : e.key === 'End' ? last : e.key === 'ArrowDown' ? Math.min(last, at + 1) : Math.max(0, at - 1);
    e.preventDefault();
    buttons[next]?.focus();
  }

  /** Unsaved edits keep the window open, by any route that closes it, and ask first. */
  dialogCanClose(): boolean {
    if (!this.unsaved().length) return true;
    this.asking.set(true);
    return false;
  }

  protected keepEditing(): void {
    this.asking.set(false);
  }

  protected discardAndClose(): void {
    this.drafts.discardAll();
    this.asking.set(false);
    this.close();
  }

  protected close(): void {
    this.ref?.close();
  }
}
