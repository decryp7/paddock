import { ChangeDetectionStrategy, Component, DestroyRef, OnInit, computed, inject, signal, viewChild } from '@angular/core';
import { MatButton } from '@angular/material/button';
import { SuTrayPopover } from '@decryp7/paddock-ui';
import { PdSelect, type PdSelectOption } from '@decryp7/paddock-ui/select';
import { PdWidgetArea, PdWidgetChooser, PdWidgetOption } from '@decryp7/paddock-ui/widgets';
import { BACKEND } from '../../core/backend';
import type { Edit, Placement, PopoverKind, PopoverWidget, VolumeChoice } from '../../core/generated/widgets';
import { PlatformService } from '../../core/platform';
import { Notifier } from '../../shared/notifier';
import { PopoverValues } from '../../state/popover-values';
import { GRADE_LABEL, GRADE_TONE } from '../../shared/labels';
import { HealthStore } from '../../state/health.store';
import { optionLabel } from '../widgets/option-label';
import { popoverMeta } from '../tray/widgets/popover-meta';
import { WidgetChoicesStore } from '../../state/widget-choices';
import { WidgetLayouts } from '../../state/widget-layouts';
import { DynoPopoverWidgets } from '../tray/widgets/popover-widgets';

const volumeKey = (v: VolumeChoice): string => (typeof v === 'string' ? v : `mount:${v.mount}`);
const volumeOf = (key: string): VolumeChoice => (key.startsWith('mount:') ? { mount: key.slice(6) } : key === 'system' ? 'system' : 'all');

/**
 * Settings > Menu bar > Popup widgets: the shared chooser over the popup's layout (family ADR 0035), with the volume choice
 * for a disk space widget and a live preview of the real popup. Every edit goes to the engine at once; Undo follows it.
 */
@Component({
  selector: 'dy-popup-widgets',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [MatButton, PdSelect, PdWidgetArea, PdWidgetChooser, PdWidgetOption, SuTrayPopover, DynoPopoverWidgets],
  template: `
    @if (view(); as v) {
      @if (choices.ready()) {
      <pd-widget-chooser [view]="v" [widgetFor]="widgetFor" [again]="again" [preview]="true" (edit)="edit($event)">
        <ng-template pdWidgetOption="diskSpace" [pdWidgetOptionOf]="sample" let-p>
          <pd-select [options]="volumeOptions()" [value]="volumeOf(p)" [ariaLabel]="label(p)" (valueChange)="setVolume(p, $event)" />
        </ng-template>
        <div slot="preview" class="preview" role="group" aria-label="Preview of the popup" inert>
          <pd-tray-popover label="Dyno popup preview" [platform]="platform">
            <div slot="header" class="head">
              <img src="brand/dyno-16.svg" alt="" width="16" height="16" />
              <strong>Dyno</strong>
              <span class="spacer"></span>
              @if (health.report(); as r) {
                <span class="grade-pill" [attr.data-tone]="gradeTone()"><strong class="num">{{ score(r.score) }}</strong> {{ grade() }}</span>
              }
            </div>
            <pd-widget-area [placements]="v.layout.placements" [catalogue]="v.catalogue" [values]="values.values()" [templates]="tpl()?.templates() ?? []" [meta]="meta" />
            <div slot="footer" class="foot"><span class="spacer"></span><button type="button" matButton="filled" tabindex="-1">Open Dyno</button></div>
          </pd-tray-popover>
        </div>
      </pd-widget-chooser>
      }
      <dy-popover-widgets />
    } @else {
      <p class="faint" role="status">Reading the popup layout…</p>
    }
  `,
  styles: `
    :host { display: block; }
    .preview { max-width: 100%; }
    /* The volume choice is compact: it sits beside the row's text and menu without crowding them. */
    pd-select { width: 11rem; min-width: 0; }
    .head { display: flex; align-items: center; gap: var(--sp-2); flex: 1; }
    .grade-pill { display: inline-flex; align-items: center; gap: 6px; height: 22px; padding: 0 var(--sp-2); border-radius: var(--r-pill); font-size: var(--fs-xs); background: var(--ok-soft); color: var(--ok); }
    .grade-pill[data-tone='warn'] { background: var(--warn-soft); color: var(--warn); }
    .grade-pill[data-tone='bad'] { background: var(--bad-soft); color: var(--bad); }
    .foot { display: flex; flex: 1; } .spacer { flex: 1; }
    .faint { color: var(--text-3); }
  `,
})
export class PopupWidgets implements OnInit {
  private readonly layouts = inject(WidgetLayouts);
  private readonly notifier = inject(Notifier);
  private readonly backend = inject(BACKEND);
  protected readonly values = inject(PopoverValues);
  protected readonly platform = (() => {
    const p = inject(PlatformService).platform;
    return p === 'browser' ? 'macos' : p;
  })();
  protected readonly view = this.layouts.popover;
  /** The type witness of the option templates (`pdWidgetOptionOf`). */
  protected readonly sample: PopoverWidget | null = null;
  protected readonly choices = inject(WidgetChoicesStore);
  protected readonly meta = popoverMeta;
  protected readonly health = inject(HealthStore);
  protected readonly score = (v: number) => Math.round(v).toString();
  protected readonly grade = computed(() => (this.health.report() ? GRADE_LABEL[this.health.report()!.grade] : ''));
  protected readonly gradeTone = computed(() => (this.health.report() ? GRADE_TONE[this.health.report()!.grade] : 'accent'));
  protected readonly widgetFor = (kind: PopoverKind): PopoverWidget => this.choices.popoverDefault(kind);
  protected readonly again = { diskSpace: 'Add volume' };
  protected readonly tpl = viewChild(DynoPopoverWidgets);
  private readonly volumes = signal<PdSelectOption[]>([]);
  protected readonly volumeOptions = computed<PdSelectOption[]>(() => [{ value: 'all', label: 'All volumes' }, { value: 'system', label: 'System volume' }, ...this.volumes()]);

  constructor() {
    const timer = setInterval(() => void this.values.preview(), 5_000);
    inject(DestroyRef).onDestroy(() => clearInterval(timer));
  }

  ngOnInit(): void {
    void this.health.loadReport();
    void this.layouts.use('popover');
    void this.choices.load();
    void this.values.preview();
    void this.backend
      .diskVolumes()
      .then((list) => this.volumes.set(list.map((v) => ({ value: `mount:${v.mount}`, label: v.name }))))
      .catch(() => undefined);
  }

  protected label(p: Placement<PopoverWidget>): string {
    return optionLabel('Volume', this.view(), p);
  }

  protected volumeOf(p: Placement<PopoverWidget>): string {
    const w = p.widget;
    return typeof w === 'object' && 'diskSpace' in w ? volumeKey(w.diskSpace.volume) : 'all';
  }

  protected async edit(e: Edit<PopoverWidget>): Promise<void> {
    const failed = await this.layouts.edit('popover', e);
    if (failed) this.notifier.error(failed);
    else void this.values.preview();
  }

  protected setVolume(p: Placement<PopoverWidget>, key: string): void {
    void this.edit({ replace: { id: p.id, widget: { diskSpace: { volume: volumeOf(key) } } } });
  }
}
