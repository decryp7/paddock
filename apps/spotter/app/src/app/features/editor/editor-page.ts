import { routeParam } from '../../core/route-param';
import { ChangeDetectionStrategy, Component, ElementRef, Injector, afterNextRender, computed, effect, inject, linkedSignal, untracked } from '@angular/core';
import { MatIcon } from '@angular/material/icon';
import { MatStep, MatStepContent, MatStepper, MatStepperIcon } from '@angular/material/stepper';
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

/** The heading each step moves focus to when it opens (the first heading of the step's content). */
const STEP_HEADING: Record<EditorStep, string> = { source: 'src-h', configure: 'cfg-h', versions: 'ver-h', notify: 'sch-h', review: 'rev-h' };

/** The notification modes in words (the review shows these, not the wire ids). */
const NOTIFY_MODE_LABEL: Record<NotifyMode, string> = { 'every-new': 'Every new release', 'newer-than-installed': 'Newer than installed', off: 'Nothing' };

/** The add/edit wizard (a linear `mat-stepper`): source, configure (with a live test), versions, notifications and actions, review. */
@Component({
  selector: 'sp-editor-page',
  imports: [MatIcon, MatStepper, MatStep, MatStepContent, MatStepperIcon, RouterLink, Panel, FieldForm, PreviewPanel, SourceStep, ExtensionChips, PatternHelper, ScheduleEditor],
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

  private readonly host = inject<ElementRef<HTMLElement>>(ElementRef);
  private readonly injector = inject(Injector);

  protected readonly schemes = SCHEME_OPTIONS;
  protected readonly draft = this.store.draft;
  /** Whether a step's own input is complete (UI convenience only: the engine re-validates on test and save). */
  private valid(step: EditorStep): boolean {
    switch (step) {
      case 'source':
        return this.store.kind() !== null;
      case 'configure':
        return this.store.kind() !== null && this.store.missing().length === 0;
      case 'versions':
        return this.draft().versions !== 'custom' || !!(this.draft().versionPattern ?? '').trim();
      default:
        return true;
    }
  }

  protected readonly canNext = computed(() => this.store.step() !== 'source' && this.valid(this.store.step()));
  /** The furthest step reached; the stepper opens steps up to it, and the next one once the current is complete. */
  private readonly furthest = linkedSignal<number, number>({ source: this.store.stepIndex, computation: (i, prev) => Math.max(i, prev?.value ?? 0) });
  protected readonly download = computed(() => findAction(this.draft().actions, 'download'));
  protected readonly command = computed(() => findAction(this.draft().actions, 'run-command'));
  protected readonly openLocation = computed(() => findAction(this.draft().actions, 'open-location'));
  protected readonly formState = computed<Loadable<unknown>>(() => (this.id() ? this.store.loadedDraft() : { value: true, loading: false, error: null }));

  constructor() {
    void this.store.loadCatalog();
    effect(() => {
      const id = this.id();
      if (id) untracked(() => void this.store.loadWatch(id));
    });
    // Focus follows the step: the new step's heading (not on first render, so opening the page does not steal focus).
    let first = true;
    effect(() => {
      const step = this.store.step();
      if (first) {
        first = false;
        return;
      }
      untracked(() => afterNextRender(() => this.host.nativeElement.querySelector<HTMLElement>(`#${STEP_HEADING[step]}`)?.focus(), { injector: this.injector }));
    });
  }

  /**
   * A step counts as done when it was reached and it and every step before it are complete, so clearing a required field shuts the later
   * headers again. Linear mode then lets the next step open; Material marks that next step `aria-disabled` until it is done, although
   * it opens (its own default for a step that is neither open nor done).
   */
  protected done(index: number): boolean {
    return index <= this.furthest() && STEPS.slice(0, index + 1).every((s) => this.valid(s.id));
  }

  /** In edit mode (the route has an id: known before the watch has loaded) the stepper has no Source step, so its indexes start one step later. */
  private get offset(): number {
    return this.id() ? 1 : 0;
  }

  protected readonly openIndex = computed(() => Math.max(0, this.store.stepIndex() - this.offset));

  /** The source in words, for the page head (editing) and the review: the kind and its main address. */
  protected readonly sourceSummary = computed(() => {
    const v = this.draft().source.values;
    const main = v['url'] ?? v['repo'] ?? v['path'] ?? v['image'] ?? v['package'] ?? v['project'] ?? v['crate'] ?? '';
    const label = this.store.kind()?.label ?? '';
    return label && main ? `${label}: ${main}` : label || main;
  });
  protected readonly schemeLabel = computed(() => SCHEME_OPTIONS.find((o) => o.value === this.draft().versions)?.label ?? this.draft().versions);
  protected readonly modeLabel = computed(() => NOTIFY_MODE_LABEL[this.draft().notify.mode]);
  /** The notify modes in order, for the segmented buttons (the same words as the review). */
  protected readonly notifyModes = (Object.keys(NOTIFY_MODE_LABEL) as NotifyMode[]).map((value) => ({ value, label: NOTIFY_MODE_LABEL[value] }));

  /** The stepper opened a step (header click or keyboard): the store follows. */
  protected onStep(index: number): void {
    const to = STEPS[index + this.offset]?.id;
    if (to && to !== this.store.step()) this.store.setStep(to);
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
