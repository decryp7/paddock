import { ChangeDetectionStrategy, Component, DOCUMENT, DestroyRef, InjectionToken, OnInit, computed, effect, inject, signal, untracked } from '@angular/core';
import { MatButton, MatIconButton } from '@angular/material/button';
import { PdNotificationHost } from '@decryp7/paddock-ui/shell/notify';
import { MatFormField, MatSuffix } from '@angular/material/form-field';
import { MatIconModule } from '@angular/material/icon';
import { MatInput } from '@angular/material/input';
import { PdSelect, type PdSelectOption } from '@decryp7/paddock-ui/select';
import { MatSlideToggleModule } from '@angular/material/slide-toggle';
import { MatTooltipModule } from '@angular/material/tooltip';
import type { CacheClearScope, CacheInfo, CacheKindInfo, CacheStorage, OnQuit, Preset, Settings } from '../../core/api.types';
import { LiveStore } from '../../core/live.store';
import { Toasts } from '../../core/toasts';
import { PRESET_HINT, PRESET_LABEL } from '../../shared/labels';
import { confirm } from '@decryp7/paddock-ui/shell/confirm';
import { DialogService } from '@decryp7/paddock-ui/shell/dialogs';
import { Panel } from '../../shared/ui/panel';
import { bytes, date, int } from '../../shared/format';
import { expiryText, oldestText, sizeText, ttlOptions } from './cache-text';
import { ServicePanel } from '../service/service-panel';
import { ServiceStore } from '../service/service.store';
import { checkSummary } from '../updates/update-text';
import { UpdatesStore } from '../updates/updates.store';
import { injectNow } from '../../shared/now';
import { refusalText, SettingsStore, undoText, unsentText, type SettingField } from './settings.store';
import { settingsPageCloseGuard } from '@decryp7/paddock-ui/shell/settings/window';

const GB = 1024 ** 3;
const MB = 1024 ** 2;
const PRESETS: Exclude<Preset, 'custom'>[] = ['lowMemory', 'balanced', 'performance'];

/** A typed value saves after the pause; `now` is a commit (Enter, leaving the field). */
type How = 'now' | 'typed';

/** The settings with a field of their own, by the name the page shows (the id is the input's, so its inline error follows it). */
const FIELD = {
  onQuit: { id: 'gen-quit', label: 'When you quit Pitwall' },
  port: { id: 'srv-port', label: 'Port' },
  maxRequest: { id: 'srv-max', label: 'Largest request' },
  timeout: { id: 'srv-timeout', label: 'Request timeout' },
  concurrent: { id: 'srv-conc', label: 'Requests at once' },
  idle: { id: 'mem-idle', label: 'Release a model after idle' },
  deepIdle: { id: 'mem-deep', label: 'Stop engines after idle' },
  pressure: { id: 'mem-pressure', label: 'Memory pressure' },
  minFree: { id: 'mem-min', label: 'Always keep free' },
  maxHot: { id: 'mem-hot', label: 'Hot models at once' },
  // The choice and the number of one value are one setting: the later edit replaces the earlier.
  hotCap: { id: 'mem-cap', label: 'Memory for hot models' },
  lanPort: { id: 'net-port', label: 'Network port' },
  retention: { id: 'prv-ret', label: 'Keep metrics and history for' },
  promptCache: { id: 'cache-prompt-mib', label: 'Prompt cache size' },
  keepWarm: { id: 'cache-keep-warm', label: 'Keep unused files for' },
} as const satisfies Record<string, SettingField>;

/** The switches' names, by their setting. */
const SWITCH_LABEL: Readonly<Record<string, string>> = {
  allowKeylessLocal: 'Accept local requests without a key',
  releaseContextFirst: 'Release the context first',
  yieldToForeground: 'Yield to full-screen apps',
  notifyOnRelease: 'Notify when memory is released',
  prefixAffinity: 'Keep a conversation on one slot',
  embeddings: 'Cache embeddings',
  offline: 'Offline mode',
  lanEnabled: 'Serve other devices on your network',
  keepRequestContent: 'Keep request content',
};

