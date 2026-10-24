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
          <h2 data-testid="whats-new-heading">What’s New in {{ n.version }}</h2>
          @if (n.release; as r) {
            @if (r.date) {
              <p class="muted">Released {{ r.date }}</p>
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
            <p data-testid="whats-new-none">There are no release notes for version {{ n.version }}.</p>
          }
        }
      }
      @case ('error') {
        <p role="alert">The release notes could not be loaded. <button type="button" class="btn" (click)="notes.reload()">Retry</button></p>
      }
      @default {
        <p class="muted" role="status">Loading…</p>
      }
    }
  `,
  styles: ABOUT_STYLES,
})
export class PdWhatsNewDialog {
  private readonly backend = inject(AboutBackend);
  protected readonly notes = resource({ loader: () => this.backend.whatsNew() });
}
