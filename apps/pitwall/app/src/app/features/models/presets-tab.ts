import { ChangeDetectionStrategy, Component, computed, effect, inject, signal, untracked } from '@angular/core';
import { MatIconModule } from '@angular/material/icon';
import { MatSlideToggleModule } from '@angular/material/slide-toggle';
import { MatTooltipModule } from '@angular/material/tooltip';
import type { ModelPreset, PresetEntry, PresetStatus } from '../../core/api.types';
import { presetStatusLook } from '../../shared/labels';
import { Confirm } from '../../shared/ui/confirm';
import { ContextMenuService, type ContextMenuItem } from '../../shared/ui/context-menu';
import { RowMenu } from '../../shared/ui/row-menu';
import { isMenuKey, moveRowFocus } from '../../shared/ui/row-nav';
import { EmptyState } from '../../shared/ui/empty-state';
import { Panel } from '../../shared/ui/panel';
import { PwSelect } from '../../shared/ui/select';
import { defaultsSummary, EMPTY_FORM, formOf, parseForm, type PresetField, type PresetForm } from './preset-form';
import { PresetsStore } from './presets.store';
import { ModelsStore } from './models.store';

/** Longest alias and system prompt the editor accepts (the engine enforces the same limits). */
export const ALIAS_MAX = 48;
export const PROMPT_MAX = 8000;

/** Example capability selections offered next to the installed models. */
export const AUTO_EXAMPLES: readonly string[] = ['auto:chat', 'auto:chat,fast', 'auto:chat,tools', 'auto:vision', 'auto:embeddings'];

/** One sentence describing what a status means for whoever reads the list. */
export function statusDetail(s: PresetStatus): string {
  return s.state === 'ready' ? `Serves ${s.modelId}. ${s.reason}.` : `${s.message}.`;
}

