import { routeParam } from '../../core/route-param';
import { ChangeDetectionStrategy, Component, computed, effect, inject, untracked } from '@angular/core';
import { MatIcon } from '@angular/material/icon';
import { Router, RouterLink } from '@angular/router';
import { BACKEND } from '../../core/backend';
import type { ChannelKind, LatestBy, NotifyMode, VersionSchemeId, WatchAction } from '../../core/api.types';
import { SCHEME_OPTIONS } from '../../shared/labels';
import { Panel } from '../../shared/ui/panel';
import type { Loadable } from '../../state/loadable';
import { Notifier } from '../../state/notifier';
import { findAction, parseArgs, replaceAction, toggleAction, type ActionKind } from './draft-utils';
import { EditorStore, STEPS, type EditorStep } from './editor.store';
import { ExtensionChips } from './extension-chips';
import { FieldForm, type FieldChange } from './field-form';
import { PatternHelper } from './pattern-helper';
import { PreviewPanel } from './preview-panel';
import { ScheduleEditor } from './schedule-editor';
import { SourceStep } from './source-step';

/** The add/edit wizard: source, configure (with a live test), versions, notifications and actions, review. */
@Component({
  selector: 'sp-editor-page',
  imports: [MatIcon, RouterLink, Panel, FieldForm, PreviewPanel, SourceStep, ExtensionChips, PatternHelper, ScheduleEditor],
  providers: [EditorStore],
  changeDetection: ChangeDetectionStrategy.OnPush,
  templateUrl: './editor-page.html',
  styleUrl: './editor-page.scss',
})
export class EditorPage {
  /** From the route (`/watches/:id/edit`); absent when adding. */
  readonly id = routeParam('id');

  protected readonly store = inject(EditorStore);
  private readonly router = inject(Router);
  private readonly toast = inject(Notifier);
  private readonly backend = inject(BACKEND);

  protected readonly steps = STEPS;
  protected readonly schemes = SCHEME_OPTIONS;
  protected readonly draft = this.store.draft;
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

  protected setScheme(versions: string): void {
    this.store.update((d) => ({ ...d, versions: versions as VersionSchemeId }));
  }

  protected setLatestBy(by: LatestBy): void {
    this.store.setLatestBy(by);
  }

  protected isTracked(id: string): boolean {
    const only = this.draft().tracking.products.only;
    return only.length === 0 || only.includes(id);
  }

  protected setting(id: string) {
    return this.draft().tracking.products.settings[id];
  }

  protected readonly channelKinds: { value: ChannelKind; label: string }[] = [
    { value: 'ntfy', label: 'ntfy' },
    { value: 'webhook', label: 'Webhook' },
    { value: 'email', label: 'Email' },
  ];

  protected setMode(mode: NotifyMode): void {
    this.store.update((d) => ({ ...d, notify: { ...d.notify, mode } }));
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
