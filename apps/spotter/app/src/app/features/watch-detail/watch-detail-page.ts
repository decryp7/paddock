import { ChangeDetectionStrategy, Component, computed, effect, inject, input, signal, untracked } from '@angular/core';
import { MatIcon } from '@angular/material/icon';
import { Router, RouterLink } from '@angular/router';
import { SuEmptyState } from '@decryp7/paddock-ui';
import type { Release, WatchAction } from '../../core/api.types';
import { Clipboard } from '../../shared/clipboard';
import { formatBytes, formatDateTime, formatDuration, formatInterval, shortHash } from '../../shared/format';
import { SCHEME_OPTIONS } from '../../shared/labels';
import { AgoPipe } from '../../shared/ui/ago.pipe';
import { Confirm } from '../../shared/ui/confirm-dialog';
import { KindIcon } from '../../shared/ui/kind-icon';
import { Panel } from '../../shared/ui/panel';
import { WatchStatus } from '../../shared/ui/watch-status';
import { WatchesStore } from '../../state/watches.store';
import { WatchDetailStore } from './watch-detail.store';

/** One watch: its latest release, history, recent checks and settings, with every action on it. */
@Component({
  selector: 'sp-watch-detail-page',
  imports: [MatIcon, RouterLink, SuEmptyState, Panel, KindIcon, WatchStatus, AgoPipe],
  providers: [WatchDetailStore],
  changeDetection: ChangeDetectionStrategy.OnPush,
  templateUrl: './watch-detail-page.html',
  styleUrl: './watch-detail-page.scss',
})
export class WatchDetailPage {
  /** From the route (`/watches/:id`). */
  readonly id = input.required<string>();

  protected readonly store = inject(WatchDetailStore);
  protected readonly watches = inject(WatchesStore);
  private readonly router = inject(Router);
  private readonly confirm = inject(Confirm);
  private readonly clipboard = inject(Clipboard);

  protected readonly detail = computed(() => this.store.detail().value);
  protected readonly summary = computed(() => this.watches.byId(this.id()) ?? this.detail()?.summary ?? null);
  protected readonly installedPicker = signal(false);
  protected readonly bytes = formatBytes;
  protected readonly dateTime = formatDateTime;
  protected readonly duration = formatDuration;
  protected readonly interval = formatInterval;
  protected readonly hash = (h: string | null) => shortHash(h);
  protected readonly checks = computed(() => [...(this.detail()?.checks ?? [])].reverse().slice(0, 12));
  protected readonly scheme = computed(() => SCHEME_OPTIONS.find((s) => s.value === this.detail()?.spec.versions)?.label ?? this.detail()?.spec.versions ?? '');

  constructor() {
    effect(() => {
      const id = this.id();
      untracked(() => void this.store.load(id));
    });
    // The list is pushed after every change (a check, an acknowledgement); the page follows it.
    let first = true;
    effect(() => {
      this.watches.watches();
      if (first) first = false;
      else untracked(() => void this.store.reload());
    });
  }

  protected async remove(): Promise<void> {
    const d = this.detail();
    if (!d) return;
    const ok = await this.confirm.ask({
      title: `Delete "${d.spec.name}"?`,
      message: 'Spotter stops watching it and forgets its history. Downloaded files stay where they are.',
      confirmLabel: 'Delete watch',
      danger: true,
    });
    if (ok && (await this.watches.remove(this.id()))) await this.router.navigate(['/watches']);
  }

  protected async setInstalled(version: string | null): Promise<void> {
    this.installedPicker.set(false);
    await this.watches.setInstalled(this.id(), version);
  }

  protected onInstalled(e: Event): void {
    const v = (e.target as HTMLSelectElement).value;
    void this.setInstalled(v === '' ? null : v);
  }

  protected copy(text: string, what: string): void {
    void this.clipboard.copy(text, what);
  }

  protected actionText(a: WatchAction): string {
    switch (a.kind) {
      case 'open-location':
        return 'Open the release location';
      case 'download':
        return `Download to ${a.folder || 'the downloads folder'}${a.verify ? ' and verify the checksum' : ''}`;
      case 'run-command':
        return `Run ${a.program}${a.args.length ? ' ' + a.args.join(' ') : ''}`;
    }
  }

  protected notifyText(mode: string): string {
    return mode === 'every-new' ? 'Every new release' : mode === 'newer-than-installed' ? 'Only releases newer than the installed version' : 'Off';
  }

  protected isInstalled(r: Release): boolean {
    return r.version === this.summary()?.installed;
  }
}
