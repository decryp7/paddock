import { ChangeDetectionStrategy, Component, OnInit, computed, effect, inject, signal, untracked } from '@angular/core';
import { MatIconButton } from '@angular/material/button';
import { MatIconModule } from '@angular/material/icon';
import { MatTabLink, MatTabNav, MatTabNavPanel } from '@angular/material/tabs';
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

/** Models: the local library, Hugging Face discovery with fit verdicts, and downloads. */
@Component({
  selector: 'pw-models-page',
  imports: [MatIconButton, MatTabNav, MatTabLink, MatTabNavPanel, MatIconModule, MatTooltipModule, Panel, LibraryTab, DiscoverTab, PresetsTab, DownloadsList, UpdateBanner],
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
          <button type="button" matIconButton aria-label="Refresh" matTooltip="Refresh" (click)="store.load()" [disabled]="store.loading()"><mat-icon svgIcon="pw:refresh" aria-hidden="true" /></button>
        </div>
      </header>

      <pw-update-banner class="updates" />

      <nav mat-tab-nav-bar class="tabs" aria-label="Models" mat-stretch-tabs="false" mat-align-tabs="start" [tabPanel]="tabPanel">
        @for (t of tabs; track t.id) {
          <button type="button" mat-tab-link [id]="'tab-' + t.id" [active]="tab() === t.id" (click)="tab.set(t.id)">
            {{ t.label }}
            @if (t.id === 'library' && store.loaded()) {
              <span class="count num">{{ store.library().length }}</span>
            }
            @if (t.id === 'downloads' && active() > 0) {
              <span class="count num">{{ active() }}</span>
            }
          </button>
        }
      </nav>

      <mat-tab-nav-panel #tabPanel>
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
      </mat-tab-nav-panel>
    </div>
  `,
  styles: `
    .tabs {
      margin-bottom: var(--pd-space-4);
    }
    .count {
      margin-inline-start: 6px;
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
}
