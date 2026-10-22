import { DialogRef } from '@angular/cdk/dialog';
import { ChangeDetectionStrategy, Component, DestroyRef, computed, inject, signal } from '@angular/core';
import { Router } from '@angular/router';
import type { AppGroup } from '../core/api.types';
import { BACKEND } from '../core/backend';
import { PlatformService } from '../core/platform';
import { formatBytes, formatPercent } from '../shared/format';
import { Icon } from '../shared/icon';
import type { IconName } from '../shared/icons';
import { PROFILE_ICON } from '../shared/labels';
import { DiagnosisStore } from '../state/diagnosis.store';
import { RulesStore } from '../state/rules.store';
import { NAV } from './nav';

export interface PaletteItem {
  id: string;
  section: 'Go to' | 'Actions' | 'Apps';
  label: string;
  hint: string;
  icon: IconName;
  keywords: string;
  run: () => void;
}

/** Ranks `items` for `query`: label prefix, then word prefix, then substring (label or keywords); empty → all. */
export function rankItems<T extends { label: string; keywords: string }>(items: readonly T[], query: string, limit = 40): T[] {
  const q = query.trim().toLowerCase();
  if (!q) return items.slice(0, limit);
  const scored: [T, number][] = [];
  for (const item of items) {
    const label = item.label.toLowerCase();
    const words = `${label} ${item.keywords.toLowerCase()}`;
    let score = -1;
    if (label.startsWith(q)) score = 3;
    else if (words.split(/[\s·(),.]+/).some((w) => w.startsWith(q))) score = 2;
    else if (words.includes(q)) score = 1;
    if (score >= 0) scored.push([item, score]);
  }
  return scored
    .sort((a, b) => b[1] - a[1])
    .slice(0, limit)
    .map(([item]) => item);
}

/**
 * Ctrl+K / ⌘K: jump to a screen, find a running app, run "Why is my PC slow?", switch profile or pause rules. A
 * combobox with a listbox (arrow keys, Enter, Escape); while open it subscribes to the full process list.
 */
