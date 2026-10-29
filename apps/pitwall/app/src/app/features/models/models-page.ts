import { ChangeDetectionStrategy, Component, OnInit, computed, effect, inject, signal, untracked } from '@angular/core';
import { MatIconModule } from '@angular/material/icon';
import { MatTooltipModule } from '@angular/material/tooltip';
import { LiveStore } from '../../core/live.store';
import { downloadActive } from '../../shared/labels';
import { injectNow } from '../../shared/now';
import { Panel } from '../../shared/ui/panel';
import { UpdateBanner } from '../updates/update-banner';
import { DiscoverTab } from './discover-tab';
import { DownloadsList } from './downloads-list';
import { LibraryTab } from './library-tab';
import { PresetsTab } from './presets-tab';
import { ModelsStore } from './models.store';

export type ModelsTab = 'library' | 'discover' | 'presets' | 'downloads';
const TABS: { id: ModelsTab; label: string }[] = [
  { id: 'library', label: 'Library' },
  { id: 'discover', label: 'Discover' },
  { id: 'presets', label: 'Presets' },
  { id: 'downloads', label: 'Downloads' },
];

/** The next tab for an arrow/Home/End key in the tab list, or null for other keys. */
export function nextTab(current: ModelsTab, key: string): ModelsTab | null {
  const i = TABS.findIndex((t) => t.id === current);
  const n = TABS.length;
  const to = key === 'ArrowRight' ? (i + 1) % n : key === 'ArrowLeft' ? (i - 1 + n) % n : key === 'Home' ? 0 : key === 'End' ? n - 1 : -1;
  return to < 0 ? null : TABS[to]!.id;
}

/** Models: the local library, Hugging Face discovery with fit verdicts, and downloads. */
@Component({
  selector: 'pw-models-page',
  imports: [MatIconModule, MatTooltipModule, Panel, LibraryTab, DiscoverTab, PresetsTab, DownloadsList, UpdateBanner],
  providers: [ModelsStore],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="page">
      <header class="page-head">
        <div>
          <h1>Models</h1>
          <p class="sub">Models on this machine, and new ones from Hugging Face with a verdict on what fits.</p>
        </div>
        <div class="actions">
          <button type="button" class="btn ghost icon" aria-label="Refresh" matTooltip="Refresh" (click)="store.load()" [disabled]="store.loading()"><mat-icon svgIcon="pw:refresh" aria-hidden="true" /></button>
        </div>
      </header>

      <pw-update-banner class="updates" />

      <div class="tabs" role="tablist" aria-label="Models">
        @for (t of tabs; track t.id) {
          <button
            type="button"
            role="tab"
            [id]="'tab-' + t.id"
            [attr.aria-selected]="tab() === t.id"
            [attr.aria-controls]="'panel-' + t.id"
            [attr.tabindex]="tab() === t.id ? 0 : -1"
            (click)="tab.set(t.id)"
            (keydown)="onKey($event)"
          >
            {{ t.label }}
            @if (t.id === 'library' && store.loaded()) {
              <span class="count num">{{ store.library().length }}</span>
            }
            @if (t.id === 'downloads' && active() > 0) {
              <span class="count num">{{ active() }}</span>
            }
          </button>
        }
      </div>

      <div role="tabpanel" [id]="'panel-' + tab()" [attr.aria-labelledby]="'tab-' + tab()">
        @switch (tab()) {
          @case ('library') {
            <pw-library-tab [now]="now()" (discover)="tab.set('discover')" />
          }
          @case ('discover') {
            <pw-discover-tab [now]="now()" />
          }
          @case ('presets') {
            <pw-presets-tab />
          }
          @case ('downloads') {
            <pw-panel heading="Downloads" icon="download" [sub]="downloadsSub()">
              <pw-downloads-list [now]="now()" />
            </pw-panel>
          }
        }
      </div>
    </div>
  `,
  styles: `
    .tabs {
      display: flex;
      gap: var(--pd-space-1);
      margin-bottom: var(--pd-space-4);
      border-bottom: 1px solid var(--pd-hairline);
      button {
        position: relative;
        display: inline-flex;
        align-items: center;
        gap: 6px;
        height: 34px;
        padding: 0 var(--pd-space-3);
        font: inherit;
        font-weight: var(--pd-weight-medium);
        color: var(--pd-text-muted);
        background: none;
        border: none;
        cursor: default;
        &:hover {
          color: var(--pd-text);
        }
        &[aria-selected='true'] {
          color: var(--pd-text);
          &::after {
            content: '';
            position: absolute;
            left: var(--pd-space-2);
            right: var(--pd-space-2);
            bottom: -1px;
            height: 2px;
            border-radius: 2px;
            background: var(--pd-accent);
          }
        }
      }
    }
  `,
})
export class ModelsPage implements OnInit {
  protected readonly store = inject(ModelsStore);
  private readonly live = inject(LiveStore);
  protected readonly now = injectNow();
  protected readonly tabs = TABS;
  readonly tab = signal<ModelsTab>('library');
  protected readonly active = computed(() => this.live.downloads().filter((d) => downloadActive(d.state)).length);
  protected readonly downloadsSub = computed(() => {
    const n = this.active();
    return n ? `${n} running` : '';
  });

  constructor() {
    // A finished model download lands in the library: refresh it once per download.
    const seen = new Set<string>();
    effect(() => {
      const done = this.live.downloads().filter((d) => d.kind === 'model' && d.state === 'done' && !seen.has(d.id));
      if (!done.length) return;
      done.forEach((d) => seen.add(d.id));
      untracked(() => void this.store.load());
    });
  }

  ngOnInit(): void {
    void this.store.load();
    void this.store.loadBuilds();
  }

  protected onKey(e: KeyboardEvent): void {
    const to = nextTab(this.tab(), e.key);
    if (!to) return;
    e.preventDefault();
    this.tab.set(to);
    (e.currentTarget as HTMLElement).parentElement?.querySelector<HTMLElement>(`#tab-${to}`)?.focus();
  }
}