/**
 * Set in a window of its own (the Settings window booted by URL): the page shows its confirmations itself, since no pages frame
 * with a notification host is around it. In the main window's in-page dialog (the browser build) the frame's host shows them.
 */
export const OWN_TOAST_HOST = new InjectionToken<boolean>('PitwallOwnToastHost', { factory: () => false });

/** A number typed into a number field, or what to fix when it is empty, not a number or outside `[min, max]`. */
export function checkNumber(value: string, min: number, max: number): { ok: true; value: number } | { ok: false; error: string } {
  const n = value.trim() === '' ? NaN : Number(value);
  if (!Number.isFinite(n) || n < min || n > max) return { ok: false, error: `Enter a number from ${min} to ${max}.` };
  return { ok: true, value: n };
}

/**
 * Pitwall's page of the Settings window (Appearance and Updates are the shell's pages beside it): general, server, memory,
 * caching, network, model updates, privacy and the background service.
 */
@Component({
  selector: 'pw-settings-page',
  imports: [MatButton, MatIconButton, MatFormField, MatInput, MatSuffix, MatIconModule, MatSlideToggleModule, MatTooltipModule, Panel, PdSelect, ServicePanel, PdNotificationHost],
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
  private readonly toasts = inject(Toasts);
  protected readonly ownToasts = inject(OWN_TOAST_HOST);
  private readonly dialogs = inject(DialogService);
  protected readonly intText = int;
  protected readonly sizeOf = bytes;
  protected readonly cacheKinds: { id: 'embeddings' | 'responses'; title: string; noun: string; about: string; ttlHint: string }[] = [
    { id: 'embeddings', title: 'Embeddings', noun: 'cached embedding', about: 'Vectors for text your apps embed. The same text and model always give the same vector.', ttlHint: 'Counted since the entry was last used, so what you keep using stays.' },
    { id: 'responses', title: 'Cached answers', noun: 'saved answer', about: 'Answers to deterministic requests from apps and presets that opted in (Apps & keys, Models ▸ Presets).', ttlHint: 'Counted from when the answer was stored, so old answers do not outlive a model update for long.' },
  ];
  /** The memory presets; Custom is what the values below make when they differ from every preset (never chosen here). */
  protected readonly presetOptions: PdSelectOption[] = [...PRESETS.map((p) => ({ value: p, label: PRESET_LABEL[p] })), { value: 'custom', label: PRESET_LABEL.custom, disabled: true }];
  protected presetHint(p: Preset): string {
    return p === 'custom' ? 'Your own values below. Choose a preset to replace them.' : `${PRESET_HINT[p]}.`;
  }
  protected readonly quitOptions: { value: OnQuit; label: string }[] = [
    { value: 'keepServing', label: 'Keep serving' },
    { value: 'stopServing', label: 'Stop serving' },
    { value: 'ask', label: 'Ask each time' },
  ];
  protected readonly capOptions: PdSelectOption[] = [
    { value: 'fits', label: 'What fits' },
    { value: 'limit', label: 'Set a limit' },
  ];
  protected readonly promptCacheOptions: PdSelectOption[] = [
    { value: 'auto', label: 'Automatic' },
    { value: 'size', label: 'Set a size' },
  ];
  protected readonly storageOptions: PdSelectOption[] = [
    { value: 'disk', label: 'On disk' },
    { value: 'memory', label: 'Memory only' },
  ];
  protected readonly d = computed(() => this.store.draft());
  protected readonly lan = computed(() => this.live.server()?.lan ?? null);
  /** What to fix in a number field (by the input's id) whose value was not taken. */
  protected readonly fieldErrors = signal<Readonly<Record<string, string>>>({});

  protected readonly refusalText = refusalText;
  protected readonly undoText = undoText;
  protected readonly unsentText = unsentText;

  constructor() {
    // The Settings window asks before it closes: a typed value still waiting is saved first, a refused one is named.
    settingsPageCloseGuard(() => this.store.closeVerdict());
    // Settings save themselves: an edit still waiting is saved when the page goes away or its window hides or closes.
    const doc = inject(DOCUMENT);
    const flush = () => void this.store.flush();
    const onHidden = () => doc.visibilityState === 'hidden' && flush();
    doc.defaultView?.addEventListener('pagehide', flush);
    doc.addEventListener('visibilitychange', onHidden);
    inject(DestroyRef).onDestroy(() => {
      doc.defaultView?.removeEventListener('pagehide', flush);
      doc.removeEventListener('visibilitychange', onHidden);
      flush();
    });
    // A saved port or network setting changes the address shown: read the overview again.
    let last: string | null = null;
    effect(() => {
      const s = this.store.saved();
      if (!s) return;
      const key = JSON.stringify([s.server.port, s.network]);
      if (last !== null && key !== last) untracked(() => void this.live.loadOverview());
      last = key;
    });
  }

  ngOnInit(): void {
    void this.service.refresh();
    void this.updates.load();
    void this.store.loadCacheInfo();
    void this.store.load();
    // The network address and certificate come from the server's overview; the Settings window reads it itself.
    if (!this.live.overview()) void this.live.loadOverview();
    this.live.connect();
  }

  protected fieldError(id: string): string | null {
    return this.fieldErrors()[id] ?? null;
  }

  /**
   * Takes a number field's value: while typing (`typed`) a valid value is saved after the pause and an invalid one waits
   * quietly; on Enter or leaving the field (`now`) a valid value is saved at once and an invalid one says inline what to fix.
   */
  private num(field: SettingField, v: string, how: How, min: number, max: number, apply: (n: number) => void): void {
    const r = checkNumber(v, min, max);
    if (!r.ok && how === 'typed') return;
    this.fieldErrors.update((all) => {
      const { [field.id]: _gone, ...rest } = all;
      return r.ok ? rest : { ...rest, [field.id]: r.error };
    });
    if (r.ok) apply(r.value);
  }

  /** A switch: saved at once. */
  protected toggle<S extends keyof Settings>(section: S, key: keyof Settings[S] & string, on: boolean): void {
    this.store.edit({ id: `${section}.${key}`, label: SWITCH_LABEL[key] ?? key }, section, { [key]: on } as Partial<Settings[S]>);
  }

  protected setPreset(v: string): void {
    if (v === 'lowMemory' || v === 'balanced' || v === 'performance') void this.store.applyPreset(v);
  }
  protected setOnQuit(v: string): void {
    if (v === 'keepServing' || v === 'stopServing' || v === 'ask') this.store.edit(FIELD.onQuit, 'general', { onQuit: v });
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

  protected setPort(v: string, how: How): void {
    this.num(FIELD.port, v, how, 1024, 65535, (n) => this.store.edit(FIELD.port, 'server', { port: Math.round(n) }, how));
  }
  protected setMaxRequest(v: string, how: How): void {
    this.num(FIELD.maxRequest, v, how, 1, 4096, (n) => this.store.edit(FIELD.maxRequest, 'server', { maxRequestBytes: Math.round(n) * MB }, how));
  }
  protected setTimeoutSecs(v: string, how: How): void {
    this.num(FIELD.timeout, v, how, 5, 86_400, (n) => this.store.edit(FIELD.timeout, 'server', { requestTimeoutSecs: Math.round(n) }, how));
  }
  protected setMaxConcurrent(v: string, how: How): void {
    this.num(FIELD.concurrent, v, how, 1, 64, (n) => this.store.edit(FIELD.concurrent, 'server', { maxConcurrent: Math.round(n) }, how));
  }
  protected setIdleMinutes(key: 'idleUnloadSecs' | 'deepIdleSecs', v: string, how: How): void {
    const field = key === 'idleUnloadSecs' ? FIELD.idle : FIELD.deepIdle;
    this.num(field, v, how, 0.5, 1440, (n) => {
      const secs = Math.round(n * 60);
      this.store.edit(field, 'memory', key === 'idleUnloadSecs' ? { idleUnloadSecs: secs } : { deepIdleSecs: secs }, how);
    });
  }
  protected setPressure(v: string, how: How): void {
    this.num(FIELD.pressure, v, how, 1, 90, (n) => this.store.edit(FIELD.pressure, 'memory', { pressureAvailablePercent: Math.round(n) }, how));
  }
  protected setMinFree(v: string, how: How): void {
    this.num(FIELD.minFree, v, how, 0, 1024, (n) => this.store.edit(FIELD.minFree, 'memory', { minFreeBytes: Math.round(n * GB) }, how));
  }
  protected setMaxHot(v: string, how: How): void {
    this.num(FIELD.maxHot, v, how, 1, 16, (n) => this.store.edit(FIELD.maxHot, 'memory', { maxHotModels: Math.round(n) }, how));
  }
  protected setHotCap(v: string, how: How): void {
    this.num(FIELD.hotCap, v, how, 1, 1024, (n) => this.store.edit(FIELD.hotCap, 'memory', { hotCapBytes: Math.round(n * GB) }, how));
  }
  protected setCapMode(v: string): void {
    this.store.edit(FIELD.hotCap, 'memory', { hotCapBytes: v === 'fits' ? null : 16 * GB });
  }
  protected setLanPort(v: string, how: How): void {
    this.num(FIELD.lanPort, v, how, 1024, 65535, (n) => this.store.edit(FIELD.lanPort, 'network', { lanPort: Math.round(n) }, how));
  }
  protected setRetention(v: string, how: How): void {
    this.num(FIELD.retention, v, how, 1, 365, (n) => this.store.edit(FIELD.retention, 'privacy', { metricsRetentionDays: Math.round(n) }, how));
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
  protected setSchedule(v: string): void {
    if (v === 'daily' || v === 'weekly') void this.updates.setCheck(true, v);
  }
  protected readonly scheduleOptions: PdSelectOption[] = [
    { value: 'weekly', label: 'Every week' },
    { value: 'daily', label: 'Every day' },
  ];
  protected ttlChoices(k: 'embeddings' | 'responses') {
    return ttlOptions(this.ttl(k), k === 'embeddings');
  }
  private cacheField(k: 'embeddings' | 'responses', what: 'storage' | 'max' | 'expiry'): SettingField {
    const title = this.cacheKinds.find((c) => c.id === k)?.title ?? k;
    return { id: `cache-${k}-${what}`, label: what === 'max' ? `${title} size` : `${title} ${what}` };
  }
  protected setStorage(k: 'embeddings' | 'responses', v: string): void {
    if (v !== 'disk' && v !== 'memory') return;
    this.store.edit(this.cacheField(k, 'storage'), 'cache', k === 'embeddings' ? { embeddingsStorage: v } : { responsesStorage: v });
  }
  protected setMaxMib(k: 'embeddings' | 'responses', v: string, how: How): void {
    const field = { ...this.cacheField(k, 'max'), id: `cache-${k}-max` };
    this.num(field, v, how, 1, 1_048_576, (n) => this.store.edit(field, 'cache', k === 'embeddings' ? { embeddingsMaxMib: Math.round(n) } : { responsesMaxMib: Math.round(n) }, how));
  }
  protected setTtl(k: 'embeddings' | 'responses', v: string): void {
    const n = Number(v);
    if (Number.isFinite(n) && n >= 0) this.store.edit(this.cacheField(k, 'expiry'), 'cache', k === 'embeddings' ? { embeddingsTtlSecs: n } : { responsesTtlSecs: n });
  }
  protected setPromptCacheMode(v: string): void {
    this.store.edit(FIELD.promptCache, 'cache', { promptCacheMib: v === 'auto' ? null : 1024 });
  }
  protected setPromptCacheMib(v: string, how: How): void {
    this.num(FIELD.promptCache, v, how, 0, 1_048_576, (n) => this.store.edit(FIELD.promptCache, 'cache', { promptCacheMib: Math.round(n) }, how));
  }
  protected setKeepWarmDays(v: string, how: How): void {
    this.num(FIELD.keepWarm, v, how, 1, 3650, (n) => this.store.edit(FIELD.keepWarm, 'cache', { keepWarmDays: Math.round(n) }, how));
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
    const ok = await confirm(this.dialogs, {
      heading: `Clear ${what}?`,
      message: 'The entries are deleted now. Apps keep working; requests are answered by the model again until the cache refills.',
      confirm: { label: 'Clear cache', destructive: true },
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
