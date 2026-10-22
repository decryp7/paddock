import { ChangeDetectionStrategy, Component, computed, effect, inject, signal, untracked } from '@angular/core';
import { MatIconModule } from '@angular/material/icon';
import { MatTooltipModule } from '@angular/material/tooltip';
import type { ModelPreset, PresetEntry, PresetStatus } from '../../core/api.types';
import { presetStatusLook } from '../../shared/labels';
import { Confirm } from '../../shared/ui/confirm';
import { EmptyState } from '../../shared/ui/empty-state';
import { Panel } from '../../shared/ui/panel';
import { defaultsSummary, EMPTY_FORM, formOf, parseForm, type PresetField, type PresetForm } from './preset-form';
import { PresetsStore } from './presets.store';
import { ModelsStore } from './models.store';

/** Longest alias and system prompt the editor accepts (the engine enforces the same limits). */
export const ALIAS_MAX = 48;
export const PROMPT_MAX = 8192;

/** Example capability selections offered next to the installed models. */
export const AUTO_EXAMPLES: readonly string[] = ['auto:chat', 'auto:chat,fast', 'auto:chat,tools', 'auto:vision', 'auto:embeddings'];

/** One sentence describing what a status means for whoever reads the list. */
export function statusDetail(s: PresetStatus): string {
  return s.state === 'ready' ? `Serves ${s.modelId}. ${s.reason}.` : `${s.message}.`;
}

