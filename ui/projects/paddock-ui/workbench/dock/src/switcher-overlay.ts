import { ChangeDetectionStrategy, Component, computed, inject } from '@angular/core';
import { WorkbenchDock } from './dock.service';
import { selected, selectId } from './switcher';

/**
 * The IDE navigator (Ctrl+Tab, VS style): the open tool windows and documents in most-recently-used order. Hold
 * Ctrl and press Tab to move, release Ctrl to switch; arrows, Enter, Escape and the mouse work too.
 */
@Component({
  selector: 'pd-dock-switcher',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @if (dock.switcher()) {
      <div class="pd-dock-switcher-backdrop"  (pointerdown)="dock.cancelSwitch()"></div>
      <div class="pd-dock-switcher" role="dialog" aria-label="Switch to panel" i18n-aria-label="@@pd.wb.dock.switcher.label" (pointerdown)="$event.stopPropagation()">
        <div class="cols">
          @for (col of columns(); track col.title) {
            <div class="col">
              <h2 [id]="'pd-dock-sw-' + col.kind">{{ col.title }}</h2>
              <ul role="listbox" [attr.aria-labelledby]="'pd-dock-sw-' + col.kind" [attr.aria-activedescendant]="current()?.kind === col.kind ? activeId() : null">
                @for (item of col.items; track item.id) {
                  <li
                    role="option"
                    [id]="'pd-dock-sw-item-' + item.id"
                    [class.on]="current()?.id === item.id"
                    [attr.aria-selected]="current()?.id === item.id"
                    (pointerenter)="pick(item.id)"
                    (click)="pick(item.id); dock.commitSwitch()"
                  >
                    {{ item.title }}
                  </li>
                } @empty {
                  <li class="none" role="presentation" i18n="@@pd.wb.dock.switcher.none">None</li>
                }
              </ul>
            </div>
          }
        </div>
        <p class="hint" i18n="@@pd.wb.dock.switcher.hint">Hold Ctrl and press Tab to move; release Ctrl to switch.</p>
      </div>
    }
  `,
  styles: `
    .pd-dock-switcher-backdrop { position: fixed; inset: 0; z-index: 200; }
    .pd-dock-switcher {
      position: fixed; z-index: 201; inset-inline-start: 50%; top: 22%; transform: translateX(-50%); min-width: 460px; max-width: 80vw;
      background: var(--pd-surface-3); color: var(--pd-text); border: 1px solid var(--pd-border-strong); border-radius: 8px;
      box-shadow: var(--pd-shadow-2); padding: 12px 14px;
    }
    .cols { display: grid; grid-template-columns: 1fr 1fr; gap: 16px; }
    h2 { margin: 0 0 6px; font-size: var(--pd-text-label); font-weight: 600; color: var(--pd-text-muted); }
    ul { list-style: none; margin: 0; padding: 0; max-height: 50vh; overflow: auto; }
    li { padding: 4px 8px; border-radius: 4px; font-size: var(--pd-text-body); cursor: pointer; }
    li.on { background: var(--pd-accent); color: var(--pd-on-accent); }
    li.none { color: var(--pd-text-subtle); cursor: default; }
    .hint { margin: 10px 0 0; font-size: var(--pd-text-label); color: var(--pd-text-muted); }
  `,
})
export class DockSwitcher {
  protected readonly dock = inject(WorkbenchDock);
  protected readonly current = computed(() => {
    const s = this.dock.switcher();
    return s ? selected(s) : null;
  });
  protected readonly activeId = computed(() => (this.current() ? `pd-dock-sw-item-${this.current()!.id}` : null));
  protected readonly columns = computed(() => {
    const items = this.dock.switcher()?.items ?? [];
    return [
      { kind: 'tool', title: $localize`:@@pd.wb.dock.switcher.tools:Active tool windows`, items: items.filter((i) => i.kind === 'tool') },
      { kind: 'document', title: $localize`:@@pd.wb.dock.switcher.documents:Active documents`, items: items.filter((i) => i.kind === 'document') },
    ];
  });

  protected pick(id: string): void {
    const s = this.dock.switcher();
    if (s) this.dock.switcher.set(selectId(s, id));
  }
}
