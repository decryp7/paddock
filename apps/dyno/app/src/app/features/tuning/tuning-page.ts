import { ChangeDetectionStrategy, Component, OnInit, computed, inject } from '@angular/core';
import { firstValueFrom } from 'rxjs';
import type { TuningSuggestion } from '../../core/api.types';
import { AsyncState } from '../../shared/async-state';
import { Dialogs } from '../../shared/dialog';
import { Icon } from '../../shared/icon';
import { TuningStore } from '../../state/tuning.store';
import { HelperGate } from '../helper/helper-gate';
import { TuningDialog, type TuningDialogData } from './tuning-dialog';
import { actionFor, groupTuning, summary } from './tuning-model';

/** The tuning catalogue: reversible suggestions grouped by topic. Nothing changes until you review one and confirm; each shows its exact change and its inverse first. */
@Component({
  selector: 'dy-tuning-page',
  imports: [AsyncState, Icon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="page">
      <header class="page-head">
        <div class="titles">
          <h1>Tuning</h1>
          <p>{{ line() }}</p>
        </div>
        <button type="button" class="btn sm ghost icon" aria-label="Check again" title="Check again" (click)="store.load()"><dy-icon name="refresh" /></button>
      </header>

      <dy-async [loaded]="store.loaded()" [error]="store.error()" [empty]="store.items().length === 0" emptyTitle="Nothing to tune" emptyText="Dyno has no suggestions for this computer right now. It checks again whenever you open this page." what="tuning suggestions" [rows]="4" (retry)="store.load()">
        <div class="stack">
        @for (g of groups(); track g.id) {
          <section class="card" [attr.aria-label]="g.title">
            <div class="card-head">
              <h2>{{ g.title }}</h2>
              <span class="sub">{{ g.text }}</span>
            </div>
            <ul class="tunes">
              @for (t of g.items; track t.id) {
                <li>
                  <div class="txt">
                    <strong>{{ t.title }}</strong>
                    <p>{{ t.explanation }}</p>
                    <div class="chips">
                      <span class="chip" [title]="'Now ' + t.current + ', suggested ' + t.proposed"><span class="k">Now</span><span class="v">{{ t.current }}</span><dy-icon name="arrow-right" /><span class="v">{{ t.proposed }}</span></span>
                      @if (t.needsAdmin) {
                        <span class="chip"><dy-icon name="shield" /> Administrator</span>
                      }
                      @if (t.applied) {
                        <span class="chip">Applied by Dyno</span>
                      }
                      @if (t.changedOutside) {
                        <span class="chip">Changed since</span>
                      }
                    </div>
                    @if (t.blocked) {
                      <p class="why">{{ t.blocked }}</p>
                    }
                    @if (t.changedOutside) {
                      <p class="why">Something else changed this setting after Dyno applied it, so Dyno will not overwrite it unless you insist.</p>
                    }
                  </div>
                  <div class="act">
                    @switch (action(t)) {
                      @case ('review') {
                        <button type="button" class="btn" [disabled]="busy(t.id)" [attr.aria-label]="'Review ' + t.title" (click)="review(t)">Review...</button>
                      }
                      @case ('blocked') {
                        <button type="button" class="btn" disabled [attr.aria-label]="t.title + ' needs administrator rights'">Needs administrator rights</button>
                      }
                      @case ('revert') {
                        <button type="button" class="btn" [disabled]="busy(t.id)" [attr.aria-label]="'Revert ' + t.title" (click)="revert(t, false)">Revert</button>
                      }
                      @case ('revertAnyway') {
                        <button type="button" class="btn" [disabled]="busy(t.id)" [attr.aria-label]="'Revert ' + t.title + ' anyway'" (click)="revert(t, true)">Revert anyway...</button>
                      }
                    }
                  </div>
                </li>
              }
            </ul>
          </section>
        }
        </div>
      </dy-async>
    </div>
  `,
  styles: `
    :host { display: block; }
    .page { display: grid; gap: var(--sp-4); align-content: start; }
    .stack { display: grid; gap: var(--sp-4); min-width: 0; align-content: start; }
    .page-head { margin-bottom: 0; }
    .card-head { flex-wrap: wrap; row-gap: 0; }
    .sub { color: var(--text-3); font-size: var(--fs-sm); }
    .tunes { list-style: none; margin: 0; padding: 0 var(--sp-4) var(--sp-2); display: grid; }
    .tunes li { display: grid; grid-template-columns: minmax(0, 1fr) auto; gap: var(--sp-4); align-items: start; padding: var(--sp-3) 0; border-top: 1px solid var(--border-subtle); }
    .tunes li:first-child { border-top: 0; }
    .tunes p { margin: 2px 0 0; color: var(--text-2); font-size: var(--fs-sm); }
    .tunes .why { color: var(--text-3); }
    .chips { display: flex; flex-wrap: wrap; gap: var(--sp-2); margin-top: var(--sp-2); }
    .chip dy-icon { --icon-size: 13px; color: var(--text-3); }
    .act { padding-top: 2px; }
    @media (max-width: 720px) { .tunes li { grid-template-columns: minmax(0, 1fr); } }
  `,
})
export class TuningPage implements OnInit {
  protected readonly store = inject(TuningStore);
  private readonly dialogs = inject(Dialogs);
  private readonly gate = inject(HelperGate);
  protected readonly groups = computed(() => groupTuning(this.store.items()));
  protected readonly line = computed(() => (this.store.loaded() ? summary(this.store.items()) : 'Reversible suggestions. Dyno changes nothing until you review one and confirm.'));
  protected readonly action = actionFor;

  ngOnInit(): void {
    void this.store.load();
  }

  protected busy(id: string): boolean {
    return this.store.busy().includes(id);
  }

  protected async review(t: TuningSuggestion): Promise<void> {
    // A change that needs administrator rights first makes sure the helper is there (explains, then the system asks once).
    if (t.needsAdmin && !(await this.gate.ensure())) return;
    if (await this.confirm({ suggestion: t, mode: 'apply', force: false })) await this.store.apply(t.id);
  }

  protected async revert(t: TuningSuggestion, force: boolean): Promise<void> {
    if (t.needsAdmin && !(await this.gate.ensure())) return;
    if (await this.confirm({ suggestion: t, mode: 'revert', force })) await this.store.revert(t.id, force);
  }

  private async confirm(data: TuningDialogData): Promise<boolean> {
    const ref = this.dialogs.open<boolean, TuningDialogData>(TuningDialog, data);
    return (await firstValueFrom(ref.closed)) === true;
  }
}
