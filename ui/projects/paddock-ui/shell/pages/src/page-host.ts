import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { ActivatedRoute, Router } from '@angular/router';
import { SuErrorState } from '@decryp7/paddock-ui';
import { ContributionRegistry, PdPartOutlet, type PageDef } from '@decryp7/paddock-ui/shell';
import { PAGE_DATA, ShellPages } from './pages';

/**
 * The routed body of a page: the page's component, loaded on first show with the providers it declares (`loadPart`, in an
 * injector of its own that ends with the page), a skeleton while it loads and an error with Try again if it fails.
 */
@Component({
  selector: 'pd-page-host',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [PdPartOutlet, SuErrorState],
  host: { '[attr.data-page]': 'page?.id ?? null' },
  template: `
    @if (page) {
      <pd-part-outlet [part]="page" [label]="page.title.text" variant="page" />
    } @else {
      <pd-error-state [heading]="t.missing" [cause]="t.choose" [retryLabel]="t.home" (retry)="home()" />
    }
  `,
  // A definite height for the routed page (ADR 0012): the host fills <main> as a flex column, so a page's own `height: 100%`
  // (a CDK virtual viewport, a split pane) resolves instead of collapsing to 0. A page that is taller than <main> still
  // scrolls <main>; a page that wants less keeps its natural height. `pd-part-outlet.pd-page` is the loaded page's box.
  styles: `
    :host { display: flex; flex-direction: column; height: 100%; min-height: 0; min-width: 0; }
    :host > ::ng-deep pd-part-outlet.pd-page { display: flex; flex-direction: column; flex: 1 1 auto; min-height: 0; }
  `,
})
export class PdPageHost {
  /** The component's texts in the window's language (plan S15: `$localize`, extracted from the package build). */
  protected readonly t = {
    missing: $localize`:@@pd.pages.missing:This page does not exist.`,
    choose: $localize`:@@pd.pages.missing.cause:Choose a page in the sidebar.`,
    home: $localize`:@@pd.pages.missing.home:Go to the first page`,
  };
  private readonly route = inject(ActivatedRoute);
  protected readonly page: PageDef | undefined = inject(ContributionRegistry).find<PageDef>('pages', String(this.route.snapshot.data[PAGE_DATA] ?? ''));

  private readonly router = inject(Router);
  private readonly pages = inject(ShellPages);

  protected home(): void {
    void this.router.navigateByUrl(this.pages.defaultUrl());
  }
}
