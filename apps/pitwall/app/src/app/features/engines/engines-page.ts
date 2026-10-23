import { ChangeDetectionStrategy, Component, OnInit, effect, inject, untracked } from '@angular/core';
import { MatIconModule } from '@angular/material/icon';
import { MatTooltipModule } from '@angular/material/tooltip';
import type { Engine } from '../../core/api.types';
import { LiveStore } from '../../core/live.store';
import { availabilityLook } from '../../shared/labels';
import { injectNow } from '../../shared/now';
import { Panel } from '../../shared/ui/panel';
import { EngineCard } from './engine-card';
import { EngineSetupBanner } from './engine-setup-banner';
import { EnginesStore } from './engines.store';

/** Engines: llama.cpp builds, catalog and processes; planned engines with the honest reason they aren't here yet. */
@Component({
  selector: 'pw-engines-page',
  imports: [MatIconModule, MatTooltipModule, Panel, EngineCard, EngineSetupBanner],
  providers: [EnginesStore],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="page">
      <header class="page-head">
        <div>
          <h1>Engines</h1>
          <p class="sub">The runtimes that serve models: pinned releases, verified and self-tested here.</p>
        </div>
        <div class="actions">
          <button type="button" class="btn ghost icon" aria-label="Refresh" matTooltip="Refresh" (click)="store.load()" [disabled]="store.loading()"><mat-icon svgIcon="pw:refresh" aria-hidden="true" /></button>
        </div>
      </header>

      @if (!store.loaded()) {
        <pw-panel heading="Engines" icon="engines" [loading]="store.loading()" [error]="store.error()" [ready]="false" [skeletonLines]="8" (retry)="store.load()" />
      } @else {
        @if (store.error(); as err) {
          <pw-panel heading="Engines" icon="engines" [error]="err" [ready]="false" (retry)="store.load()" />
        }
        @if (store.packProblems().length || store.packWarnings().length) {
          <section class="pack-notice callout" data-tone="warn" role="status" aria-label="Pack problems">
            <mat-icon svgIcon="pw:alert" aria-hidden="true" />
            <div>
              <p class="head">
                @if (store.packProblems().length) {
                  {{ store.packProblems().length }} pack {{ store.packProblems().length === 1 ? 'file was' : 'files were' }} skipped.
                } @else {
                  Packs loaded with notes.
                }
                Fix the file and use Refresh.
              </p>
              <ul>
                @for (n of store.packProblems(); track n.file + n.message) {
                  <li><span class="mono">{{ n.file }}</span> {{ n.message }}</li>
                }
                @for (n of store.packWarnings(); track n.file + n.message) {
                  <li class="warn"><span class="mono">{{ n.file }}</span> {{ n.message }}</li>
                }
              </ul>
            </div>
          </section>
        }
        <pw-engine-setup />
        <div class="stack">
          @for (e of store.available(); track e.kind) {
            <pw-engine-card [engine]="e" [now]="now()" />
          }
        </div>

        @if (store.planned().length) {
          <h2 class="section-title">Planned and other engines</h2>
          <ul class="planned" aria-label="Planned engines">
            @for (e of store.planned(); track e.kind) {
              <li>
                <span class="pname">{{ e.name }}</span>
                <span class="desc" [matTooltip]="e.description + ' ' + reason(e)" matTooltipShowDelay="500">{{ reason(e) || e.description }}</span>
                <span class="status" [attr.data-tone]="look(e).tone">{{ look(e).label }}</span>
              </li>
            }
          </ul>
        }
      }
    </div>
  `,
  styles: `
    :host {
      display: block;
    }
    .page {
      display: flex;
      flex-direction: column;
      gap: var(--sp-4);
    }
    .pack-notice {
      p {
        margin: 0;
      }
      .head {
        font-weight: var(--fw-semibold);
      }
      ul {
        margin: var(--sp-1) 0 0;
        padding: 0;
        list-style: none;
        color: var(--text-2);
      }
      .mono {
        font-size: var(--fs-sm);
      }
    }
    .stack {
      display: grid;
      gap: var(--sp-6);
    }
    .section-title {
      margin: var(--sp-2) 0 0;
      font-size: var(--fs-md);
      color: var(--text);
    }
    .planned {
      margin: calc(-1 * var(--sp-1)) 0 0;
      padding: 0;
      list-style: none;
      background: var(--bg-card);
      border: 1px solid var(--border);
      border-radius: var(--r-lg);
      li {
        display: grid;
        grid-template-columns: 160px minmax(0, 1fr) 110px;
        align-items: center;
        gap: var(--sp-3);
        min-height: 44px;
        padding: 0 var(--sp-4);
        border-top: 1px solid var(--border-soft);
        &:first-child {
          border-top: none;
        }
      }
      .status {
        justify-self: end;
      }
    }
    .pname {
      font-weight: var(--fw-semibold);
    }
    .desc {
      overflow: hidden;
      font-size: var(--fs-sm);
      color: var(--text-2);
      text-overflow: ellipsis;
      white-space: nowrap;
    }
  `,
})
export class EnginesPage implements OnInit {
  protected readonly store = inject(EnginesStore);
  private readonly live = inject(LiveStore);
  protected readonly now = injectNow();
  protected readonly look = (e: Engine) => availabilityLook(e.availability);

  constructor() {
    // A finished engine install shows up as an installed build: refresh once per download.
    const seen = new Set<string>();
    effect(() => {
      const done = this.live.downloads().filter((d) => d.kind === 'engine' && d.state === 'done' && !seen.has(d.id));
      if (!done.length) return;
      done.forEach((d) => seen.add(d.id));
      untracked(() => void this.store.load());
    });
  }

  ngOnInit(): void {
    void this.store.load();
  }

  protected reason(e: Engine): string {
    return e.availability.state === 'available' ? '' : e.availability.reason;
  }
}
