import { DialogRef } from '@angular/cdk/dialog';
import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';
import { FormsModule } from '@angular/forms';
import type { CommandError, Locker } from '../../core/api.types';
import { errorHint } from '../../core/command-error';
import { Dialogs, DialogFrame } from '../../shared/dialog';
import { Icon } from '../../shared/icon';
import { ProcessesStore } from '../../state/processes.store';

/** "What is locking this file?": the processes that hold a path open, with what they are. */
@Component({
  selector: 'dy-lockers-dialog',
  imports: [DialogFrame, FormsModule, Icon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <dy-dialog title="What is locking this file?" subtitle="Find the programs that hold a file or folder open" icon="search">
      <form class="find" (ngSubmit)="find()">
        <input class="input grow" name="path" [ngModel]="path()" (ngModelChange)="path.set($event)" placeholder="Path of the file or folder" aria-label="Path" autocomplete="off" spellcheck="false" />
        <button type="submit" class="btn primary" [disabled]="busy() || !path().trim()">Find</button>
      </form>
      @if (error(); as e) {
        <div class="err" role="alert"><dy-icon name="alert" /><span>{{ e.message }} @if (hint(); as h) {<small>{{ h }}</small>}</span></div>
      }
      @if (results(); as list) {
        @if (list.length === 0) {
          <p class="none"><dy-icon name="check" /> Nothing has it open right now.</p>
        }
        <ul>
          @for (l of list; track l.process.pid) {
            <li>
              <div class="txt"><strong>{{ l.name }}</strong><small>{{ l.kind ?? 'Holds it open' }}</small></div>
            </li>
          }
        </ul>
      }
      <ng-container footer><button type="button" class="btn" (click)="ref.close()">Close</button></ng-container>
    </dy-dialog>
  `,
  styles: `
    :host { --dialog-w: 520px; }
    .find { display: flex; gap: var(--sp-2); }
    .grow { flex: 1; }
    .err { display: flex; gap: var(--sp-2); color: var(--bad); }
    .err small { display: block; color: var(--text-2); }
    .none { display: flex; align-items: center; gap: var(--sp-2); color: var(--ok); }
    ul { list-style: none; margin: 0; padding: 0; display: grid; gap: var(--sp-1); }
    li { display: flex; align-items: center; gap: var(--sp-3); padding: var(--sp-2) var(--sp-3); border: 1px solid var(--border); border-radius: var(--r-md); }
    .txt { flex: 1; display: grid; }
    small { color: var(--text-3); }
  `,
})
export class LockersDialog {
  protected readonly ref = inject<DialogRef<void>>(DialogRef);
  private readonly store = inject(ProcessesStore);
  private readonly dialogs = inject(Dialogs);
  protected readonly path = signal('');
  protected readonly busy = signal(false);
  protected readonly results = signal<Locker[] | null>(null);
  protected readonly error = signal<CommandError | null>(null);
  protected readonly hint = computed(() => {
    const e = this.error();
    return e ? errorHint(e.code) : null;
  });

  protected async find(): Promise<void> {
    this.busy.set(true);
    this.error.set(null);
    try {
      this.results.set(await this.store.lockers(this.path().trim()));
    } catch (e) {
      this.results.set(null);
      this.error.set(e as CommandError);
    } finally {
      this.busy.set(false);
    }
  }

}
