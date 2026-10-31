import { afterNextRender, ChangeDetectionStrategy, Component, computed, ElementRef, inject, Injector, linkedSignal, resource, signal } from '@angular/core';
import { MatButton } from '@angular/material/button';
import { FormField } from '@angular/forms/signals';
import { PdField, PdFieldControl } from '@decryp7/paddock-ui/forms';
import { SuEmptyState } from '@decryp7/paddock-ui';
import { PdGlyph } from '@decryp7/paddock-ui/icons';
import { DIALOG_REF, type DialogAction, type DialogContent, type DialogRef } from '@decryp7/paddock-ui/shell/dialogs';
import { DOCK_OPTIONS, storagePrefix } from './dock-options';
import { DOCK_LAYOUT_STORE } from './dock-store';
import { layoutNameForm } from './layout-name-form';
import { cleanLayoutName, type NamedLayout, parseNamedLayouts, removeLayout, renameLayout, serializeNamedLayouts } from './named-layouts';

/**
 * What the manage dialog resolves: the layout to apply, or `save` (the user asked to save the current layout: the dock
 * runs its Save layout flow). Both close the dialog; the result is empty when it was just closed.
 */
export interface LayoutManageResult {
  readonly apply?: string;
  readonly save?: true;
}

/**
 * "Manage layouts" (a modeless form window that fits its list, ADR 0009): the named layouts with Apply, Rename and Delete. Edits are written
 * to the layout store as they are made (the window may be closed any way), so the host reads the list again when the window
 * closes. Delete has no confirmation: the row goes and an Undo strip puts it back (until another change is made or the window
 * closes). Apply resolves `{ apply: name }` and closes the window. The window reads and writes the layout document of the
 * store it is given (`DOCK_LAYOUT_STORE`) and calls nothing else.
 */
