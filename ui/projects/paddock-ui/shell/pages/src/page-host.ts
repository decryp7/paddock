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
  styles: `:host { display: block; min-width: 0; }`,
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