@Component({
  selector: 'dy-command-palette',
  imports: [Icon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="palette">
      <div class="search">
        <dy-icon name="search" />
        <input
          #input
          class="q"
          type="text"
          role="combobox"
          aria-expanded="true"
          aria-controls="palette-list"
          aria-autocomplete="list"
          [attr.aria-activedescendant]="activeId()"
          placeholder="Search screens, apps and actions…"
          [value]="query()"
          (input)="query.set(input.value); active.set(0)"
          (keydown)="key($event)"
          autocomplete="off"
          spellcheck="false"
          cdkFocusInitial
        />
        <kbd>Esc</kbd>
      </div>
      <ul id="palette-list" role="listbox" aria-label="Results">
        @for (item of results(); track item.id; let i = $index) {
          @if (i === 0 || results()[i - 1]!.section !== item.section) {
            <li class="section" role="presentation">{{ item.section }}</li>
          }
          <li
            role="option"
            class="item"
            [id]="'palette-' + item.id"
            [attr.aria-selected]="i === active()"
            (mousemove)="active.set(i)"
            tabindex="-1"
            (click)="choose(item)"
            (keydown.enter)="choose(item)"
          >
            <dy-icon [name]="item.icon" />
            <span class="label truncate">{{ item.label }}</span>
            <span class="hint">{{ item.hint }}</span>
          </li>
        } @empty {
          <li class="none" role="presentation">No matches for “{{ query() }}”.</li>
        }
      </ul>
      <div class="foot">
        <span><kbd>↑</kbd><kbd>↓</kbd> to move</span>
        <span><kbd>Enter</kbd> to choose</span>
        <span class="spacer"></span>
        <span>{{ platform.mod }} K anywhere</span>
      </div>
    </div>
  `,
  styles: `
    .palette { width: min(640px, calc(100vw - 32px)); background: var(--overlay-bg); border: 1px solid var(--border); border-radius: var(--r-xl); box-shadow: var(--shadow-3); overflow: hidden; animation: dy-fade-in var(--dur-fast) var(--ease-out); }
    .search { display: flex; align-items: center; gap: var(--sp-3); padding: 0 var(--sp-4); height: 52px; border-bottom: 1px solid var(--border); }
    .search > dy-icon { color: var(--text-3); }
    .q { flex: 1; border: 0; background: none; color: var(--text-1); font: var(--fs-lg) var(--font-sans); outline: none; min-width: 0; }
    .q::placeholder { color: var(--text-3); }
    ul { list-style: none; margin: 0; padding: var(--sp-2); max-height: min(420px, 60vh); overflow: auto; }
    .section { padding: var(--sp-2) var(--sp-2) var(--sp-1); font-size: var(--fs-xs); color: var(--text-3); font-weight: var(--fw-semibold); letter-spacing: 0.04em; text-transform: uppercase; }
    .item { display: flex; align-items: center; gap: var(--sp-3); height: 36px; padding: 0 var(--sp-3); border-radius: var(--r-md); cursor: pointer; }
    .item dy-icon { --icon-size: 16px; color: var(--text-2); }
    .item[aria-selected='true'] { background: var(--accent-soft); }
    .item[aria-selected='true'] dy-icon { color: var(--accent-text); }
    .label { flex: 1; }
    .hint { color: var(--text-3); font-size: var(--fs-sm); font-variant-numeric: tabular-nums; }
    .none { padding: var(--sp-6); text-align: center; color: var(--text-2); }
    .foot { display: flex; gap: var(--sp-4); align-items: center; padding: var(--sp-2) var(--sp-4); border-top: 1px solid var(--border); background: var(--surface-inset); color: var(--text-3); font-size: var(--fs-xs); }
    .foot kbd { margin-right: 2px; }
    .spacer { flex: 1; }
  `,
})
export class CommandPalette {
  private readonly ref = inject(DialogRef);
  private readonly router = inject(Router);
  private readonly rules = inject(RulesStore);
  private readonly diagnosis = inject(DiagnosisStore);
  protected readonly platform = inject(PlatformService);
  protected readonly query = signal('');
  protected readonly active = signal(0);
  private readonly groups = signal<AppGroup[]>([]);

  constructor() {
    const stop = inject(BACKEND).onLive('processes', (u) => this.groups.set(u.groups ?? []));
    inject(DestroyRef).onDestroy(stop);
    void this.rules.ensure();
  }

  private readonly items = computed<PaletteItem[]>(() => {
    const go = (path: string) => () => void this.router.navigateByUrl(path);
    const screens: PaletteItem[] = NAV.map((n) => ({ id: `go${n.path.replace('/', '-')}`, section: 'Go to', label: n.label, hint: '', icon: n.icon, keywords: n.keywords, run: go(n.path) }));
    screens.push({ id: 'go-cpu', section: 'Go to', label: 'CPU details', hint: 'Per core', icon: 'cpu', keywords: 'cores per core heatmap frequency performance efficiency', run: go('/cpu') });
    const actions: PaletteItem[] = [
      {
        id: 'why-slow',
        section: 'Actions',
        label: 'Why is my PC slow?',
        hint: 'Diagnose now',
        icon: 'why',
        keywords: 'diagnose slow cause',
        run: () => {
          void this.diagnosis.run();
          void this.router.navigateByUrl('/');
        },
      },
      {
        id: 'pause',
        section: 'Actions',
        label: this.rules.paused() ? 'Resume rules' : 'Pause all rules',
        hint: '',
        icon: this.rules.paused() ? 'play' : 'pause',
        keywords: 'pause resume rules taming stop',
        run: () => void this.rules.pause(!this.rules.paused()),
      },
      ...this.rules.profiles().map<PaletteItem>((p) => ({
        id: `profile-${p.id}`,
        section: 'Actions',
        label: `Switch to ${p.name}`,
        hint: p.id === this.rules.profile() ? 'Active' : 'Profile',
        icon: PROFILE_ICON[p.id],
        keywords: `profile mode ${p.name}`,
        run: () => void this.rules.setProfile(p.id),
      })),
    ];
    const apps: PaletteItem[] = [...this.groups()]
      .sort((a, b) => b.cpu - a.cpu)
      .map((g) => ({
        id: `app-${g.id}`,
        section: 'Apps',
        label: g.pids.length > 1 ? `${g.name} · ${g.pids.length}` : g.name,
        hint: `${formatPercent(g.cpu)} · ${formatBytes(g.memory)}`,
        icon: 'processes',
        keywords: g.exe?.split('\\').pop() ?? '',
        run: () => void this.router.navigate(['/processes'], { queryParams: { q: g.name } }),
      }));
    return [...screens, ...actions, ...apps];
  });

  protected readonly results = computed(() => {
    const q = this.query();
    // With no query, apps would drown the list: show screens and actions only.
    return rankItems(q ? this.items() : this.items().filter((i) => i.section !== 'Apps'), q);
  });

  protected readonly activeId = computed(() => {
    const item = this.results()[this.active()];
    return item ? `palette-${item.id}` : null;
  });

  protected key(e: KeyboardEvent): void {
    const n = this.results().length;
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      this.active.set(n ? (this.active() + 1) % n : 0);
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      this.active.set(n ? (this.active() - 1 + n) % n : 0);
    } else if (e.key === 'Enter') {
      e.preventDefault();
      const item = this.results()[this.active()];
      if (item) this.choose(item);
    }
  }

  protected choose(item: PaletteItem): void {
    this.ref.close();
    item.run();
  }
}
