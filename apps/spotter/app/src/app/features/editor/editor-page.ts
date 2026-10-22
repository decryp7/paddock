import { ChangeDetectionStrategy, Component, computed, effect, inject, input, untracked } from '@angular/core';
import { MatIcon } from '@angular/material/icon';
import { Router, RouterLink } from '@angular/router';
import { BACKEND } from '../../core/backend';
import type { ChannelKind, NotifyMode, VersionSchemeId, WatchAction } from '../../core/api.types';
import { formatInterval } from '../../shared/format';
import { INTERVAL_PRESETS, SCHEME_OPTIONS } from '../../shared/labels';
import { Panel } from '../../shared/ui/panel';
import type { Loadable } from '../../state/loadable';
import { ToastStore } from '../../state/toast.store';
import { findAction, parseArgs, replaceAction, toggleAction, type ActionKind } from './draft-utils';
import { EditorStore, STEPS, type EditorStep } from './editor.store';
import { FieldForm, type FieldChange } from './field-form';
import { PreviewPanel } from './preview-panel';
import { SourceStep } from './source-step';

/** The add/edit wizard: source, configure (with a live test), versions, notifications and actions, review. */
@Component({
  selector: 'sp-editor-page',
  imports: [MatIcon, RouterLink, Panel, FieldForm, PreviewPanel, SourceStep],
  providers: [EditorStore],
  changeDetection: ChangeDetectionStrategy.OnPush,
  templateUrl: './editor-page.html',
  styleUrl: './editor-page.scss',
})
export class EditorPage {
  /** From the route (`/watches/:id/edit`); absent when adding. */
  readonly id = input<string>();

  protected readonly store = inject(EditorStore);
  private readonly router = inject(Router);
  private readonly toast = inject(ToastStore);
  private readonly backend = inject(BACKEND);

  protected readonly steps = STEPS;
  protected readonly schemes = SCHEME_OPTIONS;
  protected readonly intervals = INTERVAL_PRESETS;
  protected readonly interval = formatInterval;
  protected readonly draft = this.store.draft;
  protected readonly intervalOptions = computed(() => {
    const secs = this.draft().intervalSecs;
    return INTERVAL_PRESETS.some((p) => p.secs === secs) ? INTERVAL_PRESETS : [...INTERVAL_PRESETS, { secs, label: formatInterval(secs) }].sort((a, b) => a.secs - b.secs);
  });
  protected readonly canNext = computed(() => {
    const s = this.store.step();
    if (s === 'source') return false;
    if (s === 'configure') return this.store.missing().length === 0;
    if (s === 'versions') return this.draft().versions !== 'custom' || !!(this.draft().versionPattern ?? '').trim();
    return true;
  });
  protected readonly download = computed(() => findAction(this.draft().actions, 'download'));
  protected readonly command = computed(() => findAction(this.draft().actions, 'run-command'));
  protected readonly openLocation = computed(() => findAction(this.draft().actions, 'open-location'));
  protected readonly formState = computed<Loadable<unknown>>(() => (this.id() ? this.store.loadedDraft() : { value: true, loading: false, error: null }));
  protected readonly reachable = computed(() => this.store.stepIndex());

  constructor() {
    void this.store.loadCatalog();
    effect(() => {
      const id = this.id();
      if (id) untracked(() => void this.store.loadWatch(id));
    });
  }

  protected go(step: EditorStep, index: number): void {
    // Steps already visited can be revisited; the source choice must come first.
    if (index <= this.reachable() && (this.store.editing() ? step !== 'source' : true)) this.store.setStep(step);
  }

  protected value(e: Event): string {
    return (e.target as HTMLInputElement | HTMLSelectElement | HTMLTextAreaElement).value;
  }

  protected checked(e: Event): boolean {
    return (e.target as HTMLInputElement).checked;
  }

  protected onField(c: FieldChange): void {
    this.store.setValue(c.key, c.value);
  }

  protected setScheme(e: Event): void {
    this.store.update((d) => ({ ...d, versions: this.value(e) as VersionSchemeId }));
  }

  protected setInterval(secs: number): void {
    if (Number.isFinite(secs) && secs >= 60) this.store.update((d) => ({ ...d, intervalSecs: Math.round(secs) }));
  }

  protected setMode(e: Event): void {
    this.store.update((d) => ({ ...d, notify: { ...d.notify, mode: this.value(e) as NotifyMode } }));
  }

  protected setQuiet(on: boolean): void {
    this.store.update((d) => ({ ...d, notify: { ...d.notify, quietHours: on ? { start: '22:00', end: '07:00' } : null } }));
  }

  protected setQuietTime(which: 'start' | 'end', value: string): void {
    this.store.update((d) => (d.notify.quietHours ? { ...d, notify: { ...d.notify, quietHours: { ...d.notify.quietHours, [which]: value } } } : d));
  }

  protected setAction(kind: ActionKind, on: boolean): void {
    this.store.update((d) => ({ ...d, actions: toggleAction(d.actions, kind, on) }));
  }

  protected patchAction(action: WatchAction): void {
    this.store.update((d) => ({ ...d, actions: replaceAction(d.actions, action) }));
  }

  protected async browseFolder(): Promise<void> {
    const folder = await this.backend.chooseFolder().catch(() => null);
    const a = this.download();
    if (folder && a) this.patchAction({ ...a, folder });
  }

  protected async browseProgram(): Promise<void> {
    const program = await this.backend.chooseProgram().catch(() => null);
    const a = this.command();
    if (program && a) this.patchAction({ ...a, program });
  }

  protected args(text: string): string[] {
    return parseArgs(text);
  }

  protected addChannel(): void {
    this.store.update((d) => ({ ...d, channels: [...d.channels, { kind: 'ntfy', target: '' }] }));
  }

  protected setChannel(i: number, patch: { kind?: ChannelKind; target?: string }): void {
    this.store.update((d) => ({ ...d, channels: d.channels.map((c, j) => (j === i ? { ...c, ...patch } : c)) }));
  }

  protected removeChannel(i: number): void {
    this.store.update((d) => ({ ...d, channels: d.channels.filter((_, j) => j !== i) }));
  }

  protected async save(): Promise<void> {
    const saved = await this.store.save();
    if (!saved) return;
    this.toast.success(this.store.editing() ? 'Watch saved' : 'Watch added', saved.name);
    await this.router.navigate(['/watches', saved.id]);
  }
}