/** Model presets and aliases (ADR 0014): apps send the alias as `model`; here the user decides what serves it. */
@Component({
  selector: 'pw-presets-tab',
  imports: [MatIconModule, MatTooltipModule, Panel, EmptyState],
  providers: [PresetsStore],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <pw-panel heading="Presets" icon="sliders" [sub]="summary()" [flush]="true" [loading]="store.loading()" [error]="store.error()" [ready]="store.loaded()" [skeletonLines]="4" (retry)="store.load()">
      <div panel-actions>
        <button type="button" class="btn sm primary" [disabled]="!!store.editor()" (click)="openNew()"><mat-icon svgIcon="pw:plus" aria-hidden="true" />New preset</button>
      </div>
      @if (store.entries().length === 0) {
        <pw-empty-state icon="sliders" heading="No presets yet" text="A preset gives an app a stable name such as debrief-explain. You decide which model, context, temperature and system prompt serve it.">
          <button type="button" class="btn primary sm" [disabled]="!!store.editor()" (click)="openNew()"><mat-icon svgIcon="pw:plus" aria-hidden="true" />New preset</button>
        </pw-empty-state>
      } @else {
        <div class="scroll">
          <table class="table">
            <thead>
              <tr>
                <th scope="col">Name apps send</th>
                <th scope="col">Target</th>
                <th scope="col">Serves now</th>
                <th scope="col">Defaults</th>
                <th scope="col"><span class="sr-only">Actions</span></th>
              </tr>
            </thead>
            <tbody>
              @for (e of store.entries(); track e.preset.alias) {
                <tr [class.editing]="store.editor()?.editing === e.preset.alias">
                  <td class="alias"><span class="mono pitwall-selectable">{{ e.preset.alias }}</span></td>
                  <td><span class="mono target pitwall-selectable">{{ e.preset.target }}</span></td>
                  <td>
                    <span class="pill dot" [attr.data-tone]="look(e).tone" [matTooltip]="detail(e)">{{ look(e).label }}</span>
                    @if (e.status.state === 'ready') {
                      <span class="serves mono">{{ e.status.modelId }}</span>
                    } @else {
                      <span class="why">{{ message(e) }}</span>
                    }
                  </td>
                  <td>
                    <span class="tags">
                      @for (d of defaults(e.preset); track d) {
                        <span class="tag">{{ d }}</span>
                      }
                      @if (e.preset.systemPrompt !== null) {
                        <span class="tag">system prompt</span>
                      }
                    </span>
                  </td>
                  <td class="right">
                    <span class="actions">
                      <button type="button" class="btn sm" [attr.aria-label]="'Edit ' + e.preset.alias" [disabled]="store.deleting().includes(e.preset.alias)" (click)="openEdit(e.preset)">Edit</button>
                      <button type="button" class="btn ghost icon sm del" [attr.aria-label]="'Delete ' + e.preset.alias" matTooltip="Delete" [disabled]="store.deleting().includes(e.preset.alias)" (click)="remove(e.preset.alias)">
                        <mat-icon svgIcon="pw:trash" aria-hidden="true" />
                      </button>
                    </span>
                  </td>
                </tr>
              }
            </tbody>
          </table>
        </div>
      }
    </pw-panel>

    @if (store.editor(); as ed) {
      <pw-panel [heading]="ed.editing === null ? 'New preset' : 'Edit ' + ed.editing" icon="sliders" [ready]="true">
        <form class="editor" (submit)="$event.preventDefault(); save()" aria-label="Preset editor">
          <div class="grid">
            <div class="field">
              <label for="pw-preset-alias">Name apps send as the model</label>
              <input class="input mono" id="pw-preset-alias" autocomplete="off" spellcheck="false" placeholder="debrief-explain" [attr.maxlength]="aliasMax" [value]="form().alias" (input)="set('alias', $any($event.target).value)" />
            </div>
            <div class="field">
              <label for="pw-preset-target">Serves from</label>
              <input class="input mono" id="pw-preset-target" autocomplete="off" spellcheck="false" placeholder="An installed model, or auto:chat,fast" list="pw-preset-targets" [value]="form().target" (input)="set('target', $any($event.target).value)" />
              <datalist id="pw-preset-targets">
                @for (m of models.library(); track m.id) {
                  <option [value]="m.id">{{ m.name }}</option>
                }
                @for (a of autoExamples; track a) {
                  <option [value]="a"></option>
                }
              </datalist>
            </div>
          </div>
          <p class="hint faint">A model id serves exactly that model. <span class="mono">auto:chat,fast</span> lets Pitwall pick the best installed model with these capabilities for each request.</p>

          <div class="grid four">
            <div class="field">
              <label for="pw-preset-context">Context (tokens)</label>
              <input class="input num" id="pw-preset-context" inputmode="numeric" autocomplete="off" placeholder="Model default" [attr.aria-invalid]="bad('context')" [value]="form().context" (input)="set('context', $any($event.target).value)" />
            </div>
            <div class="field">
              <label for="pw-preset-temperature">Temperature</label>
              <input class="input num" id="pw-preset-temperature" inputmode="decimal" autocomplete="off" placeholder="Request decides" [attr.aria-invalid]="bad('temperature')" [value]="form().temperature" (input)="set('temperature', $any($event.target).value)" />
            </div>
            <div class="field">
              <label for="pw-preset-topp">Top-p</label>
              <input class="input num" id="pw-preset-topp" inputmode="decimal" autocomplete="off" placeholder="Request decides" [attr.aria-invalid]="bad('topP')" [value]="form().topP" (input)="set('topP', $any($event.target).value)" />
            </div>
            <div class="field">
              <label for="pw-preset-max">Max tokens</label>
              <input class="input num" id="pw-preset-max" inputmode="numeric" autocomplete="off" placeholder="Request decides" [attr.aria-invalid]="bad('maxTokens')" [value]="form().maxTokens" (input)="set('maxTokens', $any($event.target).value)" />
            </div>
          </div>
          @if (parsed().invalid.size > 0) {
            <p class="err" role="alert">Context, temperature, top-p and max tokens must be plain numbers, or empty.</p>
          }

          <div class="field">
            <label for="pw-preset-system">System prompt</label>
            <textarea class="input" id="pw-preset-system" rows="4" [attr.maxlength]="promptMax" placeholder="Added in front of the conversation only when the request has no system message of its own." [value]="form().systemPrompt" (input)="set('systemPrompt', $any($event.target).value)"></textarea>
          </div>

          <div class="field engine">
            <label for="pw-preset-engine">Engine</label>
            <select class="select" id="pw-preset-engine" (change)="set('engine', $any($event.target).value)">
              <option value="" [selected]="form().engine === ''">llama.cpp</option>
              @for (e of models.engineChoices(); track e.id) {
                <option [value]="e.id" [selected]="form().engine === e.id">{{ e.name }}</option>
              }
            </select>
          </div>
          <p class="hint faint">Context and engine apply when the model is loaded for a request. A model that is already loaded keeps what it was loaded with until it is released. Values a request carries win over the defaults here.</p>

          <div class="preview" role="status" aria-live="polite" aria-label="What this preset resolves to">
            @if (store.preview(); as p) {
              @if (p.problem) {
                <p class="bad"><mat-icon svgIcon="pw:alert" aria-hidden="true" /><span>Cannot be saved: {{ p.problem }}</span></p>
              }
              @if (p.status; as s) {
                <p [class.faint]="!!p.problem">
                  <span class="pill dot" [attr.data-tone]="statusLook(s).tone">{{ statusLook(s).label }}</span>
                  @if (s.state === 'ready') {
                    <span>Would serve <b class="mono">{{ s.modelId }}</b> ({{ s.reason }}).</span>
                  } @else {
                    <span>{{ s.message }}.</span>
                  }
                </p>
              }
            } @else if (store.previewing()) {
              <p class="faint">Checking…</p>
            } @else {
              <p class="faint">Type a name and a target to see what would serve it.</p>
            }
          </div>
          @if (store.saveError(); as e) {
            <p class="err" role="alert"><mat-icon svgIcon="pw:alert" aria-hidden="true" />{{ e.message }}</p>
          }

          <div class="foot">
            <button type="button" class="btn ghost" (click)="store.closeEditor()">Cancel</button>
            <button type="submit" class="btn primary" [disabled]="!canSave()">{{ ed.editing === null ? 'Create preset' : 'Save preset' }}</button>
          </div>
        </form>
      </pw-panel>
    }
  `,
  styles: `
    :host {
      display: grid;
      gap: var(--sp-3);
    }
    .scroll {
      overflow-x: auto;
      margin-top: var(--sp-2);
    }
    .del:hover {
      color: var(--bad);
    }
    .alias {
      font-weight: var(--fw-medium);
    }
    .target,
    .serves {
      font-size: var(--fs-sm);
      word-break: break-all;
    }
    .serves {
      display: block;
      margin-top: 2px;
      color: var(--text-2);
    }
    .why {
      display: block;
      max-width: 38ch;
      margin-top: 2px;
      font-size: var(--fs-xs);
      color: var(--text-2);
    }
    .tags {
      display: flex;
      flex-wrap: wrap;
      gap: 4px;
    }
    tr.editing td {
      background: var(--accent-soft);
    }
    .actions {
      display: inline-flex;
      gap: var(--sp-1);
      align-items: center;
    }
    .editor {
      display: grid;
      gap: var(--sp-3);
    }
    .grid {
      display: grid;
      grid-template-columns: repeat(2, minmax(0, 1fr));
      gap: var(--sp-3);
      &.four {
        grid-template-columns: repeat(4, minmax(0, 1fr));
      }
    }
    @media (max-width: 760px) {
      .grid,
      .grid.four {
        grid-template-columns: minmax(0, 1fr);
      }
    }
    .field {
      display: grid;
      gap: 4px;
      label {
        font-size: var(--fs-xs);
        font-weight: var(--fw-medium);
        color: var(--text-2);
      }
      &.engine .select {
        max-width: 280px;
      }
    }
    .hint {
      margin: 0;
      font-size: var(--fs-xs);
    }
    .err {
      display: flex;
      gap: var(--sp-2);
      align-items: center;
      margin: 0;
      padding: var(--sp-2) var(--sp-3);
      font-size: var(--fs-sm);
      background: var(--bad-soft);
      border-radius: var(--r-md);
      mat-icon {
        flex: none;
        color: var(--bad);
      }
    }
    .preview {
      padding: var(--sp-3);
      background: var(--bg-sunken);
      border: 1px solid var(--border-soft);
      border-radius: var(--r-md);
      p {
        display: flex;
        flex-wrap: wrap;
        gap: var(--sp-2);
        align-items: center;
        margin: 0;
        font-size: var(--fs-sm);
      }
      p + p {
        margin-top: var(--sp-2);
      }
      .bad {
        color: var(--bad);
      }
    }
    .foot {
      display: flex;
      justify-content: flex-end;
      gap: var(--sp-2);
    }
  `,
})
export class PresetsTab {
  protected readonly store = inject(PresetsStore);
  protected readonly models = inject(ModelsStore);
  private readonly confirm = inject(Confirm);
  protected readonly aliasMax = ALIAS_MAX;
  protected readonly promptMax = PROMPT_MAX;
  protected readonly autoExamples = AUTO_EXAMPLES;
  protected readonly statusLook = presetStatusLook;
  protected readonly defaults = defaultsSummary;

  protected readonly form = signal<PresetForm>({ ...EMPTY_FORM });
  protected readonly parsed = computed(() => parseForm(this.form()));
  protected readonly summary = computed(() => {
    const n = this.store.entries().length;
    const b = this.store.broken();
    return n === 0 ? '' : `${n} preset${n === 1 ? '' : 's'}${b ? `, ${b} cannot be served now` : ''}`;
  });
  protected readonly canSave = computed(() => {
    const { draft, invalid } = this.parsed();
    const p = this.store.preview();
    return !!this.store.editor() && !this.store.saving() && invalid.size === 0 && draft.alias !== '' && draft.target !== '' && !p?.problem;
  });

  constructor() {
    void this.store.load();
    // Ask the engine what the draft resolves to whenever it changes (after typing pauses).
    effect(() => {
      const { draft, invalid } = this.parsed();
      if (!untracked(() => this.store.editor()) || invalid.size > 0 || (draft.alias === '' && draft.target === '')) return;
      untracked(() => this.store.schedulePreview(draft));
    });
  }

  protected look(e: PresetEntry) {
    return presetStatusLook(e.status);
  }

  protected detail(e: PresetEntry): string {
    return statusDetail(e.status);
  }

  protected message(e: PresetEntry): string {
    return e.status.state === 'ready' ? '' : e.status.message;
  }

  protected bad(field: PresetField): boolean {
    return this.parsed().invalid.has(field);
  }

  protected set(field: PresetField, value: string): void {
    this.form.update((f) => ({ ...f, [field]: value }));
  }

  protected openNew(): void {
    this.form.set({ ...EMPTY_FORM });
    this.store.openEditor(null);
  }

  protected openEdit(p: ModelPreset): void {
    this.form.set(formOf(p));
    this.store.openEditor(p.alias);
  }

  protected async save(): Promise<void> {
    if (this.canSave()) await this.store.save(this.parsed().draft);
  }

  protected async remove(alias: string): Promise<void> {
    const ok = await this.confirm.ask({
      title: `Delete ${alias}?`,
      message: 'Apps that ask for this name will get an error until you create it again. The model itself is not touched.',
      confirm: 'Delete preset',
      danger: true,
    });
    if (ok) await this.store.remove(alias);
  }
}
