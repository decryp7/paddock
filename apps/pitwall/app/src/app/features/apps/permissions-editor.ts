import { ChangeDetectionStrategy, Component, computed, effect, input, output, signal } from '@angular/core';
import { MatSlideToggleModule } from '@angular/material/slide-toggle';
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
  imports: [MatSlideToggleModule],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <form (submit)="$event.preventDefault(); submit()" [attr.aria-label]="'Permissions of ' + appName()">
      <fieldset class="group">
        <legend>Models</legend>
        <div class="choice" role="radiogroup" aria-label="Which models">
          <label class="radio"><input type="radio" [name]="uid + '-models'" [checked]="allModels()" (change)="allModels.set(true)" /> Every model</label>
          <label class="radio"><input type="radio" [name]="uid + '-models'" [checked]="!allModels()" (change)="allModels.set(false)" /> Only these</label>
        </div>
        @if (!allModels()) {
          @if (models().length === 0) {
            <p class="faint hint">No local models yet. Download one in Models.</p>
          } @else {
            <ul class="models">
              @for (m of models(); track m.id) {
                <li>
                  <label class="check">
                    <input type="checkbox" [checked]="chosen().has(m.id)" (change)="toggleModel(m.id)" />
                    <span class="mname">{{ m.name }}</span>
                    @if (m.quant) {
                      <span class="tag">{{ m.quant }}</span>
                    }
                  </label>
                </li>
              }
            </ul>
          }
        }
      </fieldset>

      <fieldset class="group">
        <legend>Highest priority</legend>
        <div class="seg" role="radiogroup" aria-label="Highest priority">
          @for (p of priorities; track p) {
            <button type="button" role="radio" [attr.aria-checked]="priority() === p" (click)="priority.set(p)">{{ priorityLabel[p] }}</button>
          }
        </div>
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
            <div class="field">
              <label [for]="uid + '-' + l.key">{{ l.label }}</label>
              <div class="with-unit">
                <input class="input num" [id]="uid + '-' + l.key" inputmode="numeric" placeholder="No limit" autocomplete="off" [value]="limitText()[l.key]" (input)="setLimit(l.key, $any($event.target).value)" [attr.aria-invalid]="invalid().has(l.key)" />
                <span class="unit">{{ l.unit }}</span>
              </div>
              @if (invalid().has(l.key)) {
                <span class="err" role="alert">{{ l.max ? 'A whole number from 1 to ' + l.max + ', or empty.' : 'A whole number of 1 or more, or empty.' }}</span>
              }
            </div>
          }
        </div>
      </fieldset>

      <div class="foot">
        <button type="button" class="btn ghost" [disabled]="!dirty() || busy()" (click)="reset()">Reset</button>
        <button type="submit" class="btn primary" [disabled]="!dirty() || invalid().size > 0 || busy()">Save permissions</button>
      </div>
    </form>
  `,
  styles: `
    form {
      display: grid;
      gap: var(--sp-5);
    }
    .group {
      margin: 0;
      padding: 0;
      border: none;
      min-width: 0;
    }
    legend {
      padding: 0;
      margin-bottom: var(--sp-2);
      font-size: var(--fs-sm);
      font-weight: var(--fw-semibold);
      color: var(--text);
    }
    .choice {
      display: flex;
      gap: var(--sp-5);
    }
    .radio,
    .check {
      display: inline-flex;
      align-items: center;
      gap: var(--sp-2);
      cursor: default;
      input {
        accent-color: var(--accent);
        margin: 0;
      }
    }
    .models {
      list-style: none;
      margin: var(--sp-3) 0 0;
      padding: var(--sp-1) var(--sp-3);
      display: grid;
      grid-template-columns: repeat(2, minmax(0, 1fr));
      gap: 0 var(--sp-4);
      background: var(--bg-sunken);
      border: 1px solid var(--border-soft);
      border-radius: var(--r-md);
      li {
        padding: 6px 0;
      }
    }
    .mname {
      white-space: nowrap;
      overflow: hidden;
      text-overflow: ellipsis;
    }
    .hint {
      margin-top: var(--sp-2);
      font-size: var(--fs-sm);
    }
    .seg {
      display: inline-flex;
      padding: 2px;
      background: var(--bg-active);
      border-radius: var(--r-md);
      button {
        height: 26px;
        padding: 0 var(--sp-3);
        font: inherit;
        font-size: var(--fs-sm);
        font-weight: var(--fw-medium);
        color: var(--text-2);
        background: none;
        border: none;
        border-radius: var(--r-sm);
        cursor: default;
        &[aria-checked='true'] {
          color: var(--text);
          background: var(--bg-card);
          box-shadow: var(--shadow-1);
        }
      }
    }
    .toggle-row {
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: var(--sp-4);
      padding: var(--sp-2) 0;
      border-top: 1px solid var(--border-soft);
      &:first-of-type {
        border-top: none;
        padding-top: 0;
      }
      .t {
        display: block;
        font-weight: var(--fw-medium);
      }
      .d {
        font-size: var(--fs-sm);
      }
    }
    .limits {
      display: grid;
      grid-template-columns: repeat(3, minmax(0, 1fr));
      gap: var(--sp-3);
    }
    .field {
      display: grid;
      gap: 6px;
      label {
        font-size: var(--fs-sm);
        color: var(--text-2);
      }
    }
    .with-unit {
      position: relative;
      .input {
        width: 100%;
        padding-right: 44px;
      }
      .unit {
        position: absolute;
        right: var(--sp-3);
        top: 50%;
        transform: translateY(-50%);
        font-size: var(--fs-xs);
        color: var(--text-3);
        pointer-events: none;
      }
    }
    .input[aria-invalid='true'] {
      border-color: var(--bad);
    }
    .err {
      font-size: var(--fs-xs);
      color: var(--bad);
    }
    .foot {
      display: flex;
      justify-content: flex-end;
      gap: var(--sp-2);
      padding-top: var(--sp-4);
      border-top: 1px solid var(--border-soft);
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

  protected readonly priorities = PRIORITIES;
  protected readonly priorityLabel = PRIORITY_LABEL;
  protected readonly priorityHint = PRIORITY_HINT;
  protected readonly limitFields = LIMITS;

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

  constructor() {
    effect(() => this.load(this.permissions()));
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
