import { ChangeDetectionStrategy, Component, input } from '@angular/core';
import { RouterLink } from '@angular/router';
import { Icon } from '../../shared/icon';
import type { IconName } from '../../shared/icons';

/** What a screen that is not built yet will do: one honest sentence each, shared by the page and the roadmap docs. */
export const PLANNED = {
  whySlow: {
    title: 'Why slow',
    icon: 'why',
    text: 'Names the likeliest cause of a slow PC in plain words, with the evidence, and lists memory leaks, periodic spikes and restart loops found in your history.',
  },
  rules: {
    title: 'CPU taming',
    icon: 'rules',
    text: 'Rules you switch on lower a runaway app’s priority, then use efficiency mode, then cap its CPU, and release it when it calms down. Everything is logged and undoable.',
  },
  cleanup: {
    title: 'Cleanup',
    icon: 'cleanup',
    text: 'Previews, then removes temp files, caches, crash dumps and developer leftovers to the Recycle Bin or Trash, with a disk space map. Nothing is deleted before you have seen the list.',
  },
  health: {
    title: 'Health & tuning',
    icon: 'health',
    text: 'A health score with reasons, a startup manager showing what slows your login, and reversible tuning suggestions that change something only when you click.',
  },
  reports: {
    title: 'Reports & alerts',
    icon: 'reports',
    text: 'A weekly health report and the history of alerts such as low disk space, overheating and crash loops.',
  },
} as const satisfies Record<string, { title: string; icon: IconName; text: string }>;

export type PlannedKey = keyof typeof PLANNED;

/** A tidy placeholder for a screen that is on the roadmap: what it will do, no fake data. */
@Component({
  selector: 'dy-planned',
  imports: [Icon, RouterLink],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="page">
      <div class="panel">
        <span class="ic"><dy-icon [name]="meta().icon" /></span>
        <div class="txt">
          <div class="row">
            <h1>{{ meta().title }}</h1>
            <span class="badge" data-tone="accent">Planned</span>
          </div>
          <p>{{ meta().text }}</p>
          <p class="note">This screen is not built yet. Dyno only measures until it is: nothing on your PC changes unless you enable a rule or click an action.</p>
          <a class="btn" routerLink="/"><dy-icon name="dashboard" /> Back to the dashboard</a>
        </div>
      </div>
    </div>
  `,
  styles: `
    :host { display: block; }
    .panel { display: flex; gap: var(--sp-5); align-items: flex-start; max-width: 680px; margin: var(--sp-10) auto 0; padding: var(--sp-8); background: var(--surface-2); border: 1px solid var(--border); border-radius: var(--r-xl); box-shadow: var(--shadow-1); }
    .ic { display: grid; place-items: center; width: 56px; height: 56px; flex: none; border-radius: var(--r-lg); background: var(--accent-soft); color: var(--accent-text); --icon-size: 28px; }
    .txt { display: grid; gap: var(--sp-3); justify-items: start; min-width: 0; }
    .row { display: flex; align-items: center; gap: var(--sp-3); }
    p { color: var(--text-2); font-size: var(--fs-md); line-height: 1.5; }
    .note { color: var(--text-3); font-size: var(--fs-sm); }
  `,
})
export class Planned {
  readonly page = input.required<PlannedKey>();
  protected meta() {
    return PLANNED[this.page()];
  }
}
