import { ChangeDetectionStrategy, Component, OnInit, computed, inject, input, signal } from '@angular/core';
import { RouterLink } from '@angular/router';
import { MatIconModule } from '@angular/material/icon';
import { bytes } from '../../shared/format';
import { UPDATE_KIND_LABEL } from '../../shared/labels';
import { UpdatesStore } from './updates.store';

/** Notices shown before "Show all". */
export const SHOWN = 3;

/**
 * Model updates on offer. `list` (the Models page): one row per notice with Get it, Remind me in a week and Dismiss.
 * `summary` (the dashboard's attention area): one line that links to Models. Nothing shows when there is nothing to
 * offer, and nothing here ever downloads by itself.
 */
@Component({
  selector: 'pw-update-banner',
  imports: [MatIconModule, RouterLink],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @if (store.count() > 0) {
      @if (mode() === 'summary') {
        <section class="banner summary" role="status" aria-label="Model updates">
          <mat-icon svgIcon="pw:download" aria-hidden="true" />
          <span class="text"><b class="num">{{ store.count() }}</b> {{ store.count() === 1 ? 'model update is' : 'model updates are' }} available. Pitwall only tells you; nothing downloads until you choose.</span>
          <a class="btn sm" routerLink="/models">Review</a>
        </section>
      } @else {
        <section class="banner" role="region" aria-label="Model updates">
          <header>
            <mat-icon svgIcon="pw:download" aria-hidden="true" />
            <p><b class="num">{{ store.count() }}</b> {{ store.count() === 1 ? 'update' : 'updates' }} available for models you downloaded. <span class="faint">Pitwall only tells you; nothing downloads until you choose.</span></p>
          </header>
          <ul>
            @for (n of shown(); track n.id) {
              <li [attr.data-kind]="n.kind">
                <div class="what">
                  <span class="line"><b class="mono">{{ n.modelId }}</b><span class="tag">Update available: {{ kind[n.kind] }}</span></span>
                  <span class="why">{{ n.summary }}</span>
                  <span class="meta num">{{ n.file }} · {{ size(n.sizeBytes) }} · {{ n.repo }}</span>
                </div>
                <div class="act">
                  <button type="button" class="btn primary sm" [disabled]="store.isBusy()(n.id)" [attr.aria-label]="'Get ' + n.file" (click)="store.get(n)">Get it</button>
                  <button type="button" class="btn sm" [disabled]="store.isBusy()(n.id)" [attr.aria-label]="'Remind me about ' + n.modelId + ' in a week'" (click)="store.dismiss(n.id, 7)">Remind me in a week</button>
                  <button type="button" class="btn ghost icon sm" [disabled]="store.isBusy()(n.id)" [attr.aria-label]="'Dismiss the update for ' + n.modelId" (click)="store.dismiss(n.id, null)"><mat-icon svgIcon="pw:close" aria-hidden="true" /></button>
                </div>
              </li>
            }
          </ul>
          @if (hidden() > 0) {
            <button type="button" class="btn ghost sm more" (click)="all.set(true)">Show all {{ store.count() }}</button>
          }
        </section>
      }
    }
  `,
  styles: `
    :host {
      display: contents;
    }
    .banner {
      display: grid;
      gap: var(--sp-2);
      padding: var(--sp-3) var(--sp-4);
      background: var(--accent-soft);
      border: 1px solid color-mix(in srgb, var(--accent) 32%, transparent);
      border-radius: var(--r-lg);
      mat-icon {
        color: var(--accent);
      }
    }
    .summary {
      grid-template-columns: auto minmax(0, 1fr) auto;
      align-items: center;
      gap: var(--sp-3);
      font-size: var(--fs-sm);
    }
    header {
      display: flex;
      align-items: center;
      gap: var(--sp-3);
      font-size: var(--fs-sm);
    }
    ul {
      display: grid;
      gap: var(--sp-2);
      margin: 0;
      padding: 0;
      list-style: none;
    }
    li {
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: var(--sp-3);
      padding: var(--sp-2) var(--sp-3);
      background: var(--bg-card);
      border: 1px solid var(--border);
      border-radius: var(--r-md);
    }
    .what {
      display: grid;
      gap: 2px;
      min-width: 0;
    }
    .line {
      display: flex;
      align-items: center;
      gap: var(--sp-2);
      flex-wrap: wrap;
    }
    .why {
      font-size: var(--fs-sm);
      color: var(--text-2);
    }
    .meta {
      font-size: var(--fs-xs);
      color: var(--text-3);
      overflow-wrap: anywhere;
    }
    .act {
      display: flex;
      align-items: center;
      gap: var(--sp-2);
      flex-shrink: 0;
    }
    .more {
      justify-self: start;
    }
    @media (max-width: 720px) {
      li {
        flex-direction: column;
        align-items: stretch;
      }
      .act {
        flex-wrap: wrap;
      }
    }
  `,
})
export class UpdateBanner implements OnInit {
  protected readonly store = inject(UpdatesStore);
  readonly mode = input<'list' | 'summary'>('list');
  protected readonly kind = UPDATE_KIND_LABEL;
  protected readonly size = bytes;
  protected readonly all = signal(false);
  protected readonly shown = computed(() => (this.all() ? this.store.notices() : this.store.notices().slice(0, SHOWN)));
  protected readonly hidden = computed(() => this.store.count() - this.shown().length);

  ngOnInit(): void {
    void this.store.ensureLoaded();
  }
}