/** Model presets and aliases (ADR 0014): apps send the alias as `model`; here the user decides what serves it. */
@Component({
  selector: 'pw-presets-tab',
  imports: [MatIconModule, MatTooltipModule, Panel, EmptyState, RowMenu, MatSlideToggleModule, PwSelect],
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
                <tr tabindex="0" [class.editing]="store.editor()?.editing === e.preset.alias" (click)="onRowClick(e.preset, $event)" (keydown)="onRowKey(e.preset, $event)" (contextmenu)="onRowMenu(e.preset, $event)">
                  <td class="alias"><span class="mono pitwall-selectable">{{ e.preset.alias }}</span>@if (e.preset.cacheResponses) {<span class="badge" matTooltip="Answers to repeated deterministic requests are cached">cache</span>}@if (e.preset.keepWarm) {<span class="badge" matTooltip="The system prompt is kept warm across unloads">warm</span>}</td>
                  <td><span class="mono target pitwall-selectable" [matTooltip]="e.preset.target" matTooltipShowDelay="600">{{ e.preset.target }}</span></td>
                  <td class="serves">
                    <span class="status" [attr.data-tone]="look(e).tone" [matTooltip]="detail(e)">{{ look(e).label }}</span>
                    <span class="via" [class.mono]="e.status.state === 'ready'" [matTooltip]="e.status.state === 'ready' ? e.status.modelId : message(e)">{{ e.status.state === 'ready' ? e.status.modelId : message(e) }}</span>
                  </td>
                  <td class="defs"><span [matTooltip]="defaultsText(e.preset)" matTooltipShowDelay="600">{{ defaultsText(e.preset) || '—' }}</span></td>
                  <td class="act">
                    <span class="row-actions">
                      <button type="button" class="btn ghost icon sm reveal" [attr.aria-label]="'Edit ' + e.preset.alias" matTooltip="Edit" [disabled]="store.deleting().includes(e.preset.alias)" (click)="openEdit(e.preset)">
                        <mat-icon svgIcon="pw:edit" aria-hidden="true" />
                      </button>
                      <pw-row-menu [items]="menuItems(e.preset)" [label]="'More actions for ' + e.preset.alias" />
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

          <p class="hint faint">The context applies when the model is loaded for a request. A model that is already loaded keeps what it was loaded with until it is released. Values a request carries win over the defaults here.</p>

          <div class="toggle-row">
            <div>
              <span class="t" id="pw-preset-cache-l">Cache answers</span>
              <span class="faint d">Repeat a deterministic request (temperature 0 or a fixed seed, no tools) and get the saved answer. Apps that have not opted in under Apps & keys are never cached.</span>
            </div>
            <mat-slide-toggle hideIcon aria-labelledby="pw-preset-cache-l" [checked]="form().cacheResponses" (change)="setFlag('cacheResponses', $event.checked)" />
          </div>
          <div class="toggle-row">
            <div>
              <span class="t" id="pw-preset-warm-l">Keep prompt warm</span>
              <span class="faint d">Saves the system prompt's processed prefix when the model unloads and restores it on load, so the first request is fast. Bounded in size; cleared when the model, engine or preset changes.</span>
            </div>
            <mat-slide-toggle hideIcon aria-labelledby="pw-preset-warm-l" [checked]="form().keepWarm" (change)="setFlag('keepWarm', $event.checked)" />
          </div>

          <div class="preview" role="status" aria-live="polite" aria-label="What this preset resolves to">
            @if (store.preview(); as p) {
              @if (p.problem) {
                <p class="bad"><mat-icon svgIcon="pw:alert" aria-hidden="true" /><span>Cannot be saved: {{ p.problem }}</span></p>
              }
              @if (p.status; as s) {
                <p [class.faint]="!!p.problem">
                  <span class="status" [attr.data-tone]="statusLook(s).tone">{{ statusLook(s).label }}</span>
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
    .alias {
      font-weight: var(--fw-medium);
    }
    .target,
    .via {
      font-size: var(--fs-sm);
    }
    .target {
      display: block;
      max-width: 260px;
      overflow: hidden;
      text-overflow: ellipsis;
    }
    /* One line per preset: the verdict, then what serves it (or why nothing does), truncated with a tooltip. */
    .serves {
      max-width: 340px;
    }
    .via {
      display: inline-block;
      max-width: 220px;
      margin-left: var(--sp-2);
      overflow: hidden;
      vertical-align: bottom;
      text-overflow: ellipsis;
      color: var(--text-2);
    }
    .defs {
      max-width: 260px;
      overflow: hidden;
      font-size: var(--fs-sm);
      color: var(--text-2);
      text-overflow: ellipsis;
    }
    .table td.act {
      width: 1%;
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
    }
    .toggle-row {
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: var(--sp-6);
      > div {
        display: flex;
        flex-direction: column;
        gap: 2px;
        min-width: 0;
      }
      .t {
        font-weight: var(--fw-medium);
      }
      .d {
        font-size: var(--fs-sm);
      }
    }
    .badge {
      display: inline-block;
      margin-left: var(--sp-2);
      padding: 0 6px;
      font-size: var(--fs-xs);
      font-weight: var(--fw-medium);
      line-height: 18px;
      color: var(--text-2);
      background: var(--bg-active);
      border-radius: var(--r-full);
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
  private readonly menu = inject(ContextMenuService);
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

  protected setFlag(field: 'cacheResponses' | 'keepWarm', on: boolean): void {
    this.form.update((f) => ({ ...f, [field]: on }));
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

  /** What a preset defaults, on one line: `temperature 0.3 · max 800 tokens · system prompt`. */
  protected defaultsText(p: ModelPreset): string {
    return [...defaultsSummary(p), ...(p.systemPrompt !== null ? ['system prompt'] : [])].join(' · ');
  }

  /** The list behind the row's "..." button and right-click. Delete is only here and asks first. */
  protected menuItems(p: ModelPreset): ContextMenuItem[] {
    return [
      { label: 'Edit', run: () => this.openEdit(p) },
      { label: 'Delete…', run: () => void this.remove(p.alias), danger: true, divider: true, disabled: this.store.deleting().includes(p.alias) },
    ];
  }

  protected onRowClick(p: ModelPreset, e: Event): void {
    if (!(e.target as HTMLElement).closest('button')) this.openEdit(p);
  }

  protected onRowKey(p: ModelPreset, e: KeyboardEvent): void {
    if (e.target !== e.currentTarget) return;
    if (e.key === 'Enter') {
      e.preventDefault();
      this.openEdit(p);
    } else if (isMenuKey(e)) {
      e.preventDefault();
      const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
      this.menu.open(r.left + 48, r.bottom - 8, this.menuItems(p));
    } else {
      moveRowFocus(e);
    }
  }

  protected onRowMenu(p: ModelPreset, e: MouseEvent): void {
    e.preventDefault();
    this.menu.open(e.clientX, e.clientY, this.menuItems(p));
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
