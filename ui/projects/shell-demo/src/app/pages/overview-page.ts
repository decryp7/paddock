import { ChangeDetectionStrategy, Component, computed, inject } from '@angular/core';
import { CommandService, ShellContextHost } from '@decryp7/paddock-ui/shell';
import { PdContextMenu } from '@decryp7/paddock-ui/shell/menu';
import type { DemoContext } from '../demo-context';
import { DemoNote } from '../demo-note';
import { EnginePanel } from '../engine-panel';
import { DemoPage, SECTION_STYLES } from './demo-page';

/** The first page: the contributed commands, the greetings (with their context menu), the engine panel and the note. */
@Component({
  selector: 'sd-overview-page',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [DemoPage, EnginePanel, PdContextMenu],
  template: `
    <sd-page heading="Overview" subtitle="The shell’s parts on sample data: commands, menus, health, jobs and the start-up contract.">
      <section aria-labelledby="sd-commands">
        <h2 id="sd-commands">Commands</h2>
        <p class="context" data-testid="app-context">App context: {{ contextLoaded() ? 'loaded' : 'not loaded yet' }}</p>
        <div class="commands" role="group" aria-label="Commands">
          @for (command of commands.available(); track command.id) {
            <button type="button" class="cmd" [attr.data-command]="command.id" [disabled]="!commands.isEnabled(command)" (click)="run(command.id)">
              {{ command.title.text }}
            </button>
          }
        </div>
        <pd-context-menu menu="sd.greetings" label="Greeting actions">
          <p class="greetings" data-testid="greetings">Greetings: {{ greetings() }}</p>
        </pd-context-menu>
      </section>
      <sd-engine-panel />
      <section aria-labelledby="sd-startup">
        <h2 id="sd-startup">Start-up contract</h2>
        <p class="context">The window reports ready after its first paint and answers liveness pings; this note is kept if the shell reloads the window to recover it.</p>
        <label class="note">
          <span>Note</span>
          <input #noteField type="text" data-testid="demo-note" [value]="note()" (input)="note.set(noteField.value)" />
        </label>
      </section>
    </sd-page>
  `,
  styles:
    SECTION_STYLES +
    `
    .greetings { margin: 0; padding: var(--pd-space-2); border: 1px dashed var(--pd-border-control); border-radius: var(--pd-radius-control); }
    .note { display: flex; flex-direction: column; gap: var(--pd-space-1); max-width: 40ch; font-size: var(--pd-text-label); font-weight: var(--pd-weight-medium); }
    .note input { min-height: var(--pd-button-height); padding: 0 var(--pd-space-2); border-radius: var(--pd-radius-input); border: 1px solid var(--pd-border-control); background: var(--pd-surface-1); color: var(--pd-text); font: inherit; font-size: var(--pd-text-body); font-weight: var(--pd-weight-regular); min-width: 0; }
    .note input:focus-visible { outline: 2px solid var(--pd-focus-ring); outline-offset: 1px; }
  `,
})
export class OverviewPage {
  protected readonly commands = inject(CommandService);
  private readonly host = inject(ShellContextHost);
  protected readonly note = inject(DemoNote).note;
  protected readonly contextLoaded = computed(() => this.host.context() !== null);
  protected readonly greetings = computed(() => (this.host.context() as Partial<DemoContext> | null)?.greetings?.() ?? 0);

  protected run(id: string): void {
    void this.commands.run(id);
  }
}
