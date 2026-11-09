import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { SuTrayPopover } from '@decryp7/paddock-ui';
import { PdWidgetArea, PdWidgetChooser } from '@decryp7/paddock-ui/widgets';
import type { PopoverWidget } from '../../core/generated/widgets';
import type { PopoverEdit } from '../../core/widgets.types';
import { Panel } from '../../shared/ui/panel';
import { SettingsHead } from '../../shared/ui/settings-head';
import { StatusPill } from '../tray/status-pill';
import { TrayWidgetsStore } from '../tray/tray-widgets.store';
import { WidgetSet } from '../tray/widget-set';

/**
 * Pitwall's "Tray popup" page of the Settings window (family ADR 0035 section 4): the shared widget chooser on the app's layout,
 * with a live preview of the real popup beside it. Every change is one edit sent to the app; the answer is the new layout, so
 * this page holds no layout logic.
 */
@Component({
  selector: 'pw-tray-widgets-page',
  imports: [PdWidgetChooser, PdWidgetArea, SuTrayPopover, Panel, SettingsHead, WidgetSet, StatusPill],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="page">
      <pw-settings-head heading="Tray popup" titleId="pw-tray-title" help="Choose what the popup shows when you click Pitwall's tray icon, and in which order." />
      <pw-panel [loading]="store.loading()" [error]="store.view() ? null : store.error()" [ready]="!!store.view()" (retry)="store.load()">
        @if (store.view(); as view) {
          @if (store.error(); as err) {
            <p class="problem" role="alert">{{ err.message }}</p>
          }
          <pd-widget-chooser [view]="view" [widgetFor]="widgetFor" [preview]="true" (edit)="edit($event)">
            <div slot="preview">
              <pw-widget-set #set />
              <pd-tray-popover label="Preview of the popup" platform="macos" inert>
                <div slot="header" class="head"><img src="brand/pitwall-macos.svg" alt="" width="16" height="16" /><strong>Pitwall</strong><pw-status-pill [status]="store.header()?.status ?? null" /></div>
                <pd-widget-area [placements]="view.layout.placements" [catalogue]="view.catalogue" [values]="store.values()" [templates]="set.widgets()" [meta]="set.meta" />
              </pd-tray-popover>
            </div>
          </pd-widget-chooser>
        }
      </pw-panel>
    </div>
  `,
  styles: `
    .page {
      display: grid;
      gap: var(--pd-space-3);
      max-width: 1120px;
    }
    .problem {
      margin: 0 0 var(--pd-space-3);
      padding: var(--pd-space-2) var(--pd-space-3);
      border-radius: var(--pd-radius-card);
      background: var(--pd-danger-tint);
    }
    .head {
      display: flex;
      flex: 1;
      width: 100%;
      align-items: center;
      gap: var(--pd-space-2);
    }
  `,
})
export class TrayWidgetsPage {
  protected readonly store = inject(TrayWidgetsStore);
  /** A widget of a kind: Pitwall's widgets have no options, so the widget is the kind. */
  protected readonly widgetFor = (kind: PopoverWidget): PopoverWidget => kind;

  constructor() {
    this.store.start();
  }

  protected edit(edit: PopoverEdit): void {
    void this.store.edit(edit);
  }
}
