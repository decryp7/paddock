import { ChangeDetectionStrategy, Component, OnInit, inject, input, signal } from '@angular/core';
import { MatIconModule } from '@angular/material/icon';
import { MatTooltipModule } from '@angular/material/tooltip';
import type { Fit, QuantVariant } from '../../core/api.types';
import { ago, bytes, compact } from '../../shared/format';
import { fitLook } from '../../shared/labels';
import { EmptyState } from '../../shared/ui/empty-state';
import { Panel } from '../../shared/ui/panel';
import { ModelsStore } from './models.store';

/** What a fit needs, in one line: `6.1 GB VRAM + 0.9 GB RAM · all layers on the GPU`. */
export function fitDetail(f: Fit): string {
  const parts: string[] = [];
  if (f.needVram > 0) parts.push(`${bytes(f.needVram)} VRAM`);
  if (f.needRam > 0) parts.push(`${bytes(f.needRam)} RAM`);
  const offload = { all: 'all layers on the GPU', partial: 'part of the layers on the GPU', none: 'CPU only' }[f.offload];
  return `${parts.join(' + ') || 'Nothing'} · ${offload}`;
}

/** Hugging Face search (GGUF) and a repo's quantisation variants, each with a "will this fit?" verdict. */
@Component({
  selector: 'pw-discover-tab',
  imports: [MatIconModule, MatTooltipModule, Panel, EmptyState],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <form class="search" role="search" (submit)="$event.preventDefault(); store.search(q.value)">
      <mat-icon svgIcon="pw:search" aria-hidden="true" />
      <label class="sr-only" for="hf-q">Search Hugging Face for GGUF models</label>
      <input #q id="hf-q" class="input" type="search" placeholder="Search GGUF models on Hugging Face — e.g. qwen3, gemma, llama" [value]="store.query()" autocomplete="off" />
      <button type="submit" class="btn primary" [disabled]="store.searching()">Search</button>
    </form>

    <div class="cols">
      <pw-panel class="results" heading="Results" icon="globe" [sub]="store.searched() ? store.results().length + ' repos' : ''" [flush]="true" [loading]="store.searching()" [error]="store.searchError()" [ready]="store.searched()" [skeletonLines]="8" (retry)="store.search(store.query())">
        @if (store.results().length === 0) {
          <pw-empty-state icon="search" heading="No GGUF repos found" text="Try a model family (qwen, llama, gemma) or an author." />
        }
        <ul class="list" aria-label="Search results">
          @for (r of store.results(); track r.repo) {
            <li>
              <button type="button" class="hit" [class.sel]="store.repoName() === r.repo" [attr.aria-pressed]="store.repoName() === r.repo" (click)="store.openRepo(r.repo)">
                <span class="repo">{{ r.repo }}</span>
                <span class="meta num">
                  <span><mat-icon svgIcon="pw:download" aria-hidden="true" />{{ count(r.downloads) }}</span>
                  <span><mat-icon svgIcon="pw:heart" aria-hidden="true" />{{ count(r.likes) }}</span>
                  @if (r.updatedAt) {
                    <span>updated {{ when(r.updatedAt, now()) }}</span>
                  }
                </span>
                <span class="tags">
                  @for (t of r.tags.slice(0, 3); track t) {
                    <span class="tag">{{ t }}</span>
                  }
                </span>
              </button>
            </li>
          }
        </ul>
      </pw-panel>

      <pw-panel class="repo-panel" [heading]="store.repoName() ?? 'Quantisations'" icon="models" [sub]="repoSub()" [loading]="store.repoLoading()" [error]="store.repoError()" [ready]="!store.repoName() || !!store.repo()" [skeletonLines]="8" (retry)="store.openRepo(store.repoName()!)">
        @if (store.repoName()) {
          <button panel-actions type="button" class="btn ghost icon sm" aria-label="Close repo" (click)="store.closeRepo()"><mat-icon svgIcon="pw:close" aria-hidden="true" /></button>
        }
        @if (!store.repoName()) {
          <pw-empty-state icon="models" heading="Pick a repo" text="Its quantisations appear here with a verdict on whether each fits this machine's free RAM and VRAM." />
        }
        @if (store.repo(); as repo) {
          @if (repo.gated) {
            <p class="note" role="note"><mat-icon svgIcon="pw:lock" aria-hidden="true" />This repo is gated: accept its licence on Hugging Face first.</p>
          }
          @if (repo.projectors.length) {
            <label class="proj">
              <input type="checkbox" [checked]="withProjector()" (change)="withProjector.set($any($event.target).checked)" />
              <span>Include the vision projector <span class="mono">{{ repo.projectors[0]!.file }}</span> ({{ size(repo.projectors[0]!.sizeBytes) }}) for image input</span>
            </label>
          }
          <table class="table">
            <thead>
              <tr>
                <th scope="col">Quant</th>
                <th scope="col" class="right">Size</th>
                <th scope="col">Will it fit?</th>
                <th scope="col">Needs</th>
                <th scope="col"><span class="sr-only">Download</span></th>
              </tr>
            </thead>
            <tbody>
              @for (v of repo.variants; track v.file) {
                <tr>
                  <td>
                    <span class="q mono">{{ v.quant ?? v.file }}</span>
                    @if (v.parts > 1) {
                      <span class="faint parts">{{ v.parts }} parts</span>
                    }
                  </td>
                  <td class="right num">{{ size(v.sizeBytes) }}</td>
                  <td><span class="pill dot" [attr.data-tone]="look(v).tone" [matTooltip]="v.fit.message">{{ look(v).label }}</span></td>
                  <td class="needs" [matTooltip]="v.fit.message">{{ detail(v.fit) }}</td>
                  <td class="right">
                    @if (v.downloaded) {
                      <span class="pill" data-tone="muted"><mat-icon svgIcon="pw:check" aria-hidden="true" />In library</span>
                    } @else {
                      <button type="button" class="btn sm" [class.primary]="v.fit.verdict === 'fits'" [disabled]="store.isBusy()('download:' + v.file)" [attr.aria-label]="'Download ' + v.file" (click)="download(v)">
                        <mat-icon svgIcon="pw:download" aria-hidden="true" />Download
                      </button>
                    }
                  </td>
                </tr>
              }
            </tbody>
          </table>
        }
      </pw-panel>
    </div>
  `,
  styles: `
    :host {
      display: grid;
      gap: var(--sp-3);
    }
    .search {
      position: relative;
      display: flex;
      gap: var(--sp-2);
      mat-icon {
        position: absolute;
        left: 11px;
        top: 9px;
        width: 15px;
        height: 15px;
        color: var(--text-3);
      }
      .input {
        flex: 1;
        height: 34px;
        padding-left: 34px;
      }
      .btn {
        height: 34px;
      }
    }
    .cols {
      display: grid;
      grid-template-columns: minmax(0, 5fr) minmax(0, 7fr);
      gap: var(--sp-3);
      align-items: start;
    }
    .list {
      list-style: none;
      margin: var(--sp-2) 0 0;
      padding: 0 var(--sp-2) var(--sp-2);
      max-height: 640px;
      overflow: auto;
    }
    .hit {
      display: flex;
      flex-direction: column;
      gap: 4px;
      width: 100%;
      padding: 10px var(--sp-2);
      font: inherit;
      color: inherit;
      text-align: left;
      background: none;
      border: none;
      border-radius: var(--r-md);
      cursor: pointer;
      &:hover {
        background: var(--bg-hover);
      }
      &.sel {
        background: var(--accent-soft);
        box-shadow: inset 2px 0 0 var(--accent);
      }
    }
    .repo {
      font-weight: var(--fw-medium);
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
    }
    .meta {
      display: flex;
      gap: var(--sp-3);
      font-size: var(--fs-xs);
      color: var(--text-3);
      span {
        display: inline-flex;
        align-items: center;
        gap: 3px;
      }
      mat-icon {
        width: 11px;
        height: 11px;
      }
    }
    .tags {
      display: flex;
      gap: 4px;
    }
    .note {
      display: flex;
      align-items: center;
      gap: var(--sp-2);
      margin-bottom: var(--sp-3);
      padding: var(--sp-2) var(--sp-3);
      font-size: var(--fs-sm);
      background: var(--warn-soft);
      border-radius: var(--r-md);
      mat-icon {
        color: var(--warn);
      }
    }
    .proj {
      display: flex;
      align-items: center;
      gap: var(--sp-2);
      margin-bottom: var(--sp-3);
      font-size: var(--fs-sm);
      color: var(--text-2);
      input {
        accent-color: var(--accent);
      }
    }
    .table {
      margin: 0 calc(-1 * var(--sp-4));
      width: calc(100% + 2 * var(--sp-4));
      th:first-child,
      td:first-child {
        padding-left: var(--sp-4);
      }
      th:last-child,
      td:last-child {
        padding-right: var(--sp-4);
      }
    }
    .q {
      font-weight: var(--fw-medium);
    }
    .parts {
      margin-left: var(--sp-2);
      font-size: var(--fs-xs);
    }
    .needs {
      font-size: var(--fs-sm);
      color: var(--text-2);
    }
    .pill mat-icon {
      width: 11px;
      height: 11px;
    }
  `,
})
export class DiscoverTab implements OnInit {
  readonly now = input.required<number>();
  protected readonly store = inject(ModelsStore);
  protected readonly withProjector = signal(true);
  protected readonly size = bytes;
  protected readonly count = compact;
  protected readonly when = ago;
  protected readonly detail = fitDetail;
  protected readonly look = (v: QuantVariant) => fitLook(v.fit.verdict);

  ngOnInit(): void {
    if (!this.store.searched() && !this.store.searching()) void this.store.search(this.store.query());
  }

  protected repoSub(): string {
    const r = this.store.repo();
    if (!r) return '';
    return [`${r.variants.length} files`, r.license ? `licence ${r.license}` : ''].filter(Boolean).join(' · ');
  }

  protected download(v: QuantVariant): void {
    const r = this.store.repo();
    const projector = r && r.projectors.length && this.withProjector() ? r.projectors[0]!.file : null;
    void this.store.download(v, projector);
  }
}
