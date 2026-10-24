import { CdkConnectedOverlay, CdkOverlayOrigin, type ConnectedPosition } from '@angular/cdk/overlay';
import { CdkListbox, CdkOption, type ListboxValueChangeEvent } from '@angular/cdk/listbox';
import { afterNextRender, ChangeDetectionStrategy, Component, computed, ElementRef, inject, Injector, input, output, signal, viewChild } from '@angular/core';
import { PdGlyph } from '@decryp7/paddock-ui/icons';

/** One choice of a `pd-select`. */
export interface PdSelectOption {
  readonly value: string;
  readonly label: string;
  readonly disabled?: boolean;
}

let nextId = 0;

const POSITIONS: ConnectedPosition[] = [
  { originX: 'start', originY: 'bottom', overlayX: 'start', overlayY: 'top', offsetY: 4 },
  { originX: 'start', originY: 'top', overlayX: 'start', overlayY: 'bottom', offsetY: -4 },
];

/**
 * A themed, non-native select (design guide §8, popovers): the trigger shows the chosen label, the list is a popover on the
 * surface tokens (a native `<select>` is drawn by the OS and ignores the theme, accent and density). Built on the CDK
 * listbox, so keyboard navigation, typeahead and the ARIA roles are the platform-standard ones: Enter, Space, Alt+Down or
 * Down opens, Up/Down/Home/End/typed letters move, Enter chooses, Escape or a click outside closes and returns focus to the
 * trigger. The list is at least as wide as the trigger and flips above it near the window's bottom edge.
 *
 * ```html
 * <pd-select [options]="themes" [value]="theme()" ariaLabel="Theme" (valueChange)="theme.set($event)" />
 * ```
 */
@Component({
  selector: 'pd-select',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [CdkConnectedOverlay, CdkOverlayOrigin, CdkListbox, CdkOption, PdGlyph],
  host: { '[class.pd-open]': 'open()', '[class.pd-invalid]': 'invalid()' },
  template: `
    <button
      #trigger
      type="button"
      cdkOverlayOrigin
      #origin="cdkOverlayOrigin"
      class="pd-trigger"
      role="combobox"
      aria-haspopup="listbox"
      [id]="inputId() || id"
      [attr.aria-expanded]="open()"
      [attr.aria-controls]="open() ? id + '-list' : null"
      [attr.aria-label]="ariaLabel() || null"
      [attr.aria-describedby]="describedBy() || null"
      [attr.aria-invalid]="invalid() || null"
      [disabled]="disabled()"
      (click)="toggle()"
      (keydown)="triggerKey($event)"
    >
      <span class="pd-value" [class.pd-placeholder]="!chosen()">{{ label() }}</span>
      <pd-glyph class="pd-chevron" name="chevron-down" [size]="14" />
    </button>
    <ng-template
      cdkConnectedOverlay
      [cdkConnectedOverlayOrigin]="origin"
      [cdkConnectedOverlayOpen]="open()"
      [cdkConnectedOverlayPositions]="positions"
      [cdkConnectedOverlayWidth]="width()"
      (detach)="closed()"
      (overlayOutsideClick)="closed()"
      (attach)="focusList()"
    >
      <ul #list cdkListbox class="pd-list" [id]="id + '-list'" [attr.aria-label]="ariaLabel() || null" [cdkListboxValue]="chosen() ? [value()] : []" (cdkListboxValueChange)="pick($event)" (keydown.escape)="escape($event)">
        @for (o of options(); track o.value) {
          <li cdkOption class="pd-option" [cdkOption]="o.value" [cdkOptionDisabled]="o.disabled === true" [cdkOptionTypeaheadLabel]="o.label">
            <span class="pd-option-label">{{ o.label }}</span>
            @if (o.value === value()) {
              <pd-glyph class="pd-check" name="check" [size]="14" />
            }
          </li>
        }
      </ul>
    </ng-template>
  `,
  styles: `
    :host { display: inline-flex; min-width: 0; max-width: 100%; }
    .pd-trigger { display: inline-flex; align-items: center; justify-content: space-between; gap: var(--pd-space-2); min-width: 0; width: 100%; box-sizing: border-box; min-height: var(--pd-control-height); padding: 0 var(--pd-space-2) 0 var(--pd-space-3); border: 1px solid var(--pd-border-control); border-radius: var(--pd-radius-input); background: var(--pd-surface-1); color: var(--pd-text); font: inherit; text-align: start; cursor: default; }
    :host(:not(.pd-open, .pd-invalid)) .pd-trigger:hover:not(:disabled, :focus-visible) { border-color: var(--pd-text-muted); background: var(--pd-surface-2); }
    .pd-trigger:focus-visible, :host(.pd-open) .pd-trigger { border-color: var(--pd-accent); outline: 2px solid var(--pd-focus-ring); outline-offset: 2px; }
    .pd-trigger:disabled { opacity: 0.6; cursor: not-allowed; }
    :host(.pd-invalid) .pd-trigger { border-color: var(--pd-danger); }
    .pd-value { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .pd-placeholder { color: var(--pd-text-subtle); }
    .pd-chevron { color: var(--pd-text-muted); }
    :host(.pd-open) .pd-chevron { transform: rotate(180deg); }
    .pd-list { box-sizing: border-box; flex: 1 1 auto; min-width: 0; width: 100%; max-height: 18rem; margin: 0; padding: var(--pd-space-1); overflow: auto; list-style: none; border: 1px solid var(--pd-hairline); border-radius: var(--pd-radius-popover); background: var(--pd-surface-3); color: var(--pd-text); box-shadow: var(--pd-shadow-2); outline: none; }
    .pd-option { display: flex; align-items: center; justify-content: space-between; gap: var(--pd-space-2); min-height: var(--pd-control-height); padding: 0 var(--pd-space-2); border-radius: var(--pd-radius-control); cursor: default; }
    .pd-option:hover, .pd-option.cdk-option-active, .pd-option:focus-visible { background: var(--pd-hover); outline: none; }
    .pd-option[aria-selected='true'] { font-weight: var(--pd-weight-medium); }
    .pd-option[aria-disabled='true'] { opacity: 0.5; }
    .pd-check { color: var(--pd-accent); }
  `,
})
export class PdSelect {
  /** The choices, in order. */
  readonly options = input.required<readonly PdSelectOption[]>();
  /** The chosen value; none of the options (or '') shows the placeholder. */
  readonly value = input<string>('');
  /** Text for no choice. */
  readonly placeholder = input<string>('');
  readonly disabled = input(false);
  readonly invalid = input(false);
  /** The accessible name when no `<label for>` names the trigger. */
  readonly ariaLabel = input<string>('');
  /** Space-separated ids of the hint and error texts. */
  readonly describedBy = input<string>('');
  /** The trigger's id, so a `<label for>` names it. */
  readonly inputId = input<string>('');
  /** A choice was made. */
  readonly valueChange = output<string>();

