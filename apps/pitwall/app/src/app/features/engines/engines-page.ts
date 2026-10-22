import { ChangeDetectionStrategy, Component, OnInit, effect, inject, untracked } from '@angular/core';
import { MatIconModule } from '@angular/material/icon';
import type { Engine } from '../../core/api.types';
import { LiveStore } from '../../core/live.store';
import { availabilityLook } from '../../shared/labels';
import { injectNow } from '../../shared/now';
import { Panel } from '../../shared/ui/panel';
import { EngineCard } from './engine-card';
import { EnginesStore } from './engines.store';

/** Engines: llama.cpp builds, catalog and processes; planned engines with the honest reason they aren't here yet. */
@Component({
  selector: 'pw-engines-page',
  imports: [MatIconModule, Panel, EngineCard],
  providers: [EnginesStore],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="page">
      <header class="page-head">
        <div>
          <h1>Engines</h1>
          <p class="sub">The runtimes that serve models. Builds are pinned releases, verified and self-tested on this machine.</p>
        </div>
        <div class="actions">
          <button type="button" class="btn" (click)="store.load()" [disabled]="store.loading()"><mat-icon svgIcon="pw:refresh" aria-hidden="true" />Refresh</button>
        </div>
      </header>

      @if (!store.loaded()) {
        <pw-panel heading="Engines" icon="engines" [loading]="store.loading()" [error]="store.error()" [ready]="false" [skeletonLines]="8" (retry)="store.load()" />
      } @else {
        @if (store.error(); as err) {
          <pw-panel heading="Engines" icon="engines" [error]="err" [ready]="false" (retry)="store.load()" />
        }
        @if (store.packProblems().length || store.packWarnings().length) {
          <section class="pack-notice" role="status" aria-label="Pack problems">
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
                <div class="ptop">
                  <span class="pname">{{ e.name }}</span>
                  <span class="pill" [attr.data-tone]="look(e).tone">{{ look(e).label }}</span>
                </div>
                <p class="desc">{{ e.description }}</p>
                <p class="reason"><mat-icon svgIcon="pw:info" aria-hidden="true" />{{ reason(e) }}</p>
              </li>
            }
          </ul>
        }
      }
    </div>
  `,
  styles: `
    .pack-notice {
      display: flex;
      gap: var(--sp-3);
      padding: var(--sp-3) var(--sp-4);
      background: var(--warn-soft);
      border: 1px solid var(--border);
      border-radius: var(--r-lg);
      font-size: var(--fs-sm);
      mat-icon {
        flex: none;
        width: 16px;
        height: 16px;
        color: var(--warn);
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
      li.warn {
        color: var(--text-3);
      }
    }
    .stack {
      display: grid;
      gap: var(--sp-3);
    }
    .planned {
      display: grid;
      grid-template-columns: repeat(auto-fill, minmax(260px, 1fr));
      gap: var(--sp-3);
      margin: 0;
      padding: 0;
      list-style: none;
      li {
        display: flex;
        flex-direction: column;
        gap: var(--sp-2);
        padding: var(--sp-4);
        background: var(--bg-card);
        border: 1px dashed var(--border-strong);
        border-radius: var(--r-lg);
      }
    }
    .ptop {
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: var(--sp-2);
    }
    .pname {
      font-weight: var(--fw-semibold);
    }
    .desc {
      font-size: var(--fs-sm);
      color: var(--text-2);
    }
    .reason {
      display: flex;
      gap: 6px;
      margin-top: auto;
      font-size: var(--fs-sm);
      color: var(--text-3);
      mat-icon {
        flex: none;
        width: 14px;
        height: 14px;
        margin-top: 2px;
      }
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
