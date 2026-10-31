import { ChangeDetectionStrategy, Component, computed, effect, inject, signal, untracked } from '@angular/core';
import { SHELL_PORTS } from '@decryp7/paddock-ui/shell';
import { RouterLink } from '@angular/router';
import { MatIcon } from '@angular/material/icon';
import { ClockService } from '../../core/clock.service';
import type { CacheReason } from '../../core/api.types';
import { formatBytes, formatCount, formatRelative, plural, share } from '../../shared/format';
import { confirmAction } from '../../shared/confirm';
import { LoadState } from '../../shared/ui/load-state';
import { StatusStore } from '../../state/status.store';
import { CacheStore } from './cache.store';
import { errorSentence } from './storage-labels';

const REASONS: Record<CacheReason, string> = { old: 'older than limit', overLimit: 'over the size limit' };

/** The Actions cache section: one runner's cache server folder, and trimming it by size and age. */
@Component({
  selector: 'pc-cache-section',
  imports: [MatIcon, LoadState, RouterLink],
  providers: [CacheStore],
  changeDetection: ChangeDetectionStrategy.OnPush,
  styleUrl: './storage-section.scss',
  template: `
    <section class="section" aria-labelledby="cache-title">
      <header class="section-head">
        <h2 id="cache-title">Actions cache</h2>
        @if (runners().length > 0) {
          <div class="segmented" role="group" aria-label="Runner">
            @for (r of runners(); track r.name) {
              <button type="button" [attr.aria-pressed]="r.name === store.runner()" (click)="pick(r.name)">{{ r.name }}</button>
            }
          </div>
        }
      </header>

      @if (runners().length === 0) {
        <p class="muted" data-testid="no-runners">No runners are set up yet, so there is no Actions cache to show.</p>
      } @else {
        <pc-load-state what="The cache report" [hasValue]="!!info()" [error]="store.report().error" [skeleton]="[90, 120]" (retry)="store.retry()">
          @if (info(); as c) {
            <article class="card" aria-labelledby="cache-info-title">
              <header class="card-header">
                <h3 id="cache-info-title">{{ store.runner() }}</h3>
                <span class="chip" data-testid="cache-enabled">{{ c.enabled === null ? 'Cache server: unknown' : c.enabled ? 'Cache server: enabled' : 'Cache server: disabled' }}</span>
              </header>
              <div class="card-body">
                <div class="mono path" data-testid="cache-path">{{ c.dir }}</div>
                @if (c.defaultDir) {
                  <span class="faint small">Default location.</span>
                }
                @if (c.problem) {
                  <div class="callout warning" role="status" data-testid="cache-problem">
                    <mat-icon svgIcon="pc:alert" aria-hidden="true" />
                    <div class="callout-body"><strong>Problem with the cache folder</strong><span>{{ c.problem }}</span></div>
                  </div>
                }
                @if (c.overLimit) {
                  <div class="callout warning" role="status" data-testid="cache-over-limit">
                    <mat-icon svgIcon="pc:alert" aria-hidden="true" />
                    <div class="callout-body">
                      <strong>Over the size limit</strong>
                      <span>The cache is {{ facts().size }}, above the limit of {{ facts().limit }}. Trim it below, or the runner will refuse new entries.</span>
                    </div>
                  </div>
                }
                <div class="facts">
                  <div class="fact"><span class="section-label">Size</span><span class="hero num">{{ facts().size }}</span></div>
                  <div class="fact"><span class="section-label">Entries</span><span class="num">{{ facts().entries }}{{ c.truncated ? ' (at least)' : '' }}</span></div>
                  <div class="fact"><span class="section-label">Size limit</span><span class="num">{{ facts().limit }}</span></div>
                  <div class="fact"><span class="section-label">Free on the disk</span><span class="num">{{ facts().free }}</span></div>
                </div>
                @if (facts().percent !== null) {
                  <div class="meter" [class.warning]="c.overLimit" aria-hidden="true"><span [style.width.%]="facts().percent"></span></div>
                }
                @if (c.truncated) {
                  <span class="faint small">Truncated: the scan stopped early, so the size and count are lower bounds.</span>
                }
                <p class="small">
                  <a [routerLink]="['/runners', store.runner()]" [queryParams]="{ tab: 'config' }" data-testid="cache-config-link">Turn the cache server on or off</a>
                </p>
              </div>
            </article>

            @if (top().length > 0) {
              <article class="card" aria-labelledby="cache-top-title">
                <header class="card-header"><h3 id="cache-top-title">Largest entries</h3></header>
                <div class="card-body flush">
                  <table class="table">
                    <caption class="visually-hidden">Largest cache entries of {{ store.runner() }}</caption>
                    <thead><tr><th scope="col">Name</th><th scope="col" class="num">Size</th><th scope="col">Last changed</th></tr></thead>
                    <tbody>
                      @for (e of top(); track e.name) {
                        <tr><td class="mono path">{{ e.name }}</td><td class="num">{{ e.size }}</td><td class="muted nowrap">{{ e.changed }}</td></tr>
                      }
                    </tbody>
                  </table>
                </div>
              </article>
            }

            <article class="card" aria-labelledby="trim-title">
              <header class="card-header"><h3 id="trim-title">Trim the cache</h3></header>
              <div class="card-body form">
                <div class="fields">
                  <div class="field narrow">
                    <label for="keep-gb">Keep at most</label>
                    <div class="with-unit">
                      <input id="keep-gb" class="input num" type="number" min="0" step="any" [value]="store.maxGb() ?? ''" (input)="setGb($event)" />
                      <span>GB</span>
                    </div>
                  </div>
                  <div class="field narrow">
                    <label for="older-days">Delete entries older than</label>
                    <div class="with-unit">
                      <input id="older-days" class="input num" type="number" min="0" step="1" [value]="store.maxAgeDays() ?? ''" (input)="setDays($event)" />
                      <span>days</span>
                    </div>
                  </div>
                </div>
                <p class="faint small">Set at least one. Entries are deleted oldest first.</p>

                <div class="btn-row">
                  <button type="button" class="btn" data-testid="cache-preview" (click)="preview()" [disabled]="!store.canPreview()">
                    <mat-icon svgIcon="pc:search" aria-hidden="true" />{{ store.previewing() ? 'Previewing…' : 'Preview' }}
                  </button>
                  <button type="button" class="btn danger" data-testid="cache-clean" (click)="clean()" [disabled]="!store.canClean()">
                    <mat-icon svgIcon="pc:trash" aria-hidden="true" />{{ store.cleaning() ? 'Cleaning…' : 'Clean...' }}
                  </button>
                </div>
                @if (!store.limits()) {
                  <p class="faint small" data-testid="cache-limits-hint">Enter a size limit, an age limit, or both to preview.</p>
                }

                @if (store.actionError(); as err) {
                  <div class="callout" [class.warning]="err.code === 'busy'" [class.danger]="err.code !== 'busy'" role="alert" data-testid="cache-action-error">
                    <mat-icon svgIcon="pc:alert" aria-hidden="true" />
                    <div class="callout-body">
                      <strong>{{ err.code === 'busy' ? 'Not right now' : 'That did not work' }}</strong>
                      <span>{{ errorText() }}</span>
                    </div>
                    @if (err.code !== 'badRequest') {
                      <button type="button" class="btn sm" (click)="retry()">Retry</button>
                    }
                  </div>
                }

                @if (plan(); as p) {
                  <div class="plan" data-testid="cache-plan" role="region" aria-label="Preview of the cache clean">
                    @if (p.rows.length === 0) {
                      <p class="muted">Nothing to delete with these limits.</p>
                    } @else {
                      <table class="table">
                        <caption class="visually-hidden">Entries that would be deleted</caption>
                        <thead><tr><th scope="col">Name</th><th scope="col" class="num">Size</th><th scope="col">Why</th></tr></thead>
                        <tbody>
                          @for (row of p.rows; track row.name) {
                            <tr><td class="mono path">{{ row.name }}</td><td class="num">{{ row.size }}</td><td>{{ row.reason }}</td></tr>
                          }
                        </tbody>
                      </table>
                    }
                    <p class="total"><strong class="num">{{ p.total }}</strong>&nbsp;<span class="muted">in {{ p.count }} would be freed. Nothing has been deleted.</span></p>
                  </div>
                }

                @if (outcome(); as o) {
                  <div class="callout" [class.success]="o.failures.length === 0" [class.warning]="o.failures.length > 0" role="status" data-testid="cache-result">
                    <mat-icon [svgIcon]="o.failures.length === 0 ? 'pc:check-circle' : 'pc:alert'" aria-hidden="true" />
                    <div class="callout-body">
                      <strong>{{ o.title }}</strong>
                      @for (f of o.failures; track f.name) {
                        <span class="tone-warning"><span class="mono">{{ f.name }}</span> was not deleted: {{ f.message }}</span>
                      }
                    </div>
                  </div>
                }
              </div>
            </article>
          }
        </pc-load-state>
      }
    </section>
  `,
})
export class CacheSection {
  protected readonly store = inject(CacheStore);
  private readonly status = inject(StatusStore);
  private readonly ports = inject(SHELL_PORTS);
  private readonly clock = inject(ClockService);
  private readonly lastAction = signal<'preview' | 'clean'>('preview');

