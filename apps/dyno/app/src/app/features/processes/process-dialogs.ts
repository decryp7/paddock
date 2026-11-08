import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';
import { FormsModule } from '@angular/forms';
import type { CommandError, LockerList } from '../../core/api.types';
import { errorHint } from '../../core/command-error';
import { Icon } from '../../shared/icon';
import { ReadingSource } from '../../core/reading-source';
import { ProcessesStore } from '../../state/processes.store';
import { MatButton } from '@angular/material/button';
import { MatFormField } from '@angular/material/form-field';
import { MatInput } from '@angular/material/input';

/** "What is locking this file?": the processes that hold a path open, with what they are. */
@Component({
  selector: 'dy-lockers-dialog',
  imports: [MatFormField, MatInput, MatButton, FormsModule, Icon, ReadingSource],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
      <p class="lead">Find the programs that hold a file or folder open.</p>
      <form class="find" (ngSubmit)="find()">
        <mat-form-field class="grow"><input matInput name="path" [ngModel]="path()" (ngModelChange)="path.set($event)" placeholder="Path of the file or folder" aria-label="Path" autocomplete="off" spellcheck="false" /></mat-form-field>
        <button type="submit" matButton="filled" [disabled]="busy() || !path().trim()">Find</button>
      </form>
      @if (error(); as e) {
        <div class="err" role="alert"><dy-icon name="alert" /><span>{{ e.message }} @if (hint(); as h) {<small>{{ h }}</small>}</span></div>
      }
      @if (!results() && !error() && !busy()) {
        <p class="empty" data-testid="lockers-empty">Enter a path and choose Find.</p>
      }
      @if (results(); as found) {
        @if (found.lockers.length === 0 && found.notListed === 0 && !found.truncated) {
          <p class="none"><dy-icon name="check" /> Nothing has it open right now.</p>
        }
        @if (found.lockers[0]; as first) { <dy-reading-source [via]="first.via" [canRetry]="true" (retry)="find()" /> }
        @if (found.notListed > 0) {
          <p class="more" data-testid="lockers-not-listed"><dy-icon name="info" /> {{ found.notListed }} more {{ found.notListed === 1 ? 'program' : 'programs' }} of other accounts {{ found.notListed === 1 ? 'holds' : 'hold' }} it and {{ found.notListed === 1 ? 'is' : 'are' }} not listed.</p>
        }
        @if (found.truncated) {
          <p class="more" data-testid="lockers-truncated"><dy-icon name="info" /> The search stopped at its limit, so more programs may hold it.</p>
        }
        <ul>
          @for (l of found.lockers; track l.process.pid) {
            <li>
              <div class="txt"><strong>{{ l.name }}</strong><small>{{ l.kind ?? 'Holds it open' }}</small></div>
            </li>
          }
        </ul>
      }
  `,
  styles: `
    :host { display: grid; gap: var(--sp-3); min-width: 0; }
    .lead { color: var(--text-2); }
    .find { display: flex; gap: var(--sp-2); }
    .grow { flex: 1; }
    .err { display: flex; gap: var(--sp-2); color: var(--bad); }
    .err small { display: block; color: var(--text-2); }
    .empty { color: var(--text-3); }
    .more { display: flex; align-items: center; gap: var(--sp-2); margin: 0; color: var(--text-2); }
    .none { display: flex; align-items: center; gap: var(--sp-2); color: var(--ok); }
    ul { list-style: none; margin: 0; padding: 0; display: grid; gap: var(--sp-1); }
    li { display: flex; align-items: center; gap: var(--sp-3); padding: var(--sp-2) var(--sp-3); border: 1px solid var(--border); border-radius: var(--r-md); }
    .txt { flex: 1; display: grid; }
    small { color: var(--text-3); }
  `,
})
export class LockersDialog {
  private readonly store = inject(ProcessesStore);
  protected readonly path = signal('');
  protected readonly busy = signal(false);
  protected readonly results = signal<LockerList | null>(null);
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
