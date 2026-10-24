import { inject, Injectable } from '@angular/core';
import { WorkbenchDock } from './dock.service';

/**
 * Window-level keys of the dock that a key binding cannot express: the Ctrl+Tab navigator (held Ctrl, Tab cycles,
 * releasing Ctrl switches; Ctrl+Tab on macOS too, like VS) and its keys while it is open. Every other dock key is a
 * contributed keybinding (`pd.wb.dock.*`), dispatched by the shell's keymap.
 */
@Injectable()
export class DockKeys {
  private readonly dock = inject(WorkbenchDock);

  listen(target: Document | HTMLElement): () => void {
    const down = (e: Event) => this.keydown(e as KeyboardEvent);
    const up = (e: Event) => this.keyup(e as KeyboardEvent);
    target.addEventListener('keydown', down, true);
    target.addEventListener('keyup', up, true);
    return () => {
      target.removeEventListener('keydown', down, true);
      target.removeEventListener('keyup', up, true);
    };
  }

  keydown(e: KeyboardEvent): void {
    if (e.key === 'Tab' && e.ctrlKey && !e.altKey && !e.metaKey) {
      e.preventDefault();
      e.stopPropagation();
      this.dock.switch(e.shiftKey);
      return;
    }
    if (!this.dock.switcher()) return;
    const step: Record<string, number> = { ArrowDown: 1, ArrowRight: 1, ArrowUp: -1, ArrowLeft: -1 };
    if (e.key === 'Escape') this.dock.cancelSwitch();
    else if (e.key === 'Enter') this.dock.commitSwitch();
    else if (step[e.key]) this.dock.switch((step[e.key] ?? 0) < 0);
    else return;
    e.preventDefault();
    e.stopPropagation();
  }

  keyup(e: KeyboardEvent): void {
    if (e.key === 'Control' && this.dock.switcher()) {
      e.preventDefault();
      this.dock.commitSwitch();
    }
  }
}
