import { Dialog } from '@angular/cdk/dialog';
import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';
import { MatIcon } from '@angular/material/icon';
import type { EngineUse, PruneKind } from '../../core/api.types';
import { formatBytes, formatCount, plural, share } from '../../shared/format';
import { confirmAction } from '../../shared/ui/confirm-dialog';
import { LoadState } from '../../shared/ui/load-state';
import { EngineStore, PRUNE_KINDS } from './engine.store';
import { ENGINE_NAMES, PRUNE_LABELS, countOf, errorSentence } from './storage-labels';

/** The Container engine section: usage cards, the folders Pitcrew's tools fill, and the prune workflow. */
@Component({
  selector: 'pc-engine-section',
  imports: [MatIcon, LoadState],
  providers: [EngineStore],
  changeDetection: ChangeDetectionStrategy.OnPush,
  styleUrl: './storage-section.scss',
  template: `
    <section class="section" aria-labelledby="engine-title">
      <header class="section-head">
        <h2 id="engine-title">Container engine</h2>
        @if (report(); as r) {
          @if (r.engine) {
            <span class="chip" data-testid="engine-name"><mat-icon svgIcon="pc:server" aria-hidden="true" />{{ engineName() }}</span>
          }
        }
        <button type="button" class="btn sm" (click)="store.load()" [disabled]="store.report().loading">
          <mat-icon svgIcon="pc:refresh" aria-hidden="true" [class.spin]="store.report().loading" />Refresh
        </button>
      </header>

      <pc-load-state what="The container engine report" [hasValue]="!!report()" [error]="store.report().error" [skeleton]="[90, 90]" (retry)="store.load()">
        @if (report(); as r) {
          @if (r.engine === null || r.problem) {
            <div class="callout warning" role="status" data-testid="engine-problem">
              <mat-icon svgIcon="pc:alert" aria-hidden="true" />
              <div class="callout-body">
                <strong>{{ r.engine === null ? 'No container engine found' : 'The container engine reported a problem' }}</strong>
                <span>{{ r.problem ?? 'Docker or Podman is not installed or not running.' }}</span>
                <span class="muted">Next step: start Docker or Podman (or install it), then press Refresh. Pruning is unavailable until then.</span>
              </div>
            </div>
          }

          @if (r.usage.length > 0) {
            <div class="cards">
              @for (c of cards(); track c.use.what) {
                <article class="card usage-card" [attr.data-testid]="'usage-' + c.use.what">
                  <header class="card-header"><h3>{{ c.use.what }}</h3></header>
                  <div class="card-body">
                    <p class="hero num">{{ c.size }}</p>
                    <p class="muted small">{{ c.counts }}</p>
                    <div class="meter" aria-hidden="true"><span [style.width.%]="c.percent"></span></div>
                    <p class="small"><strong class="num">{{ c.reclaimable }}</strong>&nbsp;<span class="muted">can be reclaimed</span></p>
                  </div>
                </article>
              }
            </div>
          }

          @if (r.folders.length > 0) {
            <article class="card">
              <header class="card-header"><h3>Folders</h3></header>
              <div class="card-body flush">
                <ul class="folders">
                  @for (f of folders(); track f.folder.path) {
                    <li>
                      <div class="folder-head">
                        <strong>{{ f.folder.label }}</strong>
                        <span class="num">{{ f.size }}</span>
                        <span class="muted small">{{ f.entries }}</span>
                      </div>
                      <div class="mono path">{{ f.folder.path }}</div>
                      @if (!f.folder.exists) {
                        <span class="faint small">This folder does not exist yet.</span>
                      }
                      @if (f.folder.truncated) {
                        <span class="faint small">Truncated: the scan stopped early, so the size and count are lower bounds.</span>
                      }
                    </li>
                  }
                </ul>
              </div>
            </article>
          }

          @if (store.usable()) {
            <article class="card" aria-labelledby="prune-title">
              <header class="card-header"><h3 id="prune-title">Prune</h3></header>
              <div class="card-body form">
                <fieldset class="kinds">
                  <legend class="section-label">What to remove</legend>
                  @for (k of kinds; track k) {
                    <label class="check kind">
                      <input type="checkbox" [checked]="store.kinds().includes(k)" (change)="toggle(k, $event)" [attr.data-kind]="k" />
                      <span>
                        <span>{{ labels[k].label }}</span>
                        <span class="small" [class.tone-warning]="k === 'volumes'" [class.muted]="k !== 'volumes'"> {{ labels[k].hint }}</span>
                      </span>
                    </label>
                  }
                </fieldset>
                <div class="field narrow">
                  <label for="older-than">Older than</label>
                  <div class="with-unit">
                    <input id="older-than" class="input num" type="number" min="0" step="1" [value]="store.olderThanDays()" (input)="setDays($event)" />
                    <span>days</span>
                  </div>
                  <span class="faint small">0 means any age. Applies to unused images, build cache and stopped containers.</span>
                </div>

                <div class="btn-row">
                  <button type="button" class="btn" data-testid="preview" (click)="preview()" [disabled]="!store.canPreview()">
                    <mat-icon svgIcon="pc:search" aria-hidden="true" />{{ store.previewing() ? 'Previewing…' : 'Preview' }}
                  </button>
                  <button type="button" class="btn danger" data-testid="prune" (click)="prune()" [disabled]="!store.canPrune()">
                    <mat-icon svgIcon="pc:trash" aria-hidden="true" />{{ store.pruning() ? 'Pruning…' : 'Prune...' }}
                  </button>
                </div>
                @if (!store.currentPreview() && !store.previewing()) {
                  <p class="faint small">Preview first: it lists what would go and deletes nothing. Changing the choices above asks for a new preview.</p>
                }

                @if (store.actionError(); as err) {
                  <div class="callout" [class.warning]="err.code === 'busy'" [class.danger]="err.code !== 'busy'" role="alert" data-testid="engine-action-error">
                    <mat-icon svgIcon="pc:alert" aria-hidden="true" />
                    <div class="callout-body">
                      <strong>{{ err.code === 'busy' ? 'Not right now' : 'That did not work' }}</strong>
                      <span>{{ errorText() }}</span>
                    </div>
                    <button type="button" class="btn sm" (click)="retry()">Retry</button>
                  </div>
                }

                @if (previewRows(); as p) {
                  <div class="plan" data-testid="engine-preview" role="region" aria-label="Preview of the prune">
                    <table class="table">
                      <caption class="visually-hidden">What the prune would remove</caption>
                      <thead>
                        <tr><th scope="col">Kind</th><th scope="col" class="num">Count</th><th scope="col" class="num">Size</th><th scope="col">Examples</th></tr>
                      </thead>
                      <tbody>
                        @for (row of p.rows; track row.kind) {
                          <tr>
                            <td>{{ row.label }}</td>
                            <td class="num">{{ row.count }}</td>
                            <td class="num">{{ row.size }}</td>
                            <td class="mono small examples">{{ row.examples }}</td>
                          </tr>
                        }
                      </tbody>
                    </table>
                    <p class="total"><strong class="num">{{ p.total }}</strong>&nbsp;<span class="muted">would be freed. Nothing has been deleted.</span></p>
                  </div>
                }

                @if (outcome(); as o) {
                  <div class="callout" [class.success]="o.failures.length === 0" [class.warning]="o.failures.length > 0" role="status" data-testid="engine-result">
                    <mat-icon [svgIcon]="o.failures.length === 0 ? 'pc:check-circle' : 'pc:alert'" aria-hidden="true" />
                    <div class="callout-body">
                      <strong>{{ o.title }}</strong>
                      @for (f of o.freed; track f.label) {
                        <span>{{ f.label }}: <span class="num">{{ f.size }}</span></span>
                      }
                      @for (f of o.failures; track f.label) {
                        <span class="tone-warning">{{ f.label }} failed: {{ f.message }}</span>
                      }
                    </div>
                  </div>
                }
              </div>
            </article>
          }
        }
      </pc-load-state>
    </section>
  `,
})
export class EngineSection {
  protected readonly store = inject(EngineStore);
  private readonly dialog = inject(Dialog);