@Component({
  selector: 'pd-layout-manage-dialog',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [PdField, PdFieldControl, FormField, PdGlyph, SuEmptyState, MatButton],
  template: `
    <div class="pd-lmd">
      @if (loaded.status() === 'loading') {
        <p class="pd-lmd__note" data-testid="layout-manage-loading">{{ t.loading }}</p>
      } @else if (layouts().length === 0) {
        <pd-empty-state data-testid="layout-manage-empty" icon="layers" [heading]="t.emptyHeading" [description]="t.empty">
          <button slot="action" type="button" class="pd-lmd__btn pd-lmd__btn--primary" data-testid="layout-manage-save-empty" (click)="saveCurrent()">{{ t.saveCurrent }}</button>
        </pd-empty-state>
      } @else {
        <ul class="pd-lmd__list" data-testid="layout-manage-list">
          @for (layout of layouts(); track layout.name) {
            <li class="pd-lmd__row" [attr.data-layout]="layout.name">
              @if (renaming() === layout.name) {
                <pd-field class="pd-lmd__rename" [label]="t.renameLabel" [state]="rename.name()" [submitted]="renameSubmitted()">
                  <input
                    class="pd-lmd__input"
                    type="text"
                    autocomplete="off"
                    spellcheck="false"
                    pdFieldControl
                    [formField]="rename.name"
                    data-testid="layout-manage-rename"
                    (keydown.enter)="commitRename($event)"
                    (keydown.escape)="cancelRename($event)"
                  />
                </pd-field>
                <button type="button" matButton="outlined" class="pd-lmd__rbtn" data-testid="layout-manage-rename-ok" (click)="commitRename($event)">{{ t.rename }}</button>
                <button type="button" matButton="outlined" class="pd-lmd__rbtn" data-testid="layout-manage-rename-cancel" (click)="cancelRename($event)">{{ t.cancel }}</button>
              } @else {
                <span class="pd-lmd__name" [attr.title]="layout.name">{{ layout.name }}</span>
                <button type="button" class="pd-lmd__btn" data-testid="layout-manage-apply" [attr.aria-label]="t.applyLabel(layout.name)" (click)="apply(layout.name)">{{ t.apply }}</button>
                <button type="button" class="pd-lmd__btn" data-testid="layout-manage-rename-start" [attr.aria-label]="t.renameFor(layout.name)" (click)="startRename(layout.name)">{{ t.rename }}</button>
                <button type="button" class="pd-lmd__btn pd-lmd__btn--danger" data-testid="layout-manage-delete" [attr.aria-label]="t.deleteFor(layout.name)" (click)="remove(layout.name)">
                  <pd-glyph name="trash" [size]="14" /><span>{{ t.delete }}</span>
                </button>
              }
            </li>
          }
        </ul>
      }
      @if (undo(); as u) {
        <div class="pd-lmd__undo" role="status" data-testid="layout-manage-undo">
          <span>{{ t.deleted(u.name) }}</span>
          <button type="button" class="pd-lmd__btn" data-testid="layout-manage-undo-button" (click)="restore()">{{ t.undo }}</button>
        </div>
      }
    </div>
  `,
  styles: `
    :host { display: block; color: var(--pd-text); min-width: 0; }
    .pd-lmd { display: flex; flex-direction: column; gap: var(--pd-space-3); container-type: inline-size; }
    .pd-lmd__note { margin: 0; color: var(--pd-text-muted); }
    .pd-lmd__list { margin: 0; padding: 0; list-style: none; display: flex; flex-direction: column; border: 1px solid var(--pd-hairline); border-radius: var(--pd-radius-control); }
    .pd-lmd__row { display: flex; flex-wrap: wrap; align-items: center; gap: var(--pd-space-2); padding: var(--pd-space-2) var(--pd-space-3); }
    .pd-lmd__row + .pd-lmd__row { border-top: 1px solid var(--pd-hairline); }
    .pd-lmd__name { flex: 1 1 8rem; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .pd-lmd__row:has(.pd-lmd__rename) { align-items: flex-start; }
    .pd-lmd__rename { flex: 1 1 12rem; }
    /* The buttons are the labelled outline's height and start at its top, so their centres are the field's. */
    .pd-lmd__rbtn { --mat-button-outlined-container-height: var(--mat-form-field-container-height); }
    .pd-lmd__btn { display: inline-flex; align-items: center; gap: var(--pd-space-1); min-height: var(--pd-button-height, 28px); padding: 0 var(--pd-space-3); border-radius: var(--pd-radius-control); border: 1px solid var(--pd-border-control); background: var(--pd-surface-1); color: var(--pd-text); font: inherit; font-weight: var(--pd-weight-medium); cursor: pointer; }
    .pd-lmd__btn--primary { background: var(--pd-accent-fill); border-color: var(--pd-accent-fill); color: var(--pd-on-accent); }
    .pd-lmd__btn--danger { color: var(--pd-danger-text, var(--pd-danger)); }
    .pd-lmd__btn:focus-visible { outline: 2px solid var(--pd-focus-ring); outline-offset: 1px; }
    .pd-lmd__undo { display: flex; align-items: center; justify-content: space-between; gap: var(--pd-space-2); padding: var(--pd-space-2) var(--pd-space-3); border-radius: var(--pd-radius-control); background: var(--pd-surface-3); }
  `,
})
export class PdLayoutManageDialog implements DialogContent<LayoutManageResult> {
  protected readonly ref = inject<DialogRef<LayoutManageResult>>(DIALOG_REF);
  private readonly host = inject<ElementRef<HTMLElement>>(ElementRef);
  private readonly injector = inject(Injector);
  private readonly store = inject(DOCK_LAYOUT_STORE);
  private readonly key = `${storagePrefix(inject(DOCK_OPTIONS))}.layouts`;

  protected readonly t = {
    loading: $localize`:@@pd.wb.dock.layout.manage-loading:Loading layouts…`,
    emptyHeading: $localize`:@@pd.wb.dock.layout.manage-empty-heading:No saved layouts yet`,
    empty: $localize`:@@pd.wb.dock.layout.manage-empty:Save the current arrangement of your panels to come back to it later.`,
    saveCurrent: $localize`:@@pd.wb.dock.layout.manage-save-current:Save current layout`,
    apply: $localize`:@@pd.wb.dock.layout.manage-apply:Apply`,
    rename: $localize`:@@pd.wb.dock.layout.manage-rename:Rename`,
    renameLabel: $localize`:@@pd.wb.dock.layout.manage-rename-label:New name`,
    cancel: $localize`:@@pd.wb.dock.layout.manage-cancel:Cancel`,
    delete: $localize`:@@pd.wb.dock.layout.manage-delete:Delete`,
    undo: $localize`:@@pd.wb.dock.layout.manage-undo:Undo`,
    applyLabel: (name: string) => $localize`:@@pd.wb.dock.layout.manage-apply-for:Apply layout ${name}:name:`,
    renameFor: (name: string) => $localize`:@@pd.wb.dock.layout.manage-rename-for:Rename layout ${name}:name:`,
    deleteFor: (name: string) => $localize`:@@pd.wb.dock.layout.manage-delete-for:Delete layout ${name}:name:`,
    deleted: (name: string) => $localize`:@@pd.wb.dock.layout.manage-deleted:Deleted layout ${name}:name:.`,
  };

