import { ChangeDetectionStrategy, Component, OnDestroy, OnInit, computed, inject, input, output, signal } from '@angular/core';
import { MatButton } from '@angular/material/button';
import { MatButtonToggle, MatButtonToggleGroup } from '@angular/material/button-toggle';
import { MatSlider, MatSliderThumb } from '@angular/material/slider';
import { MatTableModule } from '@angular/material/table';
import type { FitVerdict, LocalModel, SlotChoice } from '../../core/api.types';
import { bytes, bytesIn, bytesUnit, concurrencyText, int, slotsText, tokensText, varies } from '../../shared/format';
import { fitLook } from '../../shared/labels';
import { ModelsStore } from './models.store';
import { SlotsStore, contextSteps, nearestStep } from './slots.store';

/** The quick picks next to the slider. */
export const SLOT_PICKS: readonly number[] = [1, 2, 4, 8];

/** Parallel slots of one model: pick a count, see what each slot costs and how much context or concurrency fits. */
@Component({
  selector: 'pw-slots-panel',
  imports: [MatButton, MatButtonToggleGroup, MatButtonToggle, MatSlider, MatSliderThumb, MatTableModule],
  providers: [SlotsStore],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="head">
      <p class="title">Parallel slots · {{ model().name }}</p>
      <button type="button" matButton (click)="closed.emit()" aria-label="Close slots">Close</button>
    </div>
    <p class="lead">A slot serves one request at a time, so more slots let more requests run together. Each slot keeps its own context in memory.</p>

    <mat-button-toggle-group class="picks" hideSingleSelectionIndicator [aria-label]="'Parallel slots for ' + model().name" [value]="slots()" (change)="setSlots(+$event.value)">
      @for (n of picks(); track n) {
        <mat-button-toggle class="pick" [value]="n" [attr.data-tone]="tone(n)">
          <b>{{ n }}</b>
          <span>{{ adds(n) }}</span>
        </mat-button-toggle>
      }
    </mat-button-toggle-group>

    <div class="slider">
      <span aria-hidden="true">Slots: <b class="num">{{ slotsLabel() }}</b></span>
      <mat-slider min="1" max="16" step="1">
        <input matSliderThumb [value]="slots()" [attr.aria-label]="'Parallel slots for ' + model().name" [attr.aria-valuetext]="slotsLabel()" (input)="setSlots(+$any($event.target).value)" />
      </mat-slider>
    </div>
    <div class="slider">
      <span aria-hidden="true">Context per slot: <b class="num">{{ contextLabel() }}</b></span>
      <mat-slider min="0" [max]="steps().length - 1" step="1">
        <input matSliderThumb [value]="step()" [attr.aria-label]="'Context per slot for ' + model().name" [attr.aria-valuetext]="contextLabel()" (input)="setStep(+$any($event.target).value)" />
      </mat-slider>
    </div>

    @if (fits.error(); as e) {
      <p class="err" role="alert">{{ e.message }} <button type="button" matButton (click)="ask()">Retry</button></p>
    } @else if (fits.fit(); as f) {
      <p class="answer" [attr.data-tone]="look(f.verdict).tone">
        <span class="status" [attr.data-tone]="look(f.verdict).tone">{{ look(f.verdict).label }}</span>
        {{ f.message }}
      </p>
      @if (f.choices.length) {
        <table mat-table class="choices" [dataSource]="f.choices" [trackBy]="bySlots" [fixedLayout]="true">
          <caption class="sr-only">Memory and largest context for each number of slots</caption>
          <ng-container matColumnDef="slots">
            <th mat-header-cell *matHeaderCellDef scope="col" class="right">Slots</th>
            <td mat-cell *matCellDef="let c" class="right num">{{ c.slots }}</td>
          </ng-container>
          <ng-container matColumnDef="memory">
            <th mat-header-cell *matHeaderCellDef scope="col" class="right">Memory {{ unitOf(f.choices) }}</th>
            <td mat-cell *matCellDef="let c" class="right num">{{ sizeIn(c.needBytes, unitOf(f.choices)) }}</td>
          </ng-container>
          <ng-container matColumnDef="adds">
            <th mat-header-cell *matHeaderCellDef scope="col" class="right">Adds {{ unitOf(f.choices) }}</th>
            <td mat-cell *matCellDef="let c" class="right num">{{ c.extraBytes ? '+' + sizeIn(c.extraBytes, unitOf(f.choices)) : '—' }}</td>
          </ng-container>
          <ng-container matColumnDef="context">
            <th mat-header-cell *matHeaderCellDef scope="col" class="right">Longest context tokens</th>
            <td mat-cell *matCellDef="let c" class="right num">{{ c.maxContext ? intText(c.maxContext) : 'none' }}</td>
          </ng-container>
          <ng-container matColumnDef="fits">
            <th mat-header-cell *matHeaderCellDef scope="col">Fits?</th>
            <td mat-cell *matCellDef="let c"><span class="status" [attr.data-tone]="look(c.verdict).tone">{{ look(c.verdict).label }}</span></td>
          </ng-container>
          <tr mat-header-row *matHeaderRowDef="columnsOf(f.choices); sticky: true"></tr>
          <tr mat-row *matRowDef="let c; columns: columnsOf(f.choices)" [class.selected]="c.slots === slots()"></tr>
        </table>
        <p class="sum">At {{ tokens(f.context) }} per slot this machine runs {{ concurrency(f.maxSlots) }}. {{ f.kvExact ? '' : 'The memory per slot is an estimate: the model file does not describe its layout.' }}</p>
      }
    } @else if (fits.loading()) {
      <p class="faint">Asking the engine…</p>
    }

    <p class="note">
      @if (pending()) {
        {{ model().name }} is loaded with {{ slotsLabelOf(model().loadedSlots ?? 0) }}; the new count applies the next time it loads. Requests it is serving now are not interrupted.
      } @else {
        A change applies the next time the model loads; a loaded model keeps working as it is.
      }
    </p>
    <div class="foot">
      @if (model().slotsCustom) {
        <button type="button" matButton="outlined" [disabled]="busy()" (click)="useDefault()">Use the default</button>
      }
      <button type="button" matButton="filled" [disabled]="busy() || !changed()" (click)="save()">Save {{ slotsLabel() }}</button>
    </div>
  `,
  styles: `
    :host {
      display: flex;
      flex-direction: column;
      gap: var(--pd-space-3);
      width: 460px;
      max-width: calc(100vw - 32px);
      /* Taller than the window never hides Save: the panel scrolls inside the overlay. */
      max-height: calc(100vh - 24px);
      /* A steady size from the first frame, so the overlay is placed (and pushed into the window) for its final height. */
      min-height: min(600px, calc(100vh - 24px));
      overflow-y: auto;
      box-sizing: border-box;
      padding: var(--pd-space-4);
      background: var(--pd-surface-3);
      border: 1px solid var(--pd-hairline);
      border-radius: var(--pd-radius-card);
      box-shadow: var(--pd-shadow-3);
    }
    p {
      margin: 0;
    }
    .head {
      display: flex;
      align-items: center;
      justify-content: space-between;
    }
    .title {
      font-weight: var(--pd-weight-semibold);
    }
    .lead,
    .note,
    .sum {
      font-size: var(--pd-text-label);
      color: var(--pd-text-muted);
    }
    /* The quick picks: a segmented group whose segments carry the count and what it adds (two lines). */
    .picks {
      display: grid;
      grid-template-columns: repeat(auto-fit, minmax(0, 1fr));
      grid-auto-flow: column;
      --mat-button-toggle-height: 52px;
    }
    .pick {
      min-width: 0;
      ::ng-deep .mat-button-toggle-label-content {
        padding-inline: var(--pd-space-1);
        display: flex;
        flex-direction: column;
        align-items: center;
        gap: 2px;
        line-height: var(--pd-lh-label);
      }
      b {
        font-size: var(--pd-text-subtitle, 16px);
        font-variant-numeric: tabular-nums;
      }
      span {
        font-size: var(--pd-text-label);
        color: var(--pd-text-subtle);
        font-variant-numeric: tabular-nums;
      }
      &[data-tone='warn'] span {
        color: var(--pd-warning);
      }
      &[data-tone='bad'] span {
        color: var(--pd-danger);
      }
    }
    .slider {
      display: grid;
      gap: 4px;
      font-size: var(--pd-text-label);
      color: var(--pd-text-muted);
      /* Material keeps 8 px each side for the thumb at either end: a block slider in its own margins never overflows. */
      mat-slider {
        display: block;
        width: auto;
        margin: 0 8px;
        min-width: 0;
      }
    }
    .answer {
      display: flex;
      align-items: flex-start;
      gap: var(--pd-space-2);
    }
    .err {
      color: var(--pd-danger);
    }
    /* On the popover's raised surface: the rows are see-through, the header that sticks to the popover's scroll is opaque. */
    .choices {
      --mat-table-background-color: transparent;
      .mat-mdc-header-cell {
        background: var(--pd-surface-3);
      }
      .mat-column-slots {
        width: 56px;
      }
      .mat-column-memory,
      .mat-column-adds {
        width: 88px;
      }
      .mat-column-fits {
        width: 88px;
      }
    }
    .foot {
      display: flex;
      justify-content: flex-end;
      gap: var(--pd-space-2);
    }
  `,
})
export class SlotsPanel implements OnInit, OnDestroy {
  readonly model = input.required<LocalModel>();
  readonly closed = output<void>();

  protected readonly store = inject(ModelsStore);
  protected readonly fits = inject(SlotsStore);
  protected readonly tokens = tokensText;
  protected readonly intText = int;
  protected readonly sizeIn = bytesIn;
  protected readonly bySlots = (_: number, c: SlotChoice) => c.slots;
  /** The memory columns' unit, from the largest need. */
  protected unitOf(choices: readonly SlotChoice[]) {
    return bytesUnit(choices.map((c) => c.needBytes));
  }
  /** "Fits?" only when the counts differ in it (UI review 2 rule 7); the answer above already says the verdict. */
  protected columnsOf(choices: readonly SlotChoice[]): string[] {
    return ['slots', 'memory', 'adds', 'context', ...(varies(choices, (c) => c.verdict) ? ['fits'] : [])];
  }
  protected readonly concurrency = concurrencyText;
  protected readonly slotsLabelOf = slotsText;
  protected readonly look = (v: FitVerdict) => fitLook(v);

  /** The slots being looked at (starts at the model's own). */
  protected readonly slots = signal(1);
  protected readonly step = signal(0);
  protected readonly steps = computed(() => contextSteps(this.model().contextMax));
  protected readonly context = computed(() => this.steps()[this.step()] ?? this.steps()[0]!);
  protected readonly slotsLabel = computed(() => slotsText(this.slots()));
  protected readonly contextLabel = computed(() => tokensText(this.context()));
  protected readonly busy = computed(() => this.store.isBusy()('model:' + this.model().id));
  protected readonly changed = computed(() => this.slots() !== this.model().slots);
  /** Loaded with another count than the one saved: the saved one applies on the next load. */
  protected readonly pending = computed(() => this.model().loadedSlots !== null && this.model().loadedSlots !== this.model().slots);
  /** The quick picks, plus the current count when it is not one of them. */
  protected readonly picks = computed(() => [...new Set([...SLOT_PICKS, this.slots()])].sort((a, b) => a - b));

  ngOnInit(): void {
    this.slots.set(this.model().slots);
    this.step.set(nearestStep(this.steps(), 8192));
    this.ask();
  }

  ngOnDestroy(): void {
    this.fits.reset();
  }

  protected ask(): void {
    void this.fits.query(this.model().id, this.context(), this.slots());
  }

  protected setSlots(n: number): void {
    this.slots.set(n);
    this.fits.schedule(this.model().id, this.context(), n);
  }

  protected setStep(i: number): void {
    this.step.set(i);
    this.fits.schedule(this.model().id, this.context(), this.slots());
  }

  private choice(n: number): SlotChoice | undefined {
    return this.fits.fit()?.choices.find((c) => c.slots === n);
  }

  /** `adds ~1.0 GB` under a quick pick (what that many slots add over one). */
  protected adds(n: number): string {
    const c = this.choice(n);
    if (!c) return n === 1 ? 'baseline' : '…';
    return c.extraBytes ? `adds ~${bytes(c.extraBytes)}` : 'baseline';
  }

  protected tone(n: number): 'ok' | 'warn' | 'bad' {
    const v = this.choice(n)?.verdict;
    return v === 'wontFit' ? 'bad' : v === 'tight' ? 'warn' : 'ok';
  }

  protected async save(): Promise<void> {
    if (await this.store.setSlots(this.model().id, this.slots())) this.closed.emit();
  }

  protected async useDefault(): Promise<void> {
    if (await this.store.setSlots(this.model().id, null)) this.closed.emit();
  }
}
