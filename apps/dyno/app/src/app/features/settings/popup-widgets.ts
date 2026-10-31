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
import { WidgetLayouts } from '../../state/widget-layouts';
import { DynoPopoverWidgets } from '../tray/widgets/popover-widgets';

/** A new popup widget of `kind` with its default options (the engine validates it). */
export function newPopoverWidget(kind: PopoverKind): PopoverWidget {
  return kind === 'diskSpace' ? { diskSpace: { volume: 'all' } } : kind;
}

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
      <pd-widget-chooser [view]="v" [widgetFor]="widgetFor" [again]="again" [preview]="true" (edit)="edit($event)">
        <ng-template pdWidgetOption="diskSpace" let-p>
          <pd-select [options]="volumeOptions()" [value]="volumeOf(p)" [ariaLabel]="'Volume of ' + p.id" (valueChange)="setVolume(p, $event)" />
        </ng-template>
        <div slot="preview" class="preview" role="group" aria-label="Preview of the popup" inert>
          <pd-tray-popover label="Dyno popup preview" [platform]="platform">
            <div slot="header" class="head"><strong>Dyno</strong></div>
            <pd-widget-area [placements]="v.layout.placements" [catalogue]="v.catalogue" [values]="values.values()" [templates]="tpl()?.templates() ?? []" />
            <div slot="footer" class="foot"><span class="spacer"></span><button type="button" matButton="filled" tabindex="-1">Open Dyno</button></div>
          </pd-tray-popover>
        </div>
      </pd-widget-chooser>
      <dy-popover-widgets />
    } @else {
      <p class="faint" role="status">Reading the popup layout…</p>
    }
  `,
  styles: `
    :host { display: block; }
    .preview { max-width: 100%; }
    .head { display: flex; align-items: center; gap: var(--sp-2); }
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
  protected readonly widgetFor = newPopoverWidget;
  protected readonly again = { diskSpace: 'Add volume' };
  protected readonly tpl = viewChild(DynoPopoverWidgets);
  private readonly volumes = signal<PdSelectOption[]>([]);
  protected readonly volumeOptions = computed<PdSelectOption[]>(() => [{ value: 'all', label: 'All volumes' }, { value: 'system', label: 'System volume' }, ...this.volumes()]);

  constructor() {
    const timer = setInterval(() => void this.values.refresh(), 5_000);
    inject(DestroyRef).onDestroy(() => clearInterval(timer));
  }

  ngOnInit(): void {
    void this.layouts.use('popover');
    void this.values.start();
    void this.backend
      .diskVolumes()
      .then((list) => this.volumes.set(list.map((v) => ({ value: `mount:${v.mount}`, label: v.name }))))
      .catch(() => undefined);
  }

  protected volumeOf(placement: Placement<unknown>): string {
    const p = placement as Placement<PopoverWidget>;
    const w = p.widget;
    return typeof w === 'object' && 'diskSpace' in w ? volumeKey(w.diskSpace.volume) : 'all';
  }

  protected async edit(e: Edit<PopoverWidget>): Promise<void> {
    const failed = await this.layouts.edit('popover', e);
    if (failed) this.notifier.error(failed);
    else void this.values.refresh();
  }

  protected setVolume(placement: Placement<unknown>, key: string): void {
    const p = placement as Placement<PopoverWidget>;
    void this.edit({ replace: { id: p.id, widget: { diskSpace: { volume: volumeOf(key) } } } });
  }
}
