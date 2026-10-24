import { ChangeDetectionStrategy, Component, input } from '@angular/core';

/**
 * The title bar of a frame (plan §5.5; shared by the pages frame and the workbench): a leading slot (the sidebar
 * button), the app's name, the menu bar (the default slot) and a trailing slot, over a drag region for a window without
 * an OS title bar (`data-tauri-drag-region`). On macOS with an overlay title bar the app sets
 * `--pd-titlebar-inset-start` to keep clear of the window buttons. Narrow windows drop the name first (container query).
 */
@Component({
  selector: 'pd-title-bar',
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: { 'data-tauri-drag-region': '' },
  template: `
    <div class="lead"><ng-content select="[slot=lead]" /></div>
    <span class="name" data-tauri-drag-region data-testid="app-name">{{ appName() }}</span>
    <div class="menus"><ng-content /></div>
    <div class="drag" data-tauri-drag-region></div>
    <div class="end"><ng-content select="[slot=end]" /></div>
  `,
  styles: `
    :host { container: pd-title-bar / inline-size; display: flex; align-items: center; gap: var(--pd-space-2); min-width: 0; height: var(--pd-titlebar-height, 36px); padding: 0 var(--pd-space-2) 0 calc(var(--pd-space-2) + var(--pd-titlebar-inset-start, 0px)); }
    .lead, .end { display: flex; align-items: center; gap: var(--pd-space-1); flex: none; }
    .lead:empty, .end:empty { display: none; }
    .name { flex: none; font-weight: var(--pd-weight-semibold); white-space: nowrap; }
    .menus { display: flex; align-items: center; min-width: 0; overflow-x: auto; scrollbar-width: none; }
    .drag { flex: 1; align-self: stretch; min-width: 0; }
    @container pd-title-bar (max-width: 420px) { .name { display: none; } }
  `,
})
export class PdTitleBar {
  /** The app's name ("Pitwall"). */
  readonly appName = input.required<string>();
}
