import { ChangeDetectionStrategy, Component, input } from '@angular/core';
import { PdPathText } from '@decryp7/paddock-ui';

/**
 * The title bar of a frame (plan §5.5; shared by the pages frame and the workbench): a leading slot (the sidebar
 * button), the app's name, the menu bar (the default slot) and a trailing slot, over a drag region for a window without
 * an OS title bar (`data-tauri-drag-region`). 40 px high (`--pd-titlebar-height`; design guide §2). On macOS with an
 * overlay title bar `--pd-titlebar-inset-start` keeps clear of the window buttons (the pages frame sets 78 px there). The
 * name truncates (full in its tooltip) before the menus do.
 */
@Component({
  selector: 'pd-title-bar',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [PdPathText],
  host: { 'data-tauri-drag-region': '' },
  template: `
    <div class="pd-lead"><ng-content select="[slot=lead]" /></div>
    <span class="pd-name" data-tauri-drag-region data-testid="app-name"><pd-path-text [value]="appName()" [plain]="true" clip="end" /></span>
    <div class="pd-menus"><ng-content /></div>
    <div class="pd-drag" data-tauri-drag-region></div>
    <div class="pd-end"><ng-content select="[slot=end]" /></div>
  `,
  styles: `
    :host { display: flex; align-items: center; gap: var(--pd-space-2); min-width: 0; height: var(--pd-titlebar-height, 40px); padding: 0 var(--pd-space-2) 0 calc(var(--pd-space-2) + var(--pd-titlebar-inset-start, 0px)); }
    .pd-lead, .pd-end { display: flex; align-items: center; gap: var(--pd-space-1); flex: none; }
    .pd-lead:empty, .pd-end:empty { display: none; }
    /* The name gives way first (it shrinks 100 times as fast as the menus), so every menu stays whole while the name is cut to its ellipsis. */
    .pd-name { flex: 0 100 auto; min-width: 3ch; overflow: hidden; font-weight: var(--pd-weight-semibold); white-space: nowrap; }
    /* The name is shortened by measuring (one ellipsis, never after a space), and a click on it drags the window. */
    .pd-name pd-path-text { pointer-events: none; }
    .pd-menus { display: flex; align-items: center; flex: 0 1 auto; min-width: 0; max-width: 100%; overflow-x: auto; scrollbar-width: none; }
    .pd-drag { flex: 1; align-self: stretch; min-width: 0; }
  `,
})
export class PdTitleBar {
  /** The app's name ("Pitwall"). */
  readonly appName = input.required<string>();
}
