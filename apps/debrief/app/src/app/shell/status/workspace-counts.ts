import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { formatCount } from '../../core/format';
import { WorkspaceStore } from '../../core/workspace.store';
import { formatDuration } from '../../shared/format/units';

/** Status item (start): the events, matching and instance counts and the profile, or why there are none. */
@Component({
  selector: 'debrief-status-counts',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @if (store.summary(); as s) {
      @if (s.sources.length) {
      <span class="keep strong" [title]="indexedTitle(s.loadMs)" i18n="@@shell.statusBar.events">{{ fmt(s.total) }} events</span>
      @if (store.matchTotal() !== null) {
        <span class="keep" i18n="@@shell.statusBar.matching">{{ fmt(store.matchTotal()!) }} matching</span>
      }
      <span class="low" i18n="@@shell.statusBar.instances">{{ fmt(store.instanceCount()) }} instances</span>
      @if (s.profileName) {
        <span class="low" data-testid="status-profile" [title]="profileHint(s.profileName)" i18n="@@shell.statusBar.profile">Profile: {{ s.profileName }}</span>
      }
      } @else {
        <span i18n="@@shell.statusBar.noFiles">No files open</span>
      }
    } @else {
      <span i18n="@@shell.statusBar.noWorkspace">No workspace</span>
    }
  `,
  styles: `
    /* The spans are items of the status bar's row (display: contents). When space runs out the secondary labels
       truncate; the events and matching counts keep their size. */
    :host { display: contents; }
    span:not(.keep):not(.strong) { min-width: 0; overflow: hidden; text-overflow: ellipsis; }
    .keep, .strong { flex: none; }
    /* Narrow windows (H1): the low-priority items go; the counts and the controls stay. */
    @media (max-width: 1000px) { .low { display: none; } }
    .strong { font-weight: 600; color: var(--statusbar-strong); }
  `,
})
export class StatusCounts {
  protected readonly store = inject(WorkspaceStore);
  protected readonly fmt = formatCount;
  protected profileHint(name: string): string {
    return $localize`:@@shell.statusBar.profileTitle:The parsing profile in use: ${name}:name:`;
  }
  protected indexedTitle(loadMs: number): string {
    const took = formatDuration(loadMs);
    return $localize`:@@shell.statusBar.indexedTitle:Indexed in ${took}:duration:`;
  }
}