  protected readonly kinds = PRUNE_KINDS;
  protected readonly labels = PRUNE_LABELS;
  protected readonly report = computed(() => this.store.report().value);
  protected readonly engineName = computed(() => {
    const e = this.report()?.engine;
    return e ? ENGINE_NAMES[e] : '';
  });
  private readonly lastAction = signal<'preview' | 'prune'>('preview');

  protected readonly cards = computed(() =>
    (this.report()?.usage ?? []).map((use: EngineUse) => ({
      use,
      size: formatBytes(use.bytes),
      reclaimable: formatBytes(use.reclaimableBytes),
      percent: Math.round(share(use.reclaimableBytes, use.bytes) * 100),
      counts: `${formatCount(use.total)} in total, ${formatCount(use.active)} active`,
    })),
  );
  protected readonly folders = computed(() =>
    (this.report()?.folders ?? []).map((folder) => ({ folder, size: formatBytes(folder.bytes), entries: plural(folder.entries, 'entry', 'entries') })),
  );
  protected readonly previewRows = computed(() => {
    const p = this.store.currentPreview();
    if (!p) return null;
    return {
      total: formatBytes(p.totalBytes),
      rows: p.items.map((i) => ({
        kind: i.kind,
        label: PRUNE_LABELS[i.kind].label,
        count: formatCount(i.count),
        size: formatBytes(i.bytes),
        examples: i.examples.length > 0 ? i.examples.join(', ') : 'none listed',
      })),
    };
  });
  protected readonly errorText = computed(() => {
    const e = this.store.actionError();
    return e ? errorSentence(e) : '';
  });
  protected readonly outcome = computed(() => {
    const r = this.store.result();
    if (!r) return null;
    const total = r.freed.reduce((s, [, b]) => s + b, 0);
    return {
      title: r.failures.length === 0 ? `Freed ${formatBytes(total)}` : `Freed ${formatBytes(total)}, but ${plural(r.failures.length, 'part', 'parts')} failed`,
      freed: r.freed.map(([k, b]) => ({ label: PRUNE_LABELS[k].label, size: formatBytes(b) })),
      failures: r.failures.map(([k, message]) => ({ label: PRUNE_LABELS[k].label, message })),
    };
  });

