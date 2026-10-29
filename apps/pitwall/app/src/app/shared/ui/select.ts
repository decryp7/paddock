import { CdkListbox, CdkOption, type ListboxValueChangeEvent } from '@angular/cdk/listbox';
import { CdkConnectedOverlay, CdkOverlayOrigin } from '@angular/cdk/overlay';
import { ChangeDetectionStrategy, Component, computed, input, output, signal } from '@angular/core';
import { MatIconModule } from '@angular/material/icon';

export interface SelectOption {
  value: string;
  label: string;
  /** Shown but not choosable. */
  disabled?: boolean;
}

/** A themed single-choice dropdown (CDK listbox in an overlay) that replaces the OS's native select popup. */
@Component({
  selector: 'pw-select',
  imports: [CdkConnectedOverlay, CdkOverlayOrigin, CdkListbox, CdkOption, MatIconModule],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <button
      type="button"
      class="trigger"
      cdkOverlayOrigin
      #origin="cdkOverlayOrigin"
      [attr.id]="inputId()"
      [disabled]="disabled()"
      [attr.aria-label]="label() ? label() + ': ' + current() : null"
      aria-haspopup="listbox"
      [attr.aria-expanded]="open()"
      (click)="open.set(!open())"
    >
      <span class="txt">{{ current() }}</span><mat-icon svgIcon="pw:chevron-down" aria-hidden="true" />
    </button>
    <ng-template cdkConnectedOverlay [cdkConnectedOverlayOrigin]="origin" [cdkConnectedOverlayOpen]="open()" [cdkConnectedOverlayMinWidth]="minWidth()" [cdkConnectedOverlayPush]="true" [cdkConnectedOverlayViewportMargin]="12" (overlayOutsideClick)="open.set(false)" (detach)="open.set(false)">
      <ul class="pw-select-list" [tabindex]="0" cdkListbox [attr.aria-label]="label()" [cdkListboxValue]="[value()]" (cdkListboxValueChange)="pick($event)" (click)="open.set(false)" (keydown.escape)="open.set(false)">
        @for (o of options(); track o.value) {
          <li class="opt" [cdkOption]="o.value" [cdkOptionDisabled]="!!o.disabled" cdkOptionTypeaheadLabel="{{ o.label }}"><mat-icon svgIcon="pw:check" aria-hidden="true" class="tick" />{{ o.label }}</li>
        }
      </ul>
    </ng-template>
  `,
  styles: `
    /* 160 px unless the layout gives it less: the label then ends in an ellipsis (the options list keeps its width). */
    :host {
      display: inline-block;
      width: 160px;
      min-width: 0;
      max-width: 100%;
    }
    .trigger {
      display: inline-flex;
      align-items: center;
      justify-content: space-between;
      gap: var(--pd-space-2);
      width: 100%;
      min-width: 0;
      height: 30px;
      padding: 0 var(--pd-space-2) 0 var(--pd-space-3);
      font: inherit;
      font-size: var(--pd-text-body);
      color: var(--pd-text);
      text-align: left;
      background: var(--pd-surface-1);
      border: 1px solid var(--pd-border-control);
      border-radius: var(--pd-radius-control);
      cursor: default;
      outline: none;
      transition:
        border-color var(--pd-duration-micro) var(--pd-ease-move),
        box-shadow var(--pd-duration-micro) var(--pd-ease-move);
      &:focus-visible,
      &[aria-expanded='true'] {
        border-color: var(--pd-accent);
        box-shadow: 0 0 0 3px var(--pd-accent-tint);
      }
      &:disabled {
        opacity: 0.5;
      }
      mat-icon {
        flex: none;
        width: 14px;
        height: 14px;
        color: var(--pd-text-muted);
      }
    }
    .txt {
      overflow: hidden;
      white-space: nowrap;
      text-overflow: ellipsis;
    }
  `,
})
export class PwSelect {
  readonly options = input.required<SelectOption[]>();
  readonly value = input.required<string>();
  /** Accessible name, announced with the current choice. */
  readonly label = input('');
  readonly inputId = input<string | null>(null);
  readonly disabled = input(false);
  readonly changed = output<string>();

  protected readonly open = signal(false);
  protected readonly current = computed(() => this.options().find((o) => o.value === this.value())?.label ?? '');
  protected readonly minWidth = signal(180);

  protected pick(e: ListboxValueChangeEvent<string>): void {
    const v = e.value[0];
    this.open.set(false);
    if (v !== undefined && v !== this.value()) this.changed.emit(v);
  }
}