  protected readonly id = `pd-select-${++nextId}`;
  protected readonly positions = POSITIONS;
  protected readonly open = signal(false);
  protected readonly width = signal(0);
  private readonly trigger = viewChild.required<ElementRef<HTMLButtonElement>>('trigger');
  private readonly list = viewChild<ElementRef<HTMLElement>>('list');
  private readonly injector = inject(Injector);

  protected readonly chosen = computed(() => this.options().find((o) => o.value === this.value()) ?? null);
  protected readonly label = computed(() => this.chosen()?.label ?? this.placeholder());

  protected toggle(): void {
    if (this.open()) this.close(true);
    else this.show();
  }

  private show(): void {
    if (this.disabled()) return;
    this.width.set(this.trigger().nativeElement.getBoundingClientRect().width);
    this.open.set(true);
  }

  /** Down, Alt+Down, Enter and Space open the list (Space and Enter also click the button, which toggles: only Down is ours). */
  protected triggerKey(e: KeyboardEvent): void {
    if ((e.key === 'ArrowDown' || e.key === 'ArrowUp') && !this.open()) {
      e.preventDefault();
      this.show();
    }
  }

  /** The list opened: focus goes into it (the chosen option, else the first). */
  protected focusList(): void {
    afterNextRender(() => this.list()?.nativeElement.focus(), { injector: this.injector });
  }

  protected pick(e: ListboxValueChangeEvent<string>): void {
    const next = e.value[0];
    if (next === undefined) return;
    this.close(true);
    if (next !== this.value()) this.valueChange.emit(next);
  }

  /** Escape closes the list and nothing else (the window's own Escape must not also close the dialog). */
  protected escape(e: Event): void {
    e.stopPropagation();
    this.close(true);
  }

  /** The overlay detached (Escape, a click outside, or a choice): the list is closed. */
  protected closed(): void {
    this.close(false);
  }

  private close(refocus: boolean): void {
    if (!this.open()) return;
    this.open.set(false);
    if (refocus) this.trigger().nativeElement.focus();
  }
}
