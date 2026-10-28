import { ChangeDetectionStrategy, Component, OnDestroy, OnInit, computed, inject, input, output, signal } from '@angular/core';
import type { FitVerdict, LocalModel, SlotChoice } from '../../core/api.types';
import { bytes, concurrencyText, slotsText, tokensText } from '../../shared/format';
import { fitLook } from '../../shared/labels';
import { ModelsStore } from './models.store';
import { SlotsStore, contextSteps, nearestStep } from './slots.store';

/** The quick picks next to the slider. */
export const SLOT_PICKS: readonly number[] = [1, 2, 4, 8];

/** Parallel slots of one model: pick a count, see what each slot costs and how much context or concurrency fits. */
@Component({
  selector: 'pw-slots-panel',
  providers: [SlotsStore],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="head">
      <p class="title">Parallel slots · {{ model().name }}</p>
      <button type="button" class="btn ghost sm" (click)="closed.emit()" aria-label="Close slots">Close</button>
    </div>
    <p class="lead">A slot serves one request at a time, so more slots let more requests run together. Each slot keeps its own context in memory.</p>

    <div class="picks" role="radiogroup" [attr.aria-label]="'Parallel slots for ' + model().name">
      @for (n of picks(); track n) {
        <button type="button" role="radio" class="pick" [attr.aria-checked]="slots() === n" [attr.data-tone]="tone(n)" (click)="setSlots(n)">
          <b>{{ n }}</b>
          <span>{{ adds(n) }}</span>
        </button>
      }
    </div>

    <label class="slider">
      <span>Slots: <b class="num">{{ slotsLabel() }}</b></span>
      <input class="range" type="range" min="1" max="16" step="1" [value]="slots()" [attr.aria-label]="'Parallel slots for ' + model().name" (input)="setSlots(+$any($event.target).value)" />
    </label>
    <label class="slider">
      <span>Context per slot: <b class="num">{{ contextLabel() }}</b></span>
      <input class="range" type="range" min="0" [max]="steps().length - 1" step="1" [value]="step()" [attr.aria-label]="'Context per slot for ' + model().name" (input)="setStep(+$any($event.target).value)" />
    </label>

    @if (fits.error(); as e) {
      <p class="err" role="alert">{{ e.message }} <button type="button" class="btn ghost sm" (click)="ask()">Retry</button></p>
    } @else if (fits.fit(); as f) {
      <p class="answer" [attr.data-tone]="look(f.verdict).tone">
        <span class="status" [attr.data-tone]="look(f.verdict).tone">{{ look(f.verdict).label }}</span>
        {{ f.message }}
      </p>
      @if (f.choices.length) {
        <div class="scroll">
          <table class="table fit-table">
            <caption class="sr-only">Memory and largest context for each number of slots</caption>
            <thead>
              <tr>
                <th scope="col">Slots</th>
                <th scope="col" class="right">Memory</th>
                <th scope="col" class="right">Adds</th>
                <th scope="col" class="right">Longest context each</th>
                <th scope="col">Fits?</th>
              </tr>
            </thead>
            <tbody>
              @for (c of f.choices; track c.slots) {
                <tr [class.sel]="c.slots === slots()">
                  <td class="num">{{ c.slots }}</td>
                  <td class="right num">{{ size(c.needBytes, 'memory') }}</td>
                  <td class="right num">{{ c.extraBytes ? '+' + size(c.extraBytes, 'memory') : '—' }}</td>
                  <td class="right num">{{ c.maxContext ? tokens(c.maxContext) : 'none' }}</td>
                  <td><span class="status" [attr.data-tone]="look(c.verdict).tone">{{ look(c.verdict).label }}</span></td>
                </tr>
              }
            </tbody>
          </table>
        </div>
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
        <button type="button" class="btn sm" [disabled]="busy()" (click)="useDefault()">Use the default</button>
      }
      <button type="button" class="btn primary sm" [disabled]="busy() || !changed()" (click)="save()">Save {{ slotsLabel() }}</button>
    </div>
  `,
  styles: `
    :host {
      display: flex;
      flex-direction: column;
      gap: var(--sp-3);
      width: 460px;
      max-width: calc(100vw - 32px);
      /* Taller than the window never hides Save: the panel scrolls inside the overlay. */
      max-height: calc(100vh - 24px);
      /* A steady size from the first frame, so the overlay is placed (and pushed into the window) for its final height. */
      min-height: min(600px, calc(100vh - 24px));
      overflow-y: auto;
      box-sizing: border-box;
      padding: var(--sp-4);
      background: var(--bg-raised);
      border: 1px solid var(--border);
      border-radius: var(--r-lg);
      box-shadow: var(--shadow-pop);
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
      font-weight: var(--fw-semibold);
    }
    .lead,
    .note,
    .sum {
      font-size: var(--fs-sm);
      color: var(--text-2);
    }
    .picks {
      display: grid;
      grid-template-columns: repeat(4, 1fr);
      gap: var(--sp-2);
    }
    .pick {
      display: flex;
      flex-direction: column;
      align-items: center;
      gap: 2px;
      min-height: 52px;
      padding: var(--sp-2);
      font: inherit;
      color: var(--text);
      background: var(--bg-card);
      border: 1px solid var(--border);
      border-radius: var(--r-md);
      cursor: default;
      &:hover {
        background: var(--bg-hover);
      }
      &[aria-checked='true'] {
        border-color: var(--accent);
        box-shadow: 0 0 0 1px var(--accent);
      }
      b {
        font-size: var(--fs-lg, 16px);
        font-variant-numeric: tabular-nums;
      }
      span {
        font-size: var(--fs-xs);
        color: var(--text-3);
        font-variant-numeric: tabular-nums;
      }
      &[data-tone='warn'] span {
        color: var(--warn);
      }
      &[data-tone='bad'] span {
        color: var(--bad);
      }
    }
    .slider {
      display: grid;
      gap: 4px;
      font-size: var(--fs-sm);
      color: var(--text-2);
      input {
        width: 100%;
        accent-color: var(--accent);
      }
    }
    .answer {
      display: flex;
      align-items: flex-start;
      gap: var(--sp-2);
    }
    .err {
      color: var(--bad);
    }
    .scroll {
      overflow-x: auto;
    }
    .fit-table tr.sel td {
      background: color-mix(in srgb, var(--accent) 10%, transparent);
    }
    .foot {
      display: flex;
      justify-content: flex-end;
      gap: var(--sp-2);
    }
  `,
})
export class SlotsPanel implements OnInit, OnDestroy {
  readonly model = input.required<LocalModel>();
  readonly closed = output<void>();

  protected readonly store = inject(ModelsStore);
  protected readonly fits = inject(SlotsStore);
  protected readonly size = bytes;
  protected readonly tokens = tokensText;
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
    return c.extraBytes ? `adds ~${bytes(c.extraBytes, 'memory')}` : 'baseline';
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
