import { ChangeDetectionStrategy, Component, inject, resource } from '@angular/core';
import { AboutBackend } from './backend';
import { ABOUT_STYLES } from './styles';

/**
 * What's New (a viewer): the release notes of the running version, from the engine. A version without notes says so;
 * another release's notes are never shown in their place.
 */
@Component({
  selector: 'pd-whats-new-dialog',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @switch (notes.status()) {
      @case ('resolved') {
        @if (notes.value(); as n) {
          <h2 data-testid="whats-new-heading">{{ heading(n.version) }}</h2>
          @if (n.release; as r) {
            @if (r.date) {
              <p class="pd-muted">{{ released(r.date) }}</p>
            }
            @for (g of r.groups; track g.title) {
              <section [attr.aria-label]="g.title">
                <h3>{{ g.title }}</h3>
                <ul>
                  @for (item of g.items; track $index) {
                    <li>{{ item }}</li>
                  }
                </ul>
              </section>
            }
          } @else {
            <p data-testid="whats-new-none">{{ none(n.version) }}</p>
          }
        }
      }
      @case ('error') {
        <p role="alert">{{ t.failed }} <button type="button" class="pd-btn" (click)="notes.reload()">{{ t.retry }}</button></p>
      }
      @default {
        <p class="pd-muted" role="status">{{ t.loading }}</p>
      }
    }
  `,
  styles: ABOUT_STYLES,
})
export class PdWhatsNewDialog {
  /** The component's texts in the window's language (plan S15). Release notes are the app's text, shown as written. */
  protected readonly t = {
    failed: $localize`:@@pd.whats-new.failed:The release notes could not be loaded.`,
    retry: $localize`:@@pd.whats-new.retry:Retry`,
    loading: $localize`:@@pd.whats-new.loading:Loading…`,
  };
  private readonly backend = inject(AboutBackend);
  protected readonly notes = resource({ loader: () => this.backend.whatsNew() });

  protected heading(version: string): string {
    return $localize`:@@pd.whats-new.heading:What’s new in ${version}:version:`;
  }

  protected released(date: string): string {
    return $localize`:@@pd.whats-new.released:Released ${date}:date:`;
  }

  protected none(version: string): string {
    return $localize`:@@pd.whats-new.none:There are no release notes for version ${version}:version:.`;
  }
}
