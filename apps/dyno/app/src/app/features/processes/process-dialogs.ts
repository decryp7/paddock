import { DIALOG_DATA, DialogRef } from '@angular/cdk/dialog';
import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';
import { FormsModule } from '@angular/forms';
import type { CommandError, Locker } from '../../core/api.types';
import { errorHint } from '../../core/command-error';
import { Dialogs, DialogFrame } from '../../shared/dialog';
import { formatCount } from '../../shared/format';
import { Icon } from '../../shared/icon';
import { ProcessesStore } from '../../state/processes.store';

export interface AffinityData {
  name: string;
  cores: number;
  /** The cores the process may use now (bit i = core i), or `null` when unknown. */
  mask: number | null;
}

/** Which cores a process may run on: one checkbox per core. Resolves with the new bit mask. */
@Component({
  selector: 'dy-affinity-dialog',
  imports: [DialogFrame, FormsModule],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <dy-dialog title="Choose cores" [subtitle]="data.name" icon="cpu">
      <p class="msg">The app may only run on the checked cores. Pick at least one.</p>
      <div class="cores" role="group" aria-label="Cores">
        @for (c of list; track c) {
          <label class="core"><input type="checkbox" [checked]="on().has(c)" (change)="flip(c)" /> <span class="num">{{ label(c) }}</span></label>
        }
      </div>
      <div class="quick">
        <button type="button" class="btn sm" (click)="all()">All</button>
        <button type="button" class="btn sm" (click)="none()">None</button>
      </div>
      <ng-container footer>
        <button type="button" class="btn" (click)="ref.close()">Cancel</button>
        <button type="button" class="btn primary" [disabled]="on().size === 0" (click)="ref.close(mask())">Apply</button>
      </ng-container>
    </dy-dialog>
  `,
  styles: `
    .msg { color: var(--text-2); }
    .cores { display: grid; grid-template-columns: repeat(auto-fill, minmax(72px, 1fr)); gap: var(--sp-2); }
    .core { display: flex; align-items: center; gap: var(--sp-2); padding: 6px var(--sp-2); border: 1px solid var(--border); border-radius: var(--r-md); background: var(--surface-inset); }
    .quick { display: flex; gap: var(--sp-2); }
  `,
})
export class AffinityDialog {
  protected readonly data = inject<AffinityData>(DIALOG_DATA);
  protected readonly ref = inject<DialogRef<number>>(DialogRef);
  protected readonly list = Array.from({ length: this.data.cores }, (_, i) => i);
  protected readonly on = signal(new Set(this.list.filter((i) => this.data.mask === null || Math.floor(this.data.mask / 2 ** i) % 2 === 1)));
  protected readonly mask = computed(() => [...this.on()].reduce((a, i) => a + 2 ** i, 0));
  protected readonly label = (c: number) => `Core ${formatCount(c + 1)}`;

  protected flip(c: number): void {
    this.on.update((s) => {
      const next = new Set(s);
      if (!next.delete(c)) next.add(c);
      return next;
    });
  }
  protected all(): void {
    this.on.set(new Set(this.list));
  }
  protected none(): void {
    this.on.set(new Set());
  }
}

/** A hard CPU cap as a share of the whole machine. Resolves with the percentage. */
@Component({
  selector: 'dy-cap-dialog',
  imports: [DialogFrame, FormsModule],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <dy-dialog title="Cap CPU use" [subtitle]="name" icon="gauge">
      <p class="msg">The app is held to this share of the whole PC until you remove the cap or Dyno exits; Dyno releases it by itself then.</p>
      <label class="field">
        <span class="label">Cap (percent of the whole PC)</span>
        <input class="input" type="number" min="5" max="95" step="5" [ngModel]="cap()" (ngModelChange)="cap.set($event)" />
      </label>
      <ng-container footer>
        <button type="button" class="btn" (click)="ref.close()">Cancel</button>
        <button type="button" class="btn primary" [disabled]="!valid()" (click)="ref.close(cap())">Apply cap</button>
      </ng-container>
    </dy-dialog>
  `,
  styles: `.msg { color: var(--text-2); }`,
})
export class CapDialog {
  protected readonly name = inject<string>(DIALOG_DATA);
  protected readonly ref = inject<DialogRef<number>>(DialogRef);
  protected readonly cap = signal(50);
  protected readonly valid = computed(() => Number.isFinite(this.cap()) && this.cap() >= 5 && this.cap() <= 95);
}

/** "What is locking this file?": the processes that hold a path open, each with an End action. */
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
              <button type="button" class="btn sm danger" (click)="end(l)">End</button>
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

  protected async end(l: Locker): Promise<void> {
    const ok = await this.dialogs.confirm({ title: `End ${l.name}?`, message: 'Unsaved work in it is lost.', confirmLabel: 'End', danger: true });
    if (!ok) return;
    if (await this.store.act({ ref: l.process, groupId: null, name: l.name }, 'end')) await this.find();
  }
}
