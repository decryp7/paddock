import { ChangeDetectionStrategy, Component, computed, effect, inject, input, signal, untracked } from '@angular/core';
import { MatIcon } from '@angular/material/icon';
import { Router, RouterLink } from '@angular/router';
import { SuEmptyState } from '@decryp7/paddock-ui';
import type { CheckRecord, Release, WatchAction } from '../../core/api.types';
import { ClockService } from '../../core/clock.service';
import { Clipboard } from '../../shared/clipboard';
import { formatBytes, formatDateTime, formatDuration, formatInterval, formatShortDateTime, mebibytes } from '../../shared/format';
import { kindIcon, SCHEME_OPTIONS } from '../../shared/labels';
import { middleEllipsis } from '../../shared/identity';
import { AgoPipe, AgoShortPipe } from '../../shared/ui/ago.pipe';
import { Avatar } from '../../shared/ui/avatar';
import { Confirm } from '../../shared/ui/confirm-dialog';
import { Panel } from '../../shared/ui/panel';
import { MenuButton, type MenuEntry } from '../../shared/ui/row-menu';
import { StatusDot } from '../../shared/ui/status-dot';
import { Version } from '../../shared/ui/version';
import { WatchesStore } from '../../state/watches.store';
import { WatchDetailStore } from './watch-detail.store';

function choice(a: { variant?: string; extension?: string }): string {
  const parts = [a.variant, a.extension ? '.' + a.extension : ''].filter(Boolean);
  return parts.length ? ` (${parts.join(' ')})` : '';
}

/** One watch: its latest release, history, recent checks and settings, with every action on it. */
@Component({
  selector: 'sp-watch-detail-page',
  imports: [MatIcon, RouterLink, SuEmptyState, Panel, Avatar, StatusDot, Version, MenuButton, AgoPipe, AgoShortPipe],
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
  protected readonly mib = mebibytes;
  protected readonly glyph = kindIcon;
  protected readonly middle = (text: string) => middleEllipsis(text, 38);
  private readonly clock = inject(ClockService);
  protected readonly short = (ms: number | null | undefined) => formatShortDateTime(ms, this.clock.now());
  /** The last 30 checks, oldest first, each marked when it found more than the check before. */
  protected readonly checks = computed<(CheckRecord & { changed: boolean })[]>(() => {
    const all = [...(this.detail()?.checks ?? [])].sort((a, b) => a.atMs - b.atMs);
    return all.map((c, i) => ({ ...c, changed: !c.error && i > 0 && c.found > (all[i - 1]?.found ?? 0) })).slice(-30);
  });
  protected readonly lastFailure = computed(() => [...this.checks()].reverse().find((c) => c.error)?.error ?? null);
  protected readonly pickable = computed(() => (this.detail()?.history ?? []).slice(0, 12));
  private readonly selectedKey = signal<{ version: string; product: string | null } | null>(null);
  protected readonly selected = computed(() => {
    const k = this.selectedKey();
    return k ? (this.detail()?.history.find((r) => r.version === k.version && r.product === k.product) ?? null) : null;
  });
  private opener: HTMLElement | null = null;
  protected readonly menuEntries = computed<MenuEntry[]>(() => [
    { id: 'edit', label: 'Edit', icon: 'edit' },
    { id: 'installed', label: 'I have installed…', icon: 'package' },
    { id: 'copy', label: 'Copy address', icon: 'copy' },
    { id: 'delete', label: 'Delete watch', icon: 'trash', danger: true, separated: true },
  ]);
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

  protected select(r: Release): void {
    this.opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    this.selectedKey.set({ version: r.version, product: r.product });
  }

  protected isSelected(r: Release): boolean {
    return this.selected() === r;
  }

  protected closePanel(): void {
    this.selectedKey.set(null);
    this.opener?.focus();
  }

  protected checkTitle(c: CheckRecord): string {
    return `${formatDateTime(c.atMs)}: ${c.error ? 'failed, ' + c.error : 'found ' + c.found} (${formatDuration(c.tookMs)})`;
  }

  protected onMenu(id: string): void {
    const s = this.summary();
    if (id === 'edit') void this.router.navigate(['/watches', this.id(), 'edit']);
    else if (id === 'installed') this.installedPicker.update((v) => !v);
    else if (id === 'copy' && s) this.copy(s.sourceDescription, 'Address');
    else if (id === 'delete') void this.remove();
  }

  protected releaseEntries(r: Release): MenuEntry[] {
    return [
      { id: 'link', label: 'Copy link', icon: 'link' },
      ...(r.sha256 ? [{ id: 'sha', label: 'Copy SHA-256', icon: 'lock' }] : []),
      { id: 'installed', label: 'Mark as installed', icon: 'package', separated: true },
      { id: 'seen', label: 'Mark as seen', icon: 'check' },
    ];
  }

  protected onRelease(id: string, r: Release): void {
    if (id === 'link') this.copy(r.location, 'Link');
    else if (id === 'sha' && r.sha256) this.copy(r.sha256, 'Checksum');
    else if (id === 'installed') void this.setInstalled(r.version);
    else if (id === 'seen') void this.watches.acknowledge(this.id(), r.version);
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

  protected copy(text: string, what: string): void {
    void this.clipboard.copy(text, what);
  }

  protected actionText(a: WatchAction): string {
    switch (a.kind) {
      case 'open-location':
        return `Open the release location${choice(a)}`;
      case 'download':
        return `Download${choice(a)} to ${a.folder || 'the downloads folder'}${a.verify ? ' and verify the checksum' : ''}`;
      case 'run-command':
        return `Run ${a.program}${a.args.length ? ' ' + a.args.join(' ') : ''}`;
    }
  }

  protected latestByText(by: string): string {
    return by === 'modified' ? 'Newest modified time' : by === 'created' ? 'Newest created time' : by === 'name' ? 'Last name in natural order' : 'Newest version';
  }

  protected notifyText(mode: string): string {
    return mode === 'every-new' ? 'Every new release' : mode === 'newer-than-installed' ? 'Only releases newer than the installed version' : 'Off';
  }

  protected isInstalled(r: Release): boolean {
    return r.version === this.summary()?.installed;
  }
}
