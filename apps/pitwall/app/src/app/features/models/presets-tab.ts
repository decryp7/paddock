import { ChangeDetectionStrategy, Component, computed, effect, inject, signal, untracked, viewChildren } from '@angular/core';
import { MatAutocomplete, MatAutocompleteTrigger } from '@angular/material/autocomplete';
import { MatButton, MatIconButton } from '@angular/material/button';
import { MatOption, type ErrorStateMatcher } from '@angular/material/core';
import { MatFormField, MatLabel } from '@angular/material/form-field';
import { MatIconModule } from '@angular/material/icon';
import { MatInput } from '@angular/material/input';
import { MatSlideToggleModule } from '@angular/material/slide-toggle';
import { MatTableModule } from '@angular/material/table';
import { MatTooltipModule } from '@angular/material/tooltip';
import type { ModelPreset, PresetEntry, PresetStatus } from '../../core/api.types';
import { varies } from '../../shared/format';
import { presetStatusLook } from '../../shared/labels';
import { Confirm } from '../../shared/ui/confirm';
import { ItemsMenu, MoreButton, RowMenu, type MenuItem } from '../../shared/ui/menu';
import { moveRowFocus } from '../../shared/ui/row-nav';
import { Panel } from '../../shared/ui/panel';
import { SuEmptyState } from '@decryp7/paddock-ui';
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
  imports: [MatButton, MatIconButton, MatFormField, MatLabel, MatInput, MatAutocomplete, MatAutocompleteTrigger, MatOption, MatIconModule, MatTooltipModule, Panel, SuEmptyState, ItemsMenu, MoreButton, RowMenu, MatSlideToggleModule, MatTableModule],
  providers: [PresetsStore],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <pw-panel heading="Presets" icon="sliders" [sub]="summary()" [flush]="true" [loading]="store.loading()" [error]="store.error()" [ready]="store.loaded()" [skeletonLines]="4" (retry)="store.load()">
      <div panel-actions>
        <button type="button" matButton="filled" [disabled]="!!store.editor()" (click)="openNew()"><mat-icon svgIcon="pw:plus" aria-hidden="true" />New preset</button>
      </div>
      @if (store.entries().length === 0) {
        <pd-empty-state heading="No presets yet" description="A preset gives an app a stable name such as debrief-explain. You decide which model, context, temperature and system prompt serve it.">
          <mat-icon slot="icon" svgIcon="pw:sliders" aria-hidden="true" />
        </pd-empty-state>
      } @else {
        <pw-items-menu #rowMenu label="Preset actions" />
        <table mat-table class="list" [dataSource]="store.entries()" [trackBy]="byAlias" [fixedLayout]="true" aria-label="Presets">
          <ng-container matColumnDef="alias">
            <th mat-header-cell *matHeaderCellDef scope="col">Name apps send</th>
            <td mat-cell *matCellDef="let e" class="alias"><span class="mono pitwall-selectable" [matTooltip]="e.preset.alias" matTooltipShowDelay="600">{{ e.preset.alias }}</span>@if (e.preset.cacheResponses) {<span class="badge" matTooltip="Answers to repeated deterministic requests are cached">cache</span>}@if (e.preset.keepWarm) {<span class="badge" matTooltip="The system prompt is kept warm across unloads">warm</span>}</td>
          </ng-container>
          <ng-container matColumnDef="target">
            <th mat-header-cell *matHeaderCellDef scope="col">Target</th>
            <td mat-cell *matCellDef="let e"><span class="mono target pitwall-selectable" [matTooltip]="e.preset.target" matTooltipShowDelay="600">{{ e.preset.target }}</span></td>
          </ng-container>
          <ng-container matColumnDef="serves">
            <th mat-header-cell *matHeaderCellDef scope="col">Serves now</th>
            <td mat-cell *matCellDef="let e" class="serves">
              <span class="status" [attr.data-tone]="look(e).tone" [matTooltip]="detail(e)">{{ look(e).label }}</span>
              <span class="via" [class.mono]="e.status.state === 'ready'" [matTooltip]="e.status.state === 'ready' ? e.status.modelId : message(e)">{{ e.status.state === 'ready' ? e.status.modelId : message(e) }}</span>
            </td>
          </ng-container>
          <ng-container matColumnDef="defaults">
            <th mat-header-cell *matHeaderCellDef scope="col">Defaults</th>
            <td mat-cell *matCellDef="let e" class="defs" [matTooltip]="defaultsText(e.preset)" matTooltipShowDelay="600">{{ defaultsText(e.preset) || '—' }}</td>
          </ng-container>
          <ng-container matColumnDef="act">
            <th mat-header-cell *matHeaderCellDef scope="col" class="act"><span class="sr-only">Actions</span></th>
            <td mat-cell *matCellDef="let e" class="act">
              <button type="button" matIconButton class="reveal" [attr.aria-label]="'Edit ' + e.preset.alias" matTooltip="Edit" [disabled]="store.deleting().includes(e.preset.alias)" (click)="openEdit(e.preset)">
                <mat-icon svgIcon="pw:edit" aria-hidden="true" />
              </button>
              <pw-more-button [menu]="rowMenu.menu()" [items]="menuItems(e.preset)" [label]="'More actions for ' + e.preset.alias" />
            </td>
          </ng-container>
          <tr mat-header-row *matHeaderRowDef="columns(); sticky: true"></tr>
          <tr mat-row *matRowDef="let e; columns: columns()" tabindex="0" [class.selected]="store.editor()?.editing === e.preset.alias" (click)="onRowClick(e.preset, $event)" (keydown)="onRowKey(e.preset, $event)" pwRowMenu [matContextMenuTriggerFor]="rowMenu.menu()" [matContextMenuTriggerData]="{ items: menuItems(e.preset) }"></tr>
        </table>
      }
    </pw-panel>

    @if (store.editor(); as ed) {
      <pw-panel [heading]="ed.editing === null ? 'New preset' : 'Edit ' + ed.editing" icon="sliders" [ready]="true">
        <form class="editor" (submit)="$event.preventDefault(); save()" aria-label="Preset editor">
          <div class="grid">
            <mat-form-field class="field">
              <mat-label>Name apps send as the model</mat-label>
              <input matInput class="mono" id="pw-preset-alias" autocomplete="off" spellcheck="false" placeholder="debrief-explain" [attr.maxlength]="aliasMax" [value]="form().alias" (input)="set('alias', $any($event.target).value)" />
            </mat-form-field>
            <mat-form-field class="field">
              <mat-label>Serves from</mat-label>
              <input matInput class="mono" id="pw-preset-target" autocomplete="off" spellcheck="false" placeholder="An installed model, or auto:chat,fast" [matAutocomplete]="targets" [value]="form().target" (input)="set('target', $any($event.target).value)" />
              <mat-autocomplete #targets="matAutocomplete" (optionSelected)="set('target', $event.option.value)">
                @for (m of models.library(); track m.id) {
                  <mat-option [value]="m.id"><span class="mono">{{ m.id }}</span> <span class="faint">{{ m.name }}</span></mat-option>
                }
                @for (a of autoExamples; track a) {
                  <mat-option [value]="a"><span class="mono">{{ a }}</span></mat-option>
                }
              </mat-autocomplete>
            </mat-form-field>
          </div>
          <p class="hint faint">A model id serves exactly that model. <span class="mono">auto:chat,fast</span> lets Pitwall pick the best installed model with these capabilities for each request.</p>

          <div class="grid four">
            <mat-form-field class="field">
              <mat-label>Context (tokens)</mat-label>
              <input matInput class="num" id="pw-preset-context" inputmode="numeric" autocomplete="off" placeholder="Model default" [errorStateMatcher]="invalid.context" [value]="form().context" (input)="set('context', $any($event.target).value)" />
            </mat-form-field>
            <mat-form-field class="field">
              <mat-label>Temperature</mat-label>
              <input matInput class="num" id="pw-preset-temperature" inputmode="decimal" autocomplete="off" placeholder="Request decides" [errorStateMatcher]="invalid.temperature" [value]="form().temperature" (input)="set('temperature', $any($event.target).value)" />
            </mat-form-field>
            <mat-form-field class="field">
              <mat-label>Top-p</mat-label>
              <input matInput class="num" id="pw-preset-topp" inputmode="decimal" autocomplete="off" placeholder="Request decides" [errorStateMatcher]="invalid.topP" [value]="form().topP" (input)="set('topP', $any($event.target).value)" />
            </mat-form-field>
            <mat-form-field class="field">
              <mat-label>Max tokens</mat-label>
              <input matInput class="num" id="pw-preset-max" inputmode="numeric" autocomplete="off" placeholder="Request decides" [errorStateMatcher]="invalid.maxTokens" [value]="form().maxTokens" (input)="set('maxTokens', $any($event.target).value)" />
            </mat-form-field>
          </div>
          @if (parsed().invalid.size > 0) {
            <p class="err" role="alert">Context, temperature, top-p and max tokens must be plain numbers, or empty.</p>
          }

          <mat-form-field class="field">
            <mat-label>System prompt</mat-label>
            <textarea matInput id="pw-preset-system" rows="4" [attr.maxlength]="promptMax" placeholder="Added in front of the conversation only when the request has no system message of its own." [value]="form().systemPrompt" (input)="set('systemPrompt', $any($event.target).value)"></textarea>
          </mat-form-field>

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
            <button type="button" matButton (click)="store.closeEditor()">Cancel</button>
            <button type="submit" matButton="filled" [disabled]="!canSave()">{{ ed.editing === null ? 'Create preset' : 'Save preset' }}</button>
          </div>
        </form>
      </pw-panel>
    }
  `,
  styles: `
    :host {
      display: grid;
      gap: var(--pd-space-3);
    }
    .list {
      margin-top: var(--pd-space-2);
    }
    .alias {
      font-weight: var(--pd-weight-medium);
    }
    .target,
    .via {
      font-size: var(--pd-text-label);
    }
    .target {
      display: block;
      overflow: hidden;
      text-overflow: ellipsis;
    }
    /* One line per preset: the verdict, then what serves it (or why nothing does), truncated with a tooltip. */
    .via {
      display: inline-block;
      max-width: calc(100% - 96px);
      margin-left: var(--pd-space-2);
      overflow: hidden;
      vertical-align: bottom;
      text-overflow: ellipsis;
      color: var(--pd-text-muted);
    }
    .defs {
      font-size: var(--pd-text-label);
      color: var(--pd-text-muted);
      text-overflow: ellipsis;
    }
    .mat-column-alias {
      width: 24%;
    }
    .mat-column-target,
    .mat-column-defaults {
      width: 20%;
    }
    /* Edit and the "..." menu, with the cell's padding. */
    .mat-column-act {
      width: 84px;
    }

    .actions {
      display: inline-flex;
      gap: var(--pd-space-1);
      align-items: center;
    }
    .editor {
      display: grid;
      gap: var(--pd-space-3);
    }
    .grid {
      display: grid;
      grid-template-columns: repeat(2, minmax(0, 1fr));
      gap: var(--pd-space-3);
      /* Four across when the panel has room, then two, then one: the fields keep a usable width. */
      &.four {
        grid-template-columns: repeat(auto-fit, minmax(180px, 1fr));
      }
    }
    @media (max-width: 760px) {
      .grid {
        grid-template-columns: minmax(0, 1fr);
      }
    }
    .field {
      width: 100%;
      min-width: 0;
    }
    .toggle-row {
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: var(--pd-space-6);
      > div {
        display: flex;
        flex-direction: column;
        gap: 2px;
        min-width: 0;
      }
      .t {
        font-weight: var(--pd-weight-medium);
      }
      .d {
        font-size: var(--pd-text-label);
      }
    }
    .badge {
      display: inline-block;
      margin-left: var(--pd-space-2);
      padding: 0 6px;
      font-size: var(--pd-text-label);
      font-weight: var(--pd-weight-medium);
      line-height: 18px;
      color: var(--pd-text-muted);
      background: var(--pd-pressed);
      border-radius: var(--pd-radius-pill);
    }
    .hint {
      margin: 0;
      font-size: var(--pd-text-label);
    }
    .err {
      display: flex;
      gap: var(--pd-space-2);
      align-items: center;
      margin: 0;
      padding: var(--pd-space-2) var(--pd-space-3);
      font-size: var(--pd-text-label);
      background: var(--pd-danger-tint);
      border-radius: var(--pd-radius-control);
      mat-icon {
        flex: none;
        color: var(--pd-danger);
      }
    }
    .preview {
      padding: var(--pd-space-3);
      background: var(--pd-surface-2);
      border-radius: var(--pd-radius-control);
      p {
        display: flex;
        flex-wrap: wrap;
        gap: var(--pd-space-2);
        align-items: center;
        margin: 0;
        font-size: var(--pd-text-label);
      }
      p + p {
        margin-top: var(--pd-space-2);
      }
      .bad {
        color: var(--pd-danger);
      }
    }
    .foot {
      display: flex;
      justify-content: flex-end;
      gap: var(--pd-space-2);
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

  private readonly inputs = viewChildren(MatInput);

  constructor() {
    void this.store.load();
    // effect-state: pushes the parsed form's invalid fields into Material's error tracker, which has no signal input.
    effect(() => {
      this.parsed();
      for (const i of this.inputs()) i.updateErrorState();
    });
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

  /**
   * The number fields show Material's error state (red outline, `aria-invalid`) while their text is not a plain number. The
   * fields have no form control, so Material does not re-check them by itself: the effect below asks it to on every change.
   */
  protected readonly invalid: Record<'context' | 'temperature' | 'topP' | 'maxTokens', ErrorStateMatcher> = {
    context: { isErrorState: () => this.bad('context') },
    temperature: { isErrorState: () => this.bad('temperature') },
    topP: { isErrorState: () => this.bad('topP') },
    maxTokens: { isErrorState: () => this.bad('maxTokens') },
  };

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
  /** Defaults only when the presets differ in them (UI review 2 rule 7: no constant column). */
  protected readonly columns = computed(() => ['alias', 'target', 'serves', ...(varies(this.store.entries(), (e) => this.defaultsText(e.preset)) ? ['defaults'] : []), 'act']);
  protected readonly byAlias = (_: number, e: PresetEntry) => e.preset.alias;

  protected menuItems(p: ModelPreset): MenuItem[] {
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
    } else {
      moveRowFocus(e);
    }
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
