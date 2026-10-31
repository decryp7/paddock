import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';
import { SHELL_PORTS } from '@decryp7/paddock-ui/shell';
import { CdkMenu, CdkMenuItem, CdkMenuTrigger } from '@angular/cdk/menu';
import { MatIcon } from '@angular/material/icon';
import type { EngineUse, PruneKind } from '../../core/api.types';
import { formatBytes, formatCount, plural, share } from '../../shared/format';
import { Notify } from '../../state/notify';
import { confirmAction } from '../../shared/confirm';
import { LoadState } from '../../shared/ui/load-state';
import { EngineStore, PRUNE_KINDS } from './engine.store';
import { ENGINE_NAMES, PRUNE_LABELS, countOf, errorSentence, middleEllipsis } from './storage-labels';

/** The Container engine section: usage cards, the folders Pitcrew's tools fill, and the prune workflow. */
@Component({
  selector: 'pc-engine-section',
  imports: [MatIcon, LoadState, CdkMenu, CdkMenuItem, CdkMenuTrigger],
  changeDetection: ChangeDetectionStrategy.OnPush,
  styleUrl: './storage-section.scss',
  template: `
    <section class="section" aria-labelledby="engine-title">
      <header class="section-head">
        <h2 id="engine-title">Container engine</h2>
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
                    @if (c.percent > 0) {
                      <p class="small muted"><span class="num">{{ c.reclaimable }}</span> reclaimable</p>
                      <div class="stack" role="img" [attr.aria-label]="c.reclaimable + ' of ' + c.size + ' reclaimable'">
                        <span class="used" [style.width.%]="100 - c.percent"></span><span class="reclaim" [style.width.%]="c.percent"></span>
                      </div>
                      <p class="legend faint small"><span class="swatch used"></span>In use<span class="swatch reclaim"></span>Reclaimable</p>
                    } @else {
                      <p class="small muted">Nothing to reclaim</p>
                    }
                  </div>
                </article>
              }
            </div>
          }

          @if (r.folders.length > 0) {
            <article class="card" aria-labelledby="folders-title">
              <header class="card-header"><h3 id="folders-title">Folders</h3></header>
              <div class="card-body flush scroll">
                <table class="table folders">
                  <thead>
                    <tr>
                      <th scope="col">Folder</th>
                      <th scope="col" class="num">Size</th>
                      <th scope="col" class="num">Entries</th>
                      <th scope="col"><span class="visually-hidden">Actions</span></th>
                    </tr>
                  </thead>
                  <tbody>
                    @for (f of folders(); track f.folder.path) {
                      <tr>
                        <th scope="row" class="folder-cell">
                          <span class="folder-label">{{ f.folder.label }}</span>
                          <span class="mono path" [title]="f.folder.path">{{ f.shortPath }}</span>
                          @if (!f.folder.exists) {
                            <span class="faint small">This folder does not exist yet.</span>
                          }
                          @if (f.folder.truncated) {
                            <span class="faint small">Truncated: the scan stopped early, so the size and count are lower bounds.</span>
                          }
                        </th>
                        <td class="num nowrap">{{ f.size }}</td>
                        <td class="num nowrap">{{ f.entries }}</td>
                        <td class="row-actions">
                          <button type="button" class="btn icon sm ghost" [attr.aria-label]="'Copy path of ' + f.folder.label" title="Copy path" (click)="copyPath(f.folder.path)">
                            <mat-icon svgIcon="pc:copy" aria-hidden="true" />
                          </button>
                          <button type="button" class="btn icon sm ghost more" [cdkMenuTriggerFor]="folderMenu" [cdkMenuTriggerData]="{ $implicit: f }" [attr.aria-label]="'More actions for ' + f.folder.label">
                            <mat-icon svgIcon="pc:more" aria-hidden="true" />
                          </button>
                        </td>
                      </tr>
                    }
                  </tbody>
                </table>
              </div>
            </article>
          }

          @if (store.usable()) {
            <article class="card" aria-labelledby="prune-title">
              <header class="card-header"><h3 id="prune-title" tabindex="-1">Prune</h3></header>
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

    <ng-template #folderMenu let-f>
      <div class="pc-menu" cdkMenu>
        <button type="button" class="pc-menu-item" cdkMenuItem (cdkMenuItemTriggered)="copyPath(f.folder.path)">Copy path</button>
        <button type="button" class="pc-menu-item" cdkMenuItem (cdkMenuItemTriggered)="copyDetails(f)">Copy details</button>
      </div>
    </ng-template>
  `,
})
export class EngineSection {
  protected readonly store = inject(EngineStore);
  private readonly ports = inject(SHELL_PORTS);
  private readonly toasts = inject(Notify);

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
    (this.report()?.folders ?? []).map((folder) => ({ folder, shortPath: middleEllipsis(folder.path), size: formatBytes(folder.bytes), entries: plural(folder.entries, 'entry', 'entries') })),
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

  protected copyPath(path: string): Promise<void> {
    return this.copy(path, 'Path copied');
  }

  protected copyDetails(f: { folder: { label: string; path: string }; size: string; entries: string }): Promise<void> {
    return this.copy(`${f.folder.label}\n${f.folder.path}\n${f.size}, ${f.entries}`, 'Details copied');
  }

  private async copy(value: string, done: string): Promise<void> {
    try {
      await navigator.clipboard.writeText(value);
      this.toasts.show({ tone: 'success', icon: 'check-circle', title: done, body: null });
    } catch {
      this.toasts.show({ tone: 'warning', icon: 'alert', title: 'Could not copy', body: 'Select the text and press Ctrl/Cmd+C instead.' });
    }
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
    const ok = await confirmAction(this.ports, {
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
