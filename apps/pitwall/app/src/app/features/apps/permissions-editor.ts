import { ChangeDetectionStrategy, Component, computed, effect, input, output, signal, untracked, viewChildren } from '@angular/core';
import { MatButton } from '@angular/material/button';
import { MatCheckbox } from '@angular/material/checkbox';
import type { ErrorStateMatcher } from '@angular/material/core';
import { MatError, MatFormField, MatLabel, MatSuffix } from '@angular/material/form-field';
import { MatInput } from '@angular/material/input';
import { MatRadioButton, MatRadioGroup } from '@angular/material/radio';
import { MatTooltipModule } from '@angular/material/tooltip';
import { MatSlideToggleModule } from '@angular/material/slide-toggle';
import { SuSegmentedControl, type SuSegment } from '@decryp7/paddock-ui';
import type { AppLimits, AppPermissions, LocalModel, Priority } from '../../core/api.types';
import { PRIORITY_LABEL } from '../../shared/labels';

const PRIORITIES: Priority[] = ['interactive', 'background', 'batch'];

const PRIORITY_HINT: Record<Priority, string> = {
  interactive: 'May jump ahead of background and batch work',
  background: 'Runs when no interactive request waits',
  batch: 'Runs only when nothing else does',
};

/** A limit field's text as a value: empty is no limit, a positive whole number is the limit, anything else invalid. */
export function parseLimit(text: string, max = Number.MAX_SAFE_INTEGER): number | null | 'invalid' {
  const t = text.trim();
  if (t === '') return null;
  if (!/^\d+$/.test(t)) return 'invalid';
  const n = Number(t);
  return n >= 1 && n <= max && Number.isSafeInteger(n) ? n : 'invalid';
}

/** Deep equality of two permission sets (model order ignored). */
export function samePermissions(a: AppPermissions, b: AppPermissions): boolean {
  const models = (p: AppPermissions) => (p.models === null ? null : [...p.models].sort().join('\n'));
  return (
    models(a) === models(b) &&
    a.maxPriority === b.maxPriority &&
    a.tools === b.tools &&
    a.extendedApis === b.extendedApis &&
    a.cacheResponses === b.cacheResponses &&
    LIMITS.every((l) => a.limits[l.key] === b.limits[l.key])
  );
}

type LimitKey = keyof AppLimits;

const LIMITS: { key: LimitKey; label: string; unit: string; max?: number }[] = [
  { key: 'requestsPerMin', label: 'Requests per minute', unit: '/min' },
  { key: 'requestsPerDay', label: 'Requests per day', unit: '/day' },
  { key: 'tokensPerDay', label: 'Tokens per day', unit: 'tok' },
  { key: 'tokensPerMonth', label: 'Tokens per month', unit: 'tok' },
  { key: 'maxConcurrent', label: 'Requests at once', unit: 'max' },
  { key: 'maxMemoryPercent', label: 'Memory share of RAM', unit: '%', max: 100 },
];

const NO_TEXT: Record<LimitKey, string> = { requestsPerMin: '', requestsPerDay: '', tokensPerDay: '', tokensPerMonth: '', maxConcurrent: '', maxMemoryPercent: '' };