  protected readonly runners = this.status.runners;
  protected readonly info = computed(() => this.store.report().value);
  protected readonly facts = computed(() => {
    const c = this.info();
    if (!c) return { size: '', entries: '', limit: '', free: '', percent: null as number | null };
    return {
      size: formatBytes(c.bytes),
      entries: formatCount(c.entries),
      limit: c.sizeLimitMb === null ? 'none' : formatBytes(c.sizeLimitMb * 1024 * 1024),
      free: c.freeBytes === null ? 'unknown' : formatBytes(c.freeBytes),
      percent: c.sizeLimitMb === null ? null : Math.round(share(c.bytes, c.sizeLimitMb * 1024 * 1024) * 100),
    };
  });
  protected readonly top = computed(() => {
    const now = this.clock.now();
    return (this.info()?.top ?? []).map((e) => ({ name: e.name, size: formatBytes(e.bytes), changed: formatRelative(e.modified, now) }));
  });
  protected readonly plan = computed(() => {
    const p = this.store.currentPreview();
    if (!p) return null;
    return {
      total: formatBytes(p.bytes),
      count: plural(p.entries.length, 'entry', 'entries'),
      rows: p.entries.map(([e, reason]) => ({ name: e.name, size: formatBytes(e.bytes), reason: REASONS[reason] })),
    };
  });
  protected readonly errorText = computed(() => {
    const e = this.store.actionError();
    return e ? errorSentence(e) : '';
  });
  protected readonly outcome = computed(() => {
    const r = this.store.result();
    if (!r) return null;
    const done = r.plan.entries.length - r.failures.length;
    return {
      title: r.failures.length === 0 ? `Deleted ${plural(done, 'entry', 'entries')}, freed ${formatBytes(r.plan.bytes)}` : `Deleted ${plural(Math.max(0, done), 'entry', 'entries')}; ${plural(r.failures.length, 'entry', 'entries')} could not be deleted`,
      failures: r.failures.map(([name, message]) => ({ name, message })),
    };
  });