  protected readonly loaded = resource({ loader: () => this.store.load(this.key) });
  /** The list as the user is editing it; it starts from what the store holds. */
  protected readonly layouts = linkedSignal<readonly NamedLayout[]>(() => (this.loaded.hasValue() ? parseNamedLayouts(this.loaded.value()) : []));
  /** The layout being renamed, if any. */
  protected readonly renaming = signal<string | null>(null);
  protected readonly renameSubmitted = signal(false);
  protected readonly rename = layoutNameForm(
    () => this.renaming() ?? '',
    () => this.layouts().map((l) => l.name),
    () => this.renaming() ?? '',
  );
  /** The layout just deleted, and the place it had, for Undo. */
  protected readonly undo = signal<{ name: string; layout: NamedLayout; index: number } | null>(null);
  private readonly names = computed(() => this.layouts().map((l) => l.name));

  private write(list: readonly NamedLayout[]): void {
    this.layouts.set(list);
    void this.store.save(this.key, serializeNamedLayouts(list)).catch(() => undefined);
  }

  /** Save current layout at the leading edge once there are layouts (the empty state carries its own button); the frame adds Close. */
  readonly dialogActions = computed<readonly DialogAction<LayoutManageResult>[]>(() =>
    this.loaded.status() === 'loading' || this.layouts().length === 0
      ? []
      : [{ kind: 'answer', id: 'save', label: this.t.saveCurrent, role: 'secondary', lead: true, answer: () => ({ save: true }) }],
  );

  protected saveCurrent(): void {
    this.ref.answer({ save: true });
  }

  protected apply(name: string): void {
    this.ref.answer({ apply: name });
  }

  protected startRename(name: string): void {
    this.undo.set(null);
    this.renameSubmitted.set(false);
    this.renaming.set(name);
    afterNextRender(() => this.host.nativeElement.querySelector<HTMLInputElement>('[data-testid="layout-manage-rename"]')?.focus(), { injector: this.injector });
  }

  protected cancelRename(event?: Event): void {
    event?.stopPropagation();
    this.endRename();
  }

  /** The rename field goes: focus returns to that row's Rename button (it would otherwise fall to the page). */
  private endRename(name: string | null = this.renaming()): void {
    this.renaming.set(null);
    afterNextRender(
      () => {
        const row = [...this.host.nativeElement.querySelectorAll<HTMLElement>('li[data-layout]')].find((li) => li.dataset['layout'] === name);
        row?.querySelector<HTMLElement>('[data-testid="layout-manage-rename-start"]')?.focus();
      },
      { injector: this.injector },
    );
  }

  protected commitRename(event?: Event): void {
    event?.stopPropagation();
    const from = this.renaming();
    if (from === null) return;
    this.renameSubmitted.set(true);
    const field = this.rename.name();
    if (field.invalid()) return;
    const to = cleanLayoutName(field.value());
    if (to && to !== from) this.write(renameLayout(this.layouts(), from, to));
    this.endRename(to && to !== from ? to : from);
  }

  protected remove(name: string): void {
    const index = this.names().indexOf(name);
    const layout = this.layouts()[index];
    if (!layout) return;
    this.renaming.set(null);
    this.undo.set({ name, layout, index });
    this.write(removeLayout(this.layouts(), name));
  }

  protected restore(): void {
    const u = this.undo();
    if (!u) return;
    this.undo.set(null);
    if (this.names().includes(u.name)) return;
    const next = [...this.layouts()];
    next.splice(Math.min(u.index, next.length), 0, u.layout);
    this.write(next);
  }
}
