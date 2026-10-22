import { ChangeDetectionStrategy, Component, DOCUMENT, DestroyRef, OnInit, afterNextRender, computed, inject, signal } from '@angular/core';
import { MatIconModule } from '@angular/material/icon';
import { MatSlideToggleModule } from '@angular/material/slide-toggle';
import { MatTooltipModule } from '@angular/material/tooltip';
import type { CacheClearScope, CacheInfo, CacheKindInfo, CacheStorage, Preset, ThemeChoice } from '../../core/api.types';
import { LiveStore } from '../../core/live.store';
import { ThemeService } from '../../core/theme';
import { Toasts } from '../../core/toasts';
import { PRESET_HINT, PRESET_LABEL } from '../../shared/labels';
import { Confirm } from '../../shared/ui/confirm';
import { Panel } from '../../shared/ui/panel';
import { PwSelect } from '../../shared/ui/select';
import { bytes, date, int } from '../../shared/format';
import { expiryText, oldestText, sizeText, ttlOptions } from './cache-text';
import { consentDetail } from '../service/service-consent';
import { ServiceStore } from '../service/service.store';
import { checkSummary } from '../updates/update-text';
import { UpdatesStore } from '../updates/updates.store';
import { injectNow } from '../../shared/now';
import { SettingsStore } from './settings.store';

const GB = 1024 ** 3;
const MB = 1024 ** 2;
const PRESETS: Exclude<Preset, 'custom'>[] = ['lowMemory', 'balanced', 'performance'];

const SECTIONS = [
  { id: 'general', label: 'General', icon: 'sliders' },
  { id: 'server', label: 'Server', icon: 'server' },
  { id: 'memory', label: 'Memory', icon: 'memory' },
  { id: 'cache', label: 'Caching', icon: 'bolt' },
  { id: 'network', label: 'Network', icon: 'globe' },
  { id: 'model-updates', label: 'Model updates', icon: 'download' },
  { id: 'privacy', label: 'Privacy', icon: 'lock' },
  { id: 'service', label: 'Service', icon: 'server' },
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
  imports: [MatIconModule, MatSlideToggleModule, MatTooltipModule, Panel, PwSelect],
  providers: [SettingsStore],
  changeDetection: ChangeDetectionStrategy.OnPush,
  templateUrl: './settings-page.html',
  styleUrl: './settings-page.scss',
})
export class SettingsPage implements OnInit {
  protected readonly store = inject(SettingsStore);
  protected readonly live = inject(LiveStore);
  protected readonly service = inject(ServiceStore);
  protected readonly updates = inject(UpdatesStore);
  private readonly now = injectNow();
  protected readonly updateSummary = computed(() => {
    const s = this.updates.status();
    return s ? checkSummary(s, this.now()) : '';
  });
  private readonly theme = inject(ThemeService);
  private readonly toasts = inject(Toasts);
  private readonly doc = inject(DOCUMENT);
  private readonly confirm = inject(Confirm);
  protected readonly sections = SECTIONS;
  protected readonly intText = int;
  protected readonly sizeOf = bytes;
  protected readonly cacheKinds: { id: 'embeddings' | 'responses'; title: string; noun: string; about: string; ttlHint: string }[] = [
    { id: 'embeddings', title: 'Embeddings', noun: 'cached embedding', about: 'Vectors for text your apps embed. The same text and model always give the same vector.', ttlHint: 'Counted since the entry was last used, so what you keep using stays.' },
    { id: 'responses', title: 'Cached answers', noun: 'saved answer', about: 'Answers to deterministic requests from apps and presets that opted in (Apps & keys, Models ▸ Presets).', ttlHint: 'Counted from when the answer was stored, so old answers do not outlive a model update for long.' },
  ];
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
    void this.service.refresh();
    void this.updates.load();
    void this.store.loadCacheInfo();
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

  protected platformLabel(platform: string): string {
    return platform === 'windows' ? 'Windows service' : platform === 'macos' ? 'Login item (launch agent)' : 'systemd user service';
  }

  protected serviceHint(platform: string): string {
    return consentDetail(platform);
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

  protected storage(k: 'embeddings' | 'responses'): CacheStorage {
    const c = this.store.draft()?.cache;
    return c ? (k === 'embeddings' ? c.embeddingsStorage : c.responsesStorage) : 'disk';
  }
  protected maxMib(k: 'embeddings' | 'responses'): number {
    const c = this.store.draft()?.cache;
    return c ? (k === 'embeddings' ? c.embeddingsMaxMib : c.responsesMaxMib) : 0;
  }
  protected ttl(k: 'embeddings' | 'responses'): number {
    const c = this.store.draft()?.cache;
    return c ? (k === 'embeddings' ? c.embeddingsTtlSecs : c.responsesTtlSecs) : 0;
  }
  protected ttlChoices(k: 'embeddings' | 'responses') {
    return ttlOptions(this.ttl(k), k === 'embeddings');
  }
  protected setStorage(k: 'embeddings' | 'responses', v: CacheStorage): void {
    this.store.edit('cache', k === 'embeddings' ? { embeddingsStorage: v } : { responsesStorage: v });
  }
  protected setMaxMib(k: 'embeddings' | 'responses', v: string): void {
    const n = parseNumber(v, 1, 1_048_576);
    if (n !== null) this.store.edit('cache', k === 'embeddings' ? { embeddingsMaxMib: Math.round(n) } : { responsesMaxMib: Math.round(n) });
  }
  protected setTtl(k: 'embeddings' | 'responses', v: string): void {
    const n = Number(v);
    if (Number.isFinite(n) && n >= 0) this.store.edit('cache', k === 'embeddings' ? { embeddingsTtlSecs: n } : { responsesTtlSecs: n });
  }
  protected setPromptCacheAuto(auto: boolean): void {
    this.store.edit('cache', { promptCacheMib: auto ? null : 1024 });
  }
  protected setPromptCacheMib(v: string): void {
    const n = parseNumber(v, 0, 1_048_576);
    if (n !== null) this.store.edit('cache', { promptCacheMib: Math.round(n) });
  }
  protected setKeepWarmDays(v: string): void {
    const n = parseNumber(v, 1, 3650);
    if (n !== null) this.store.edit('cache', { keepWarmDays: Math.round(n) });
  }
  protected cacheSize(k: CacheKindInfo): string {
    return sizeText(k);
  }
  protected oldest(k: CacheKindInfo): string {
    return oldestText(k, this.now());
  }
  protected expiry(k: CacheKindInfo): string {
    return expiryText(k);
  }
  protected nextSweep(at: CacheInfo['nextSweepAt']): string {
    return date(at, true);
  }

  /** Clearing is destructive but cheap to undo (the caches refill), so it asks once, naming what goes. */
  protected async clear(scope: CacheClearScope, what: string): Promise<void> {
    const ok = await this.confirm.ask({
      title: `Clear ${what}?`,
      message: 'The entries are deleted now. Apps keep working; requests are answered by the model again until the cache refills.',
      confirm: 'Clear now',
      danger: true,
    });
    if (ok) await this.store.clearCache(scope);
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
