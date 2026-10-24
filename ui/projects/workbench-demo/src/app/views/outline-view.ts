import { ChangeDetectionStrategy, Component, computed, inject } from '@angular/core';
import { ShellContextHost } from '@decryp7/paddock-ui/shell';
import type { WorkbenchDemoContext } from '../context';
import { DemoRepo } from '../repo';

/** The outline of the selected file (a side view): its symbols; a click opens the file. */
@Component({
  selector: 'wd-outline-view',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @if (symbols().length) {
      <ul class="list" aria-label="Symbols">
        @for (s of symbols(); track s.name) {
          <li><button type="button" class="sym" (click)="open()"><span class="kind">{{ s.kind }}</span><span class="name">{{ s.name }}</span><span class="line">{{ s.line }}</span></button></li>
        }
      </ul>
    } @else {
      <p class="none">No symbols in {{ path() ?? 'the selected file' }}.</p>
    }
  `,
  styles: `
    :host { display: block; height: 100%; overflow: auto; padding: var(--pd-space-2); font-size: var(--pd-text-body); }
    .list { list-style: none; margin: 0; padding: 0; }
    .sym { display: flex; gap: var(--pd-space-2); width: 100%; min-width: 0; height: 26px; align-items: center; padding: 0 var(--pd-space-2); border: 0; border-radius: var(--pd-radius-control); background: transparent; color: var(--pd-text); font: inherit; cursor: pointer; text-align: start; }
    .sym:hover { background: var(--pd-hover); }
    .sym:focus-visible { outline: 2px solid var(--pd-focus-ring); }
    .kind { flex: none; width: 3.5em; color: var(--pd-text-muted); font-family: var(--pd-font-mono); font-size: var(--pd-text-label); }
    .name { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .line { flex: none; color: var(--pd-text-muted); font-variant-numeric: tabular-nums; }
    .none { margin: 0; padding: var(--pd-space-2); color: var(--pd-text-muted); }
  `,
})
export class OutlineView {
  private readonly host = inject(ShellContextHost);
  private readonly repo = inject(DemoRepo);
  private readonly ctx = computed(() => this.host.context() as unknown as WorkbenchDemoContext | null);
  protected readonly path = computed(() => this.ctx()?.selectedPath() ?? null);
  protected readonly symbols = computed(() => {
    const p = this.path();
    return p ? (this.repo.file(p)?.symbols ?? []) : [];
  });

  protected open(): void {
    const p = this.path();
    if (p) void this.ctx()?.openFile(p);
  }
}
