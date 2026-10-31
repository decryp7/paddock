import { ChangeDetectionStrategy, Component, computed, inject } from '@angular/core';
import { SuTrayPopover } from '@decryp7/paddock-ui';
import { PdWidgetArea, PdWidgetChooser } from '@decryp7/paddock-ui/widgets';
import type { PopoverWidget } from '../../core/generated/widgets';
import type { PopoverEdit, PopoverView } from '../../core/widgets.types';
import { Panel } from '../../shared/ui/panel';
import { TrayWidgetsStore } from '../../state/tray-widgets.store';
import type { Loadable } from '../../state/loadable';
import { WidgetSet } from '../tray/widget-set';

/**
 * Spotter's "Tray popup" page of the Settings window (family ADR 0035 section 4): the shared widget chooser on the
 * engine's layout, with a live preview of the real popup beside it. Every change is one edit sent to the engine; the answer
 * is the new layout, so this page holds no layout logic.
 */
@Component({
  selector: 'sp-tray-widgets-page',
  imports: [PdWidgetChooser, PdWidgetArea, SuTrayPopover, Panel, WidgetSet],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="page narrow">
      <header class="page-head">
        <h1>Tray popup</h1>
        <p class="help">Choose what the popup shows when you click Spotter's tray icon, and in which order.</p>
      </header>
      <sp-panel [state]="state()" label="the popup's widgets" (retry)="store.load()">
        @if (store.view(); as view) {
          @if (store.error(); as err) {
            <p class="problem" role="alert">{{ err.message }}</p>
          }
          <pd-widget-chooser [view]="view" [widgetFor]="widgetFor" [preview]="true" (edit)="edit($event)">
            <div slot="preview">
              <sp-widget-set #set />
              <pd-tray-popover label="Preview of the popup" platform="macos" inert>
                <div slot="header" class="head"><strong>Spotter</strong></div>
                <pd-widget-area [placements]="view.layout.placements" [catalogue]="view.catalogue" [values]="store.values()" [templates]="set.widgets()" />
              </pd-tray-popover>
            </div>
          </pd-widget-chooser>
        }
      </sp-panel>
    </div>
  `,
  styles: `
    .narrow {
      max-width: 880px;
    }
    .help {
      margin: 0;
      color: var(--pd-text-muted);
    }
    .problem {
      margin: 0 0 var(--pd-space-3);
      padding: var(--pd-space-2) var(--pd-space-3);
      border-radius: var(--pd-radius-card);
      background: var(--pd-danger-tint);
    }
    .head {
      display: flex;
      align-items: center;
      gap: var(--pd-space-2);
    }
  `,
})
export class TrayWidgetsPage {
  protected readonly store = inject(TrayWidgetsStore);
  /** What the panel shows: the last good layout, whether a read runs and its error. */
  protected readonly state = computed<Loadable<PopoverView>>(() => ({ value: this.store.view(), loading: this.store.loading(), error: this.store.view() ? null : this.store.error() }));
  /** A widget of a kind: Spotter's widgets have no options, so the widget is the kind. */
  protected readonly widgetFor = (kind: PopoverWidget): PopoverWidget => kind;

  constructor() {
    this.store.start();
  }

  protected edit(edit: PopoverEdit): void {
    void this.store.edit(edit);
  }
}
