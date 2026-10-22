import { ChangeDetectionStrategy, Component, OnInit, computed, inject, input, signal } from '@angular/core';
import { MatIconModule } from '@angular/material/icon';
import { MatSlideToggleModule } from '@angular/material/slide-toggle';
import { MatTooltipModule } from '@angular/material/tooltip';
import type { Fit, QuantVariant, Recommendation } from '../../core/api.types';
import { ago, bytes, compact, concurrencyText } from '../../shared/format';
import { fitLook } from '../../shared/labels';
import { EmptyState } from '../../shared/ui/empty-state';
import { Panel } from '../../shared/ui/panel';
import { contextText } from './library-tab';
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
  imports: [MatIconModule, MatTooltipModule, MatSlideToggleModule, Panel, EmptyState],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <pw-panel class="recs" heading="Recommended for this machine" icon="sparkles" [sub]="recsSub()" [flush]="true" [loading]="store.packsLoading()" [error]="store.packsError()" [ready]="!!store.packs()" [skeletonLines]="3" (retry)="store.loadPacks()">
      <div class="chips" role="group" aria-label="Filter recommendations by tag">
        <button type="button" class="chip" [attr.aria-pressed]="store.packTag() === null" (click)="store.loadPacks(null)">All</button>
        @for (t of store.packTags(); track t) {
          <button type="button" class="chip" [attr.aria-pressed]="store.packTag() === t" (click)="store.loadPacks(t)">{{ t }}</button>
        }
      </div>
      @if (store.packs(); as p) {
        @if (p.recommendations.length === 0) {
          <pw-empty-state icon="models" heading="Nothing recommended" text="No catalog model with this tag fits this machine. Try another tag." />
        } @else {
          <ul class="rec-list" aria-label="Recommended models">
            @for (r of p.recommendations; track r.id) {
              <li class="rec row">
                <span class="rec-name" [matTooltip]="r.repo" matTooltipShowDelay="600">{{ r.name }}</span>
                <span class="rec-meta num">{{ r.quant ?? 'GGUF' }} · {{ size(r.sizeBytes) }} · {{ ctxText(r.contextTokens) }}</span>
                <span class="status" [attr.data-tone]="recLook(r).tone" [matTooltip]="r.fit.message">{{ recLook(r).label }}</span>
                <span class="rec-act">
                  @if (isLocal(r)) {
                    <span class="faint in-lib">In library</span>
                  } @else {
                    <button type="button" class="btn ghost icon sm reveal" [disabled]="store.isBusy()('rec:' + r.id) || r.fit.verdict === 'wontFit'" [attr.aria-label]="'Download ' + r.name" matTooltip="Download" (click)="store.downloadRecommendation(r)">
                      <mat-icon svgIcon="pw:download" aria-hidden="true" />
                    </button>
                  }
                </span>
              </li>
            }
          </ul>
        }
      }
    </pw-panel>

    <form class="search" role="search" (submit)="$event.preventDefault(); store.search(q.value)">
      <mat-icon svgIcon="pw:search" aria-hidden="true" />
      <label class="sr-only" for="hf-q">Search Hugging Face for GGUF models</label>
      <input #q id="hf-q" class="input" type="search" placeholder="Search GGUF models on Hugging Face — e.g. qwen3, gemma, llama" [value]="store.query()" autocomplete="off" (input)="typed.set(q.value.trim() !== '')" />
      <button type="submit" class="btn" [class.primary]="hasText()" [disabled]="store.searching()">Search</button>
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
            <p class="callout" data-tone="warn" role="note"><mat-icon svgIcon="pw:lock" aria-hidden="true" />This repo is gated: accept its licence on Hugging Face first.</p>
          }
          @if (repo.projectors.length) {
            <div class="proj">
              <mat-slide-toggle hideIcon aria-labelledby="lbl-proj" [checked]="withProjector()" (change)="withProjector.set($event.checked)" />
              <span id="lbl-proj">Include the vision projector <span class="mono">{{ repo.projectors[0]!.file }}</span> ({{ size(repo.projectors[0]!.sizeBytes) }}) for image input</span>
            </div>
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
                  <td>
                    <span class="status" [attr.data-tone]="look(v).tone" [matTooltip]="v.fit.message">{{ look(v).label }}</span>
                    @if (v.fit.verdict !== 'wontFit' && v.fit.verdict !== 'unknown') {
                      <span class="faint parts">{{ concurrency(v.maxSlots) }}</span>
                    }
                  </td>
                  <td class="needs" [matTooltip]="v.fit.message">{{ detail(v.fit) }}</td>
                  <td class="act">
                    @if (v.downloaded) {
                      <span class="faint in-lib">In library</span>
                    } @else {
                      <span class="row-actions">
                        <button type="button" class="btn ghost icon sm reveal" [disabled]="store.isBusy()('download:' + v.file)" [attr.aria-label]="'Download ' + v.file" matTooltip="Download" (click)="download(v)">
                          <mat-icon svgIcon="pw:download" aria-hidden="true" />
                        </button>
                      </span>
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
    .chips {
      display: flex;
      flex-wrap: wrap;
      gap: var(--sp-1);
      padding: 0 var(--sp-4) var(--sp-2);
    }
    .chip {
      height: 24px;
      padding: 0 var(--sp-3);
      font: inherit;
      font-size: var(--fs-sm);
      color: var(--text-2);
      background: none;
      border: 1px solid var(--border-strong);
      border-radius: var(--r-full);
      cursor: default;
      &:hover {
        color: var(--text);
        background: var(--bg-hover);
      }
      &[aria-pressed='true'] {
        color: var(--accent);
        background: var(--accent-soft);
        border-color: var(--accent);
      }
    }
    /* A recommendation is one line: name, what it is, the verdict, and a download that appears on hover. */
    .rec-list {
      margin: 0;
      padding: 0 0 var(--sp-1);
      list-style: none;
    }
    .rec {
      display: grid;
      grid-template-columns: minmax(160px, 1.2fr) minmax(0, 1fr) 80px 96px;
      align-items: center;
      gap: var(--sp-3);
      min-height: 40px;
      padding: 0 var(--sp-4);
      border-top: 1px solid var(--border-soft);
      &:hover {
        background: var(--bg-hover);
      }
    }
    .rec .reveal {
      opacity: 0;
    }
    .rec:hover .reveal,
    .rec:focus-within .reveal,
    .rec .reveal:focus-visible {
      opacity: 1;
    }
    .rec-act {
      justify-self: end;
    }
    .in-lib {
      font-size: var(--fs-sm);
    }
    .rec-name {
      font-weight: var(--fw-semibold);
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
    }
    .rec-meta {
      font-size: var(--fs-sm);
      color: var(--text-2);
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
      cursor: default;
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
      font-size: var(--fs-sm);
      color: var(--text-2);
      span {
        display: inline-flex;
        align-items: center;
        gap: 3px;
      }
      mat-icon {
        width: 12px;
        height: 12px;
      }
    }
    .callout {
      margin-bottom: var(--sp-3);
    }
    .proj {
      display: flex;
      align-items: center;
      gap: var(--sp-2);
      margin-bottom: var(--sp-3);
      font-size: var(--fs-sm);
      color: var(--text-2);
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
      font-size: var(--fs-sm);
    }
    .table td.act {
      width: 1%;
      text-align: right;
    }
    .needs {
      font-size: var(--fs-sm);
      color: var(--text-2);
    }
  `,
})
export class DiscoverTab implements OnInit {
  readonly now = input.required<number>();
  protected readonly store = inject(ModelsStore);
  protected readonly withProjector = signal(true);
  /** The search field has text: only then is Search the page's primary action. */
  protected readonly typed = signal(false);
  protected readonly hasText = computed(() => this.typed() || this.store.query().trim() !== '');
  protected readonly size = bytes;
  protected readonly count = compact;
  protected readonly when = ago;
  protected readonly detail = fitDetail;
  protected readonly concurrency = concurrencyText;
  protected readonly look = (v: QuantVariant) => fitLook(v.fit.verdict);

  ngOnInit(): void {
    if (!this.store.packs() && !this.store.packsLoading()) void this.store.loadPacks();
    if (!this.store.searched() && !this.store.searching()) void this.store.search(this.store.query());
  }

  protected readonly ctxText = contextText;
  protected readonly recLook = (r: Recommendation) => fitLook(r.fit.verdict);

  protected recsSub(): string {
    const p = this.store.packs();
    return p ? `${p.recommendations.length} models` : '';
  }

  /** Whether the recommended file is already in the library. */
  protected isLocal(r: Recommendation): boolean {
    return this.store.library().some((m) => m.repo === r.repo && m.file === r.file);
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
