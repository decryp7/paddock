import { ChangeDetectionStrategy, Component, DOCUMENT, DestroyRef, OnInit, afterNextRender, computed, inject, signal } from '@angular/core';
import { MatIconModule } from '@angular/material/icon';
import { MatSlideToggleModule } from '@angular/material/slide-toggle';
import { MatTooltipModule } from '@angular/material/tooltip';
import type { Preset, ThemeChoice } from '../../core/api.types';
import { LiveStore } from '../../core/live.store';
import { ThemeService } from '../../core/theme';
import { Toasts } from '../../core/toasts';
import { PRESET_HINT, PRESET_LABEL } from '../../shared/labels';
import { Panel } from '../../shared/ui/panel';
import { SettingsStore } from './settings.store';

const GB = 1024 ** 3;
const MB = 1024 ** 2;
const PRESETS: Exclude<Preset, 'custom'>[] = ['lowMemory', 'balanced', 'performance'];

const SECTIONS = [
  { id: 'general', label: 'General', icon: 'sliders' },
  { id: 'server', label: 'Server', icon: 'server' },
  { id: 'memory', label: 'Memory', icon: 'memory' },
  { id: 'network', label: 'Network', icon: 'globe' },
  { id: 'privacy', label: 'Privacy', icon: 'lock' },
  { id: 'updates', label: 'Updates', icon: 'download' },
] as const;

/** A number from a number input, clamped to `[min, max]`, or null when it is empty or not a number. */
export function parseNumber(value: string, min = -Infinity, max = Infinity): number | null {
  if (value.trim() === '') return null;
  const n = Number(value);
  if (!Number.isFinite(n)) return null;
  return Math.min(max, Math.max(min, n));
}

/** Everything Pitwall can be told: general, server, memory, network, privacy and updates. */
@Component({
  selector: 'pw-settings-page',
  imports: [MatIconModule, MatSlideToggleModule, MatTooltipModule, Panel],
  providers: [SettingsStore],
  changeDetection: ChangeDetectionStrategy.OnPush,
  templateUrl: './settings-page.html',
  styleUrl: './settings-page.scss',
})
export class SettingsPage implements OnInit {
  protected readonly store = inject(SettingsStore);
  protected readonly live = inject(LiveStore);
  private readonly theme = inject(ThemeService);
  private readonly toasts = inject(Toasts);
  private readonly doc = inject(DOCUMENT);
  protected readonly sections = SECTIONS;
  protected readonly presets = PRESETS;
  protected readonly presetLabel = PRESET_LABEL;
  protected readonly presetHint = PRESET_HINT;
  protected readonly active = signal<string>('general');
  protected readonly themes: { value: ThemeChoice; label: string; icon: string }[] = [
    { value: 'system', label: 'System', icon: 'monitor' },
    { value: 'light', label: 'Light', icon: 'sun' },
    { value: 'dark', label: 'Dark', icon: 'moon' },
  ];
  protected readonly d = computed(() => this.store.draft());
  protected readonly lan = computed(() => this.live.server()?.lan ?? null);

  constructor() {
    // Scroll spy: the sub-navigation follows the section at the top of the view.
    let observer: IntersectionObserver | null = null;
    afterNextRender(() => {
      if (typeof IntersectionObserver === 'undefined') return;
      observer = new IntersectionObserver(
        (entries) => {
          const top = entries.filter((e) => e.isIntersecting).sort((a, b) => a.boundingClientRect.top - b.boundingClientRect.top)[0];
          if (top) this.active.set(top.target.id.replace(/^set-/, ''));
        },
        { rootMargin: '0px 0px -65% 0px' },
      );
      this.observe = () => this.doc.querySelectorAll('[id^="set-"]').forEach((el) => observer?.observe(el));
      this.observe();
    });
    inject(DestroyRef).onDestroy(() => observer?.disconnect());
  }

  /** Starts observing the section cards (they render once settings load). */
  private observe: () => void = () => undefined;

  ngOnInit(): void {
    void this.store.load().then(() => setTimeout(() => this.observe()));
  }

  protected go(id: string): void {
    this.active.set(id);
    this.doc.getElementById(`set-${id}`)?.scrollIntoView?.({ behavior: 'smooth', block: 'start' });
  }

  protected setTheme(choice: ThemeChoice): void {
    this.store.edit('general', { theme: choice });
    this.theme.set(choice);
  }

  protected discard(): void {
    this.store.discard();
    const t = this.store.saved()?.general.theme;
    if (t) this.theme.set(t);
  }

  protected value(e: Event): string {
    return (e.target as HTMLInputElement).value;
  }

  protected gb(bytes: number | null): string {
    return bytes === null ? '' : String(Math.round((bytes / GB) * 10) / 10);
  }

  protected mb(bytes: number): string {
    return String(Math.round(bytes / MB));
  }

  protected minutes(secs: number): string {
    return String(Math.round((secs / 60) * 10) / 10);
  }

  protected setPort(v: string): void {
    const n = parseNumber(v, 1, 65535);
    if (n !== null) this.store.edit('server', { port: Math.round(n) });
  }
  protected setMaxRequest(v: string): void {
    const n = parseNumber(v, 1, 4096);
    if (n !== null) this.store.edit('server', { maxRequestBytes: Math.round(n) * MB });
  }
  protected setTimeoutSecs(v: string): void {
    const n = parseNumber(v, 5, 86_400);
    if (n !== null) this.store.edit('server', { requestTimeoutSecs: Math.round(n) });
  }
  protected setMaxConcurrent(v: string): void {
    const n = parseNumber(v, 1, 64);
    if (n !== null) this.store.edit('server', { maxConcurrent: Math.round(n) });
  }
  protected setIdleMinutes(key: 'idleUnloadSecs' | 'deepIdleSecs', v: string): void {
    const n = parseNumber(v, 0.5, 1440);
    if (n === null) return;
    const secs = Math.round(n * 60);
    this.store.edit('memory', key === 'idleUnloadSecs' ? { idleUnloadSecs: secs } : { deepIdleSecs: secs });
  }
  protected setPressure(v: string): void {
    const n = parseNumber(v, 1, 90);
    if (n !== null) this.store.edit('memory', { pressureAvailablePercent: Math.round(n) });
  }
  protected setMinFree(v: string): void {
    const n = parseNumber(v, 0, 1024);
    if (n !== null) this.store.edit('memory', { minFreeBytes: Math.round(n * GB) });
  }
  protected setMaxHot(v: string): void {
    const n = parseNumber(v, 1, 16);
    if (n !== null) this.store.edit('memory', { maxHotModels: Math.round(n) });
  }
  protected setHotCap(v: string): void {
    const n = parseNumber(v, 1, 1024);
    if (n !== null) this.store.edit('memory', { hotCapBytes: Math.round(n * GB) });
  }
  protected setCapMode(fits: boolean): void {
    this.store.edit('memory', { hotCapBytes: fits ? null : 16 * GB });
  }
  protected setLanPort(v: string): void {
    const n = parseNumber(v, 1024, 65535);
    if (n !== null) this.store.edit('network', { lanPort: Math.round(n) });
  }
  protected setRetention(v: string): void {
    const n = parseNumber(v, 1, 365);
    if (n !== null) this.store.edit('privacy', { metricsRetentionDays: Math.round(n) });
  }

  protected async copy(text: string): Promise<void> {
    try {
      await navigator.clipboard.writeText(text);
      this.toasts.ok('Copied.');
    } catch {
      this.toasts.show('Copy failed: select the text and copy it yourself.', 'bad');
    }
  }
}