  constructor() {
    effect(() => {
      const list = this.runners();
      const current = untracked(() => this.store.runner());
      if (list.length > 0 && (current === null || !list.some((r) => r.name === current))) untracked(() => void this.store.load(list[0]!.name));
    });
  }

  protected pick(name: string): void {
    if (name !== this.store.runner()) void this.store.load(name);
  }

  protected setGb(event: Event): void {
    this.store.setMaxGb(parse(event));
  }

  protected setDays(event: Event): void {
    this.store.setMaxAgeDays(parse(event));
  }

  protected async preview(): Promise<void> {
    this.lastAction.set('preview');
    await this.store.runPreview();
  }

  /** Asks first, naming what goes and how much; nothing is deleted unless the user confirms. */
  protected async clean(): Promise<void> {
    this.lastAction.set('clean');
    const p = this.store.currentPreview();
    if (!p || !this.store.canClean()) return;
    const ok = await confirmAction(this.ports, {
      title: `Delete ${formatBytes(p.bytes)} from the cache of ${this.store.runner()}?`,
      body: `This deletes ${plural(p.entries.length, 'cache entry', 'cache entries')}. Workflows that need them will rebuild them. It cannot be undone.`,
      confirm: 'Delete now',
      danger: true,
    });
    if (ok) await this.store.clean();
  }

  protected retry(): Promise<void> {
    return this.lastAction() === 'clean' ? this.clean() : this.preview();
  }
}

/** The number in a field, or null when it is empty or not a number. */
function parse(event: Event): number | null {
  const raw = (event.target as HTMLInputElement).value.trim();
  if (raw === '') return null;
  const n = Number(raw);
  return Number.isFinite(n) ? n : null;
}
