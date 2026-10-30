import { ChangeDetectionStrategy, Component, OnInit, computed, inject, input, signal } from '@angular/core';
import { MatButton, MatIconButton } from '@angular/material/button';
import { MatChipListbox, MatChipOption } from '@angular/material/chips';
import { MatFormField, MatPrefix } from '@angular/material/form-field';
import { MatIconModule } from '@angular/material/icon';
import { MatInput } from '@angular/material/input';
import { MatSlideToggleModule } from '@angular/material/slide-toggle';
import { MatTooltipModule } from '@angular/material/tooltip';
import type { Fit, QuantVariant, Recommendation } from '../../core/api.types';
import { ago, bytes, compact, concurrencyText } from '../../shared/format';
import { fitLook } from '../../shared/labels';
import { Panel } from '../../shared/ui/panel';
import { contextText } from './library-tab';
import { ModelsStore } from './models.store';
import { SuEmptyState } from '@decryp7/paddock-ui';

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
  imports: [MatButton, MatIconButton, MatChipListbox, MatChipOption, MatFormField, MatPrefix, MatInput, MatIconModule, MatTooltipModule, MatSlideToggleModule, Panel, SuEmptyState],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <pw-panel class="recs" heading="Recommended for this machine" icon="sparkles" [sub]="recsSub()" [flush]="true" [loading]="store.packsLoading()" [error]="store.packsError()" [ready]="!!store.packs()" [skeletonLines]="3" (retry)="store.loadPacks()">
      <mat-chip-listbox class="chips" aria-label="Filter recommendations by tag" hideSingleSelectionIndicator [value]="store.packTag() ?? ''" (change)="store.loadPacks($event.value || null)">
        <mat-chip-option value="">All</mat-chip-option>
        @for (t of store.packTags(); track t) {
          <mat-chip-option [value]="t">{{ t }}</mat-chip-option>
        }
      </mat-chip-listbox>
      @if (store.packs(); as p) {
        @if (p.recommendations.length === 0) {
          <pd-empty-state heading="Nothing recommended" description="No catalog model with this tag fits this machine. Try another tag."><mat-icon slot="icon" svgIcon="pw:models" aria-hidden="true" /></pd-empty-state>
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
                    <button type="button" matIconButton class="reveal" [disabled]="store.isBusy()('rec:' + r.id) || r.fit.verdict === 'wontFit'" [attr.aria-label]="'Download ' + r.name" matTooltip="Download" (click)="store.downloadRecommendation(r)">
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

    @if (store.packs(); as p) {
      @if (p.problems.length || p.warnings.length) {
        <section class="pack-notice callout" data-tone="warn" role="status" aria-label="Catalog pack problems">
          <mat-icon svgIcon="pw:alert" aria-hidden="true" />
          <div>
            <p class="head">
              @if (p.problems.length) {
                {{ p.problems.length }} catalog {{ p.problems.length === 1 ? 'file was' : 'files were' }} skipped.
              } @else {
                Catalog files loaded with notes.
              }
              Fix the file in your packs folder and reopen this page.
            </p>
            <ul>
              @for (n of p.problems; track n.file + n.message) {
                <li><span class="mono">{{ n.file }}</span> {{ n.message }}</li>
              }
              @for (n of p.warnings; track n.file + n.message) {
                <li><span class="mono">{{ n.file }}</span> {{ n.message }}</li>
              }
            </ul>
          </div>
        </section>
      }
    }

    <form class="search" role="search" (submit)="$event.preventDefault(); store.search(q.value)">
      <mat-form-field class="field">
        <mat-icon matPrefix svgIcon="pw:search" aria-hidden="true" />
        <input #q matInput id="hf-q" type="search" aria-label="Search Hugging Face for GGUF models" placeholder="Search GGUF models on Hugging Face — e.g. qwen3, gemma, llama" [value]="store.query()" autocomplete="off" (input)="typed.set(q.value.trim() !== '')" />
      </mat-form-field>
      <button type="submit" [matButton]="hasText() ? 'filled' : 'outlined'" [disabled]="store.searching()">Search</button>
    </form>

    <div class="cols">
      <pw-panel class="results" heading="Results" icon="globe" [sub]="store.searched() ? store.results().length + ' repos' : ''" [flush]="true" [loading]="store.searching()" [error]="store.searchError()" [ready]="store.searched()" [skeletonLines]="8" (retry)="store.search(store.query())">
        @if (store.results().length === 0) {
          <pd-empty-state heading="No GGUF repos found" description="Try a model family (qwen, llama, gemma) or an author."><mat-icon slot="icon" svgIcon="pw:search" aria-hidden="true" /></pd-empty-state>
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
          <button panel-actions type="button" matIconButton aria-label="Close repo" matTooltip="Close repo" (click)="store.closeRepo()"><mat-icon svgIcon="pw:close" aria-hidden="true" /></button>
        }
        @if (!store.repoName()) {
          <pd-empty-state heading="Pick a repo" description="Its quantisations appear here with a verdict on whether each fits this machine's free RAM and VRAM."><mat-icon slot="icon" svgIcon="pw:models" aria-hidden="true" /></pd-empty-state>
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
                        <button type="button" matIconButton class="reveal" [disabled]="store.isBusy()('download:' + v.file)" [attr.aria-label]="'Download ' + v.file" matTooltip="Download" (click)="download(v)">
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
    .pack-notice {
      p {
        margin: 0;
      }
      .head {
        font-weight: var(--pd-weight-semibold);
      }
      ul {
        margin: var(--pd-space-1) 0 0;
        padding: 0;
        list-style: none;
        color: var(--pd-text-muted);
      }
      .mono {
        font-size: var(--pd-text-label);
      }
    }
    :host {
      display: grid;
      gap: var(--pd-space-3);
    }
    .search {
      display: flex;
      align-items: center;
      gap: var(--pd-space-2);
      .field {
        flex: 1;
        min-width: 0;
      }
      mat-icon[matPrefix] {
        margin: 0 var(--pd-space-2) 0 var(--pd-space-3);
        color: var(--pd-text-subtle);
      }
    }
    .chips {
      display: block;
      padding: 0 var(--pd-space-4) var(--pd-space-2);
    }
    /* A recommendation is one line: name, what it is, the verdict, and a download that appears on hover. */
    .rec-list {
      margin: 0;
      padding: 0 0 var(--pd-space-1);
      list-style: none;
    }
    .rec {
      display: grid;
      grid-template-columns: minmax(160px, 1.2fr) minmax(0, 1fr) 80px 96px;
      align-items: center;
      gap: var(--pd-space-3);
      min-height: 40px;
      padding: 0 var(--pd-space-4);
      border-top: 1px solid var(--pd-hairline);
      &:hover {
        background: var(--pd-hover);
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
      font-size: var(--pd-text-label);
    }
    .rec-name {
      font-weight: var(--pd-weight-semibold);
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
    }
    .rec-meta {
      font-size: var(--pd-text-label);
      color: var(--pd-text-muted);
    }
    .cols {
      display: grid;
      grid-template-columns: minmax(0, 5fr) minmax(0, 7fr);
      gap: var(--pd-space-3);
      align-items: start;
    }
    .list {
      list-style: none;
      margin: var(--pd-space-2) 0 0;
      padding: 0 var(--pd-space-2) var(--pd-space-2);
      max-height: 640px;
      overflow: auto;
    }
    .hit {
      display: flex;
      flex-direction: column;
      gap: 4px;
      width: 100%;
      padding: 10px var(--pd-space-2);
      font: inherit;
      color: inherit;
      text-align: left;
      background: none;
      border: none;
      border-radius: var(--pd-radius-control);
      cursor: default;
      &:hover {
        background: var(--pd-hover);
      }
      &.sel {
        background: var(--pd-accent-tint);
        box-shadow: inset 2px 0 0 var(--pd-accent);
      }
    }
    .repo {
      font-weight: var(--pd-weight-medium);
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
    }
    .meta {
      display: flex;
      gap: var(--pd-space-3);
      font-size: var(--pd-text-label);
      color: var(--pd-text-muted);
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
      margin-bottom: var(--pd-space-3);
    }
    .proj {
      display: flex;
      align-items: center;
      gap: var(--pd-space-2);
      margin-bottom: var(--pd-space-3);
      font-size: var(--pd-text-label);
      color: var(--pd-text-muted);
    }
    .table {
      margin: 0 calc(-1 * var(--pd-space-4));
      width: calc(100% + 2 * var(--pd-space-4));
      th:first-child,
      td:first-child {
        padding-left: var(--pd-space-4);
      }
      th:last-child,
      td:last-child {
        padding-right: var(--pd-space-4);
      }
    }
    .q {
      font-weight: var(--pd-weight-medium);
    }
    .parts {
      margin-left: var(--pd-space-2);
      font-size: var(--pd-text-label);
    }
    .table td.act {
      width: 1%;
      text-align: right;
    }
    .needs {
      max-width: 340px;
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
      font-size: var(--pd-text-label);
      color: var(--pd-text-muted);
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
