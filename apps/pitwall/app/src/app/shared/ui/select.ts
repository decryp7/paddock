import { CdkListbox, CdkOption, type ListboxValueChangeEvent } from '@angular/cdk/listbox';
import { CdkConnectedOverlay, CdkOverlayOrigin } from '@angular/cdk/overlay';
import { ChangeDetectionStrategy, Component, computed, input, output, signal } from '@angular/core';
import { MatIconModule } from '@angular/material/icon';

export interface SelectOption {
  value: string;
  label: string;
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
          <li class="opt" [cdkOption]="o.value" cdkOptionTypeaheadLabel="{{ o.label }}"><mat-icon svgIcon="pw:check" aria-hidden="true" class="tick" />{{ o.label }}</li>
        }
      </ul>
    </ng-template>
  `,
  styles: `
    :host {
      display: inline-block;
      max-width: 100%;
    }
    .trigger {
      display: inline-flex;
      align-items: center;
      justify-content: space-between;
      gap: var(--sp-2);
      width: 100%;
      min-width: 160px;
      height: 30px;
      padding: 0 var(--sp-2) 0 var(--sp-3);
      font: inherit;
      font-size: var(--fs-base);
      color: var(--text);
      text-align: left;
      background: var(--bg-input);
      border: 1px solid var(--border-strong);
      border-radius: var(--r-md);
      cursor: default;
      outline: none;
      transition:
        border-color var(--dur-fast) var(--ease),
        box-shadow var(--dur-fast) var(--ease);
      &:focus-visible,
      &[aria-expanded='true'] {
        border-color: var(--accent);
        box-shadow: 0 0 0 3px var(--accent-soft);
      }
      &:disabled {
        opacity: 0.5;
      }
      mat-icon {
        flex: none;
        width: 14px;
        height: 14px;
        color: var(--text-2);
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
