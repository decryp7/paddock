import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';
import { Router } from '@angular/router';
import { SuEmptyState } from '@decryp7/paddock-ui';
import type { DiscoveredFeed, ImportFormat, SourceKind } from '../../core/api.types';
import { GROUP_LABEL, GROUP_ORDER } from '../../shared/labels';
import { KindIcon } from '../../shared/ui/kind-icon';
import { Panel } from '../../shared/ui/panel';
import { ToastStore } from '../../state/toast.store';
import { EditorStore } from './editor.store';

type Way = 'presets' | 'kinds' | 'feed' | 'import';

/** Step 1: how to start: a ready-made preset, a kind of source, a page that advertises feeds, or a list to import. */
@Component({
  selector: 'sp-source-step',
  imports: [SuEmptyState, KindIcon, Panel],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="tabs" role="tablist" aria-label="How to start">
      @for (w of ways; track w.id) {
        <button type="button" role="tab" class="tab" [class.on]="way() === w.id" [attr.aria-selected]="way() === w.id" (click)="way.set(w.id)">{{ w.label }}</button>
      }
    </div>

    @switch (way()) {
      @case ('presets') {
        <sp-panel [state]="store.presets()" label="the presets" (retry)="store.loadCatalog()">
          <div class="grid">
            @for (p of store.presets().value ?? []; track p.id) {
              <button type="button" class="card" (click)="store.pickPreset(p)">
                <sp-kind-icon [kind]="p.draft.source.kind" />
                <span class="t">{{ p.name }}</span>
                <span class="d">{{ p.description }}</span>
                <span class="chip">{{ p.category }}</span>
              </button>
            }
          </div>
        </sp-panel>
      }
      @case ('kinds') {
        <sp-panel [state]="store.kinds()" label="the source kinds" (retry)="store.loadCatalog()">
          @for (g of groups(); track g.id) {
            <h3 class="group">{{ g.label }}</h3>
            <div class="grid">
              @for (k of g.kinds; track k.id) {
                <button type="button" class="card" (click)="store.pickKind(k)">
                  <sp-kind-icon [kind]="k.id" />
                  <span class="t">{{ k.label }}</span>
                  <span class="d">{{ k.description }}</span>
                </button>
              }
            </div>
          }
        </sp-panel>
      }
      @case ('feed') {
        <form class="stack" (submit)="find($event, url.value)">
          <div class="field">
            <label for="feed-url">Page address</label>
            <div class="inline">
              <input #url id="feed-url" class="input" type="url" placeholder="https://example.org/blog" />
              <button type="submit" class="btn primary" [disabled]="store.discovering()">{{ store.discovering() ? 'Looking…' : 'Find feeds' }}</button>
            </div>
            <span class="help">Spotter reads the page and lists the release and blog feeds it advertises.</span>
          </div>
        </form>
        @if (store.discoverError(); as err) {
          <p class="error" role="alert">{{ err }}</p>
        }
        @if (store.feeds(); as feeds) {
          @if (feeds.length) {
            <ul class="feeds" aria-label="Feeds found">
              @for (f of feeds; track f.url) {
                <li>
                  <div class="truncate"><strong>{{ f.title }}</strong><div class="help truncate">{{ f.url }}</div></div>
                  <button type="button" class="btn" (click)="pickFeed(f)">Watch this feed</button>
                </li>
              }
            </ul>
          } @else {
            <pd-empty-state heading="No feeds found" description="That page does not advertise a feed. Try the Web page source kind instead." />
          }
        }
      }
      @case ('import') {
        <div class="stack">
          <div class="field">
            <label for="import-format">Format</label>
            <select id="import-format" class="input narrow" [value]="format()" (change)="format.set(asFormat($event))">
              <option value="winget">winget export (JSON)</option>
              <option value="brewfile">Brewfile</option>
              <option value="opml">OPML (feed list)</option>
            </select>
          </div>
          <div class="field">
            <label for="import-text">Paste the list</label>
            <textarea #text id="import-text" class="input" rows="8" spellcheck="false"></textarea>
          </div>
          <div class="actions">
            <button type="button" class="btn primary" (click)="store.importList(format(), text.value)" [disabled]="store.importing() || !text.value.trim()">Read the list</button>
          </div>
          @if (store.importError(); as err) {
            <p class="error" role="alert">{{ err }}</p>
          }
          @if (store.imported(); as r) {
            <div class="surface">
              <p><strong>{{ r.drafts.length }}</strong> watch{{ r.drafts.length === 1 ? '' : 'es' }} ready to add@if (r.skipped.length) { , {{ r.skipped.length }} line{{ r.skipped.length === 1 ? '' : 's' }} skipped }.</p>
              <ul class="plain">
                @for (d of r.drafts; track d.name) {
                  <li>{{ d.name }}</li>
                }
              </ul>
              @if (r.skipped.length) {
                <details><summary>Skipped lines</summary><ul class="plain">@for (s of r.skipped; track s) { <li class="mono">{{ s }}</li> }</ul></details>
              }
              <div class="actions"><button type="button" class="btn primary" (click)="addAll()" [disabled]="!r.drafts.length">Add {{ r.drafts.length }} watch{{ r.drafts.length === 1 ? '' : 'es' }}</button></div>
            </div>
          }
        </div>
      }
    }
  `,
  styles: `
    :host {
      display: grid;
      gap: var(--pd-space-4);
    }
    .tabs {
      display: flex;
      gap: var(--pd-space-1);
      border-bottom: 1px solid var(--pd-hairline);
    }
    .tab {
      padding: var(--pd-space-2) var(--pd-space-4);
      border: 0;
      border-bottom: 2px solid transparent;
      background: transparent;
      color: var(--pd-text-muted);
      font: inherit;
      font-weight: var(--pd-weight-medium);
    }
    .tab.on {
      border-color: var(--pd-accent);
      color: var(--pd-text);
    }
    .group {
      margin: var(--pd-space-4) 0 var(--pd-space-2);
      color: var(--pd-text-muted);
      font-size: var(--pd-text-label);
      font-weight: var(--pd-weight-medium);
    }
    .grid {
      display: grid;
      grid-template-columns: repeat(auto-fill, minmax(220px, 1fr));
      gap: var(--pd-card-gap);
    }
    .card {
      display: grid;
      justify-items: start;
      align-content: start;
      gap: var(--pd-space-2);
      padding: var(--pd-card-padding);
      border: 1px solid var(--pd-hairline);
      border-radius: var(--pd-radius-card);
      background: var(--pd-surface-1);
      color: var(--pd-text);
      font: inherit;
      text-align: left;
    }
    .card:hover {
      border-color: var(--pd-accent);
      background: var(--pd-hover);
    }
    .t {
      font-weight: var(--pd-weight-semibold);
    }
    .d,
    .help {
      color: var(--pd-text-muted);
      font-size: var(--pd-text-caption);
    }
    .stack {
      display: grid;
      gap: var(--pd-space-4);
      max-width: 640px;
    }
    .inline {
      display: flex;
      gap: var(--pd-space-2);
    }
    .narrow {
      max-width: 280px;
    }
    .error {
      color: var(--pd-danger);
    }
    .feeds,
    .plain {
      margin: 0;
      padding: 0;
      list-style: none;
    }
    .feeds {
      display: grid;
      max-width: 640px;
      border: 1px solid var(--pd-hairline);
      border-radius: var(--pd-radius-card);
    }
    .feeds li {
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: var(--pd-space-3);
      padding: var(--pd-space-3) var(--pd-space-4);
      border-bottom: 1px solid var(--pd-hairline);
    }
    .feeds li:last-child {
      border-bottom: 0;
    }
  `,
})
export class SourceStep {
  protected readonly store = inject(EditorStore);
  private readonly router = inject(Router);
  private readonly toast = inject(ToastStore);

  protected readonly ways: { id: Way; label: string }[] = [
    { id: 'presets', label: 'Popular software' },
    { id: 'kinds', label: 'Source kinds' },
    { id: 'feed', label: 'Find a feed' },
    { id: 'import', label: 'Import a list' },
  ];
  protected readonly way = signal<Way>('presets');
  protected readonly format = signal<ImportFormat>('winget');
  protected readonly groups = computed(() =>
    GROUP_ORDER.map((id) => ({ id, label: GROUP_LABEL[id], kinds: (this.store.kinds().value ?? []).filter((k: SourceKind) => k.group === id) })).filter((g) => g.kinds.length),
  );

  protected find(e: Event, url: string): void {
    e.preventDefault();
    if (url.trim()) void this.store.discover(url);
  }

  protected pickFeed(f: DiscoveredFeed): void {
    this.store.pickFeed(f);
  }

  protected asFormat(e: Event): ImportFormat {
    return (e.target as HTMLSelectElement).value as ImportFormat;
  }

  protected async addAll(): Promise<void> {
    const { saved, failed } = await this.store.saveImported();
    if (saved) this.toast.success(saved === 1 ? 'Added 1 watch' : `Added ${saved} watches`);
    if (failed.length) this.toast.error(`${failed.length} could not be added`, failed.join('\n'));
    if (!failed.length) await this.router.navigate(['/watches']);
  }

}