/** Edits an app's permissions locally; Save emits the parsed permissions (the server validates again). */
@Component({
  selector: 'pw-permissions-editor',
  imports: [MatButton, MatCheckbox, MatError, MatFormField, MatInput, MatLabel, MatRadioButton, MatRadioGroup, MatSuffix, MatSlideToggleModule, MatTooltipModule, SuSegmentedControl],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <form (submit)="$event.preventDefault(); submit()" [attr.aria-label]="'Permissions of ' + appName()">
      <fieldset class="group">
        <legend>Models</legend>
        <mat-radio-group class="choice" aria-label="Which models" [value]="allModels() ? 'all' : 'some'" (change)="allModels.set($event.value === 'all')">
          <mat-radio-button value="all">Every model</mat-radio-button>
          <mat-radio-button value="some">Only these</mat-radio-button>
        </mat-radio-group>
        @if (!allModels()) {
          @if (models().length === 0) {
            <p class="faint hint">No local models yet. Download one in Models.</p>
          } @else {
            <ul class="models">
              @for (m of models(); track m.id) {
                <li>
                  <mat-checkbox class="check" [checked]="chosen().has(m.id)" (change)="toggleModel(m.id)">
                    <span class="mname" [matTooltip]="m.name" matTooltipShowDelay="600">{{ m.name }}</span>
                    @if (m.quant) {
                      <span class="tag">{{ m.quant }}</span>
                    }
                  </mat-checkbox>
                </li>
              }
            </ul>
          }
        }
      </fieldset>

      <fieldset class="group">
        <legend>Highest priority</legend>
        <pd-segmented-control label="Highest priority" [options]="prioritySegs" [value]="priority()" (valueChange)="setPriority($event)" />
        <p class="faint hint">{{ priorityHint[priority()] }}</p>
      </fieldset>

      <fieldset class="group">
        <legend>Capabilities</legend>
        <div class="toggle-row">
          <div>
            <span class="t">Tools</span>
            <span class="faint d">Call MCP tools</span>
          </div>
          <mat-slide-toggle [checked]="tools()" (change)="tools.set($event.checked)" aria-label="Tools" />
        </div>
        <div class="toggle-row">
          <div>
            <span class="t">Extended APIs</span>
            <span class="faint d">Pitwall's /x/v1 endpoints: rerank, tasks, batch</span>
          </div>
          <mat-slide-toggle [checked]="extended()" (change)="extended.set($event.checked)" aria-label="Extended APIs" />
        </div>
        <div class="toggle-row">
          <div>
            <span class="t">Cache answers</span>
            <span class="faint d">Repeat a deterministic request (temperature 0 or a fixed seed, no tools) and get the saved answer. Off by default.</span>
          </div>
          <mat-slide-toggle [checked]="cacheResponses()" (change)="cacheResponses.set($event.checked)" aria-label="Cache answers" />
        </div>
      </fieldset>

      <fieldset class="group">
        <legend>Limits</legend>
        <div class="limits">
          @for (l of limitFields; track l.key) {
            <mat-form-field class="field">
              <mat-label>{{ l.label }}</mat-label>
              <input matInput class="num" [id]="uid + '-' + l.key" inputmode="numeric" placeholder="No limit" autocomplete="off" [errorStateMatcher]="matchers[l.key]" [value]="limitText()[l.key]" (input)="setLimit(l.key, $any($event.target).value)" />
              <span matTextSuffix class="unit">{{ l.unit }}</span>
              @if (invalid().has(l.key)) {
                <mat-error><span role="alert">{{ l.max ? 'A whole number from 1 to ' + l.max + ', or empty.' : 'A whole number of 1 or more, or empty.' }}</span></mat-error>
              }
            </mat-form-field>
          }
        </div>
      </fieldset>

      <div class="foot">
        <button type="button" matButton [disabled]="!dirty() || busy()" (click)="reset()">Reset</button>
        <button type="submit" matButton="filled" [disabled]="!dirty() || invalid().size > 0 || busy()">Save permissions</button>
      </div>
    </form>
  `,
  styles: `
    form {
      display: grid;
      gap: var(--pd-space-4);
    }
    .group {
      margin: 0;
      padding: 0;
      border: none;
      min-width: 0;
    }
    legend {
      padding: 0;
      margin-bottom: var(--pd-space-2);
      font-size: var(--pd-text-label);
      font-weight: var(--pd-weight-semibold);
      color: var(--pd-text);
    }
    .choice {
      display: flex;
      gap: var(--pd-space-4);
    }
    .check {
      display: block;
      max-width: 100%;
      min-width: 0;
    }
    .check ::ng-deep label {
      display: inline-flex;
      align-items: center;
      gap: var(--pd-space-2);
      min-width: 0;
      overflow: hidden;
    }
    .models {
      list-style: none;
      margin: var(--pd-space-3) 0 0;
      padding: var(--pd-space-1) var(--pd-space-3);
      display: grid;
      /* Two columns when there is room for a model's name, else one. */
      grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
      gap: 0 var(--pd-space-4);
      background: var(--pd-surface-2);
      border: 1px solid var(--pd-hairline);
      border-radius: var(--pd-radius-control);
      li {
        min-width: 0;
      }
    }
    /* A long model name ends in an ellipsis inside the checkbox's label instead of pushing it out of its column. */
    .check {
      display: block;
      min-width: 0;
      ::ng-deep .mdc-form-field {
        display: flex;
        max-width: 100%;
        min-width: 0;
      }
      ::ng-deep .mdc-form-field > label {
        display: flex;
        align-items: center;
        gap: var(--pd-space-1);
        min-width: 0;
        overflow: hidden;
      }
    }
    .mname {
      min-width: 0;
      white-space: nowrap;
      overflow: hidden;
      text-overflow: ellipsis;
    }
    .hint {
      margin-top: var(--pd-space-2);
      font-size: var(--pd-text-label);
    }
    .toggle-row {
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: var(--pd-space-4);
      padding: var(--pd-space-2) 0;
      border-top: 1px solid var(--pd-hairline);
      &:first-of-type {
        border-top: none;
        padding-top: 0;
      }
      .t {
        display: block;
        font-weight: var(--pd-weight-medium);
      }
      .d {
        font-size: var(--pd-text-label);
      }
    }
    .limits {
      display: grid;
      grid-template-columns: repeat(3, minmax(0, 1fr));
      gap: var(--pd-space-3);
    }
    .field {
      min-width: 0;
    }
    .unit {
      padding-inline-end: var(--pd-space-2);
      font-size: var(--pd-text-label);
      color: var(--pd-text-subtle);
    }
    .foot {
      display: flex;
      justify-content: flex-end;
      gap: var(--pd-space-2);
      padding-top: var(--pd-space-4);
      border-top: 1px solid var(--pd-hairline);
    }
  `,
})
export class PermissionsEditor {
  private static next = 0;
  protected readonly uid = `pw-perm-${PermissionsEditor.next++}`;

  readonly permissions = input.required<AppPermissions>();
  readonly models = input<LocalModel[]>([]);
  readonly appName = input('');
  readonly busy = input(false);
  readonly save = output<AppPermissions>();

  protected readonly priorityHint = PRIORITY_HINT;
  protected readonly limitFields = LIMITS;
  protected readonly prioritySegs: SuSegment[] = PRIORITIES.map((p) => ({ value: p, label: PRIORITY_LABEL[p] }));
  /** A limit field shows Material's error state (red outline, the message under it) while its text is invalid. */
  protected readonly matchers = Object.fromEntries(LIMITS.map((l) => [l.key, { isErrorState: () => this.invalid().has(l.key) } satisfies ErrorStateMatcher])) as unknown as Record<LimitKey, ErrorStateMatcher>;

  protected readonly allModels = signal(true);
  protected readonly chosen = signal<ReadonlySet<string>>(new Set());
  protected readonly priority = signal<Priority>('interactive');
  protected readonly tools = signal(false);
  protected readonly extended = signal(false);
  protected readonly cacheResponses = signal(false);
  protected readonly limitText = signal<Record<LimitKey, string>>({ ...NO_TEXT });

  protected readonly invalid = computed(() => {
    const t = this.limitText();
    return new Set(LIMITS.filter((l) => parseLimit(t[l.key], l.max) === 'invalid').map((l) => l.key));
  });

  /** The edited permissions, or null while a field is invalid. */
  readonly value = computed<AppPermissions | null>(() => {
    if (this.invalid().size) return null;
    const t = this.limitText();
    return {
      models: this.allModels() ? null : [...this.chosen()],
      maxPriority: this.priority(),
      tools: this.tools(),
      extendedApis: this.extended(),
      cacheResponses: this.cacheResponses(),
      limits: Object.fromEntries(LIMITS.map((l) => [l.key, parseLimit(t[l.key], l.max) as number | null])) as unknown as AppLimits,
    };
  });

  protected readonly dirty = computed(() => {
    const v = this.value();
    return v === null || !samePermissions(v, this.permissions());
  });

  private readonly limitInputs = viewChildren(MatInput);

  constructor() {
    effect(() => this.load(this.permissions()));
    // effect-state: a matInput without a form control never asks its matcher by itself; tell it when the invalid set changes.
    effect(() => {
      this.invalid();
      const inputs = this.limitInputs();
      untracked(() => inputs.forEach((i) => i.updateErrorState()));
    });
  }

  protected reset(): void {
    this.load(this.permissions());
  }

  private load(p: AppPermissions): void {
    const text = (n: number | null) => (n === null ? '' : String(n));
    this.allModels.set(p.models === null);
    this.chosen.set(new Set(p.models ?? []));
    this.priority.set(p.maxPriority);
    this.tools.set(p.tools);
    this.extended.set(p.extendedApis);
    this.cacheResponses.set(p.cacheResponses);
    this.limitText.set(Object.fromEntries(LIMITS.map((l) => [l.key, text(p.limits[l.key])])) as Record<LimitKey, string>);
  }

  protected setPriority(p: string): void {
    const chosen = PRIORITIES.find((x) => x === p);
    if (chosen) this.priority.set(chosen);
  }

  protected toggleModel(id: string): void {
    const next = new Set(this.chosen());
    if (next.has(id)) next.delete(id);
    else next.add(id);
    this.chosen.set(next);
  }

  protected setLimit(key: LimitKey, text: string): void {
    this.limitText.update((t) => ({ ...t, [key]: text }));
  }

  protected submit(): void {
    const v = this.value();
    if (v && this.dirty()) this.save.emit(v);
  }
}