  constructor() {
    void this.store.load();
  }

  protected toggle(kind: PruneKind, event: Event): void {
    this.store.toggle(kind, (event.target as HTMLInputElement).checked);
  }

  protected setDays(event: Event): void {
    this.store.setOlderThanDays(Number((event.target as HTMLInputElement).value));
  }

  protected async preview(): Promise<void> {
    this.lastAction.set('preview');
    await this.store.runPreview();
  }

  /** Asks first, naming what goes and how much; nothing is removed unless the user confirms. */
  protected async prune(): Promise<void> {
    this.lastAction.set('prune');
    const p = this.store.currentPreview();
    if (!p || !this.store.canPrune()) return;
    const what = p.items.map((i) => `${countOf(i.kind, i.count)} (${PRUNE_LABELS[i.kind].label.toLowerCase()})`).join(', ');
    const ok = await confirmAction(this.dialog, {
      title: `Remove ${formatBytes(p.totalBytes)} from ${this.engineName()}?`,
      body: `This removes ${what}. It cannot be undone.${this.store.selection().kinds.includes('volumes') ? ' Volumes hold data: unused volumes are deleted with what is in them.' : ''}`,
      confirm: 'Prune now',
      danger: true,
    });
    if (ok) await this.store.prune();
  }

  protected retry(): Promise<void> {
    return this.lastAction() === 'prune' ? this.prune() : this.preview();
  }
}
