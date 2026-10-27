import { ChangeDetectionStrategy, Component, computed, input, output } from '@angular/core';
import { EXTENSION_CHIPS, extensionList, toggleExtension } from './draft-utils';

/** File types as toggle chips (iso, exe, msi, zip, dmg, pkg); other types are typed in the field above. */
@Component({
  selector: 'sp-extension-chips',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="chips" role="group" aria-label="File types">
      @for (e of chips; track e) {
        <button type="button" class="chip-btn" [class.on]="active().includes(e)" [attr.aria-pressed]="active().includes(e)" (click)="toggle(e)">.{{ e }}</button>
      }
      @for (e of custom(); track e) {
        <button type="button" class="chip-btn on" aria-pressed="true" [attr.aria-label]="'Remove .' + e" (click)="toggle(e)">.{{ e }} ×</button>
      }
    </div>
  `,
  styles: `
    .chips {
      display: flex;
      flex-wrap: wrap;
      gap: var(--pd-space-1);
    }
    .chip-btn {
      padding: 2px var(--pd-space-3);
      border: 1px solid var(--pd-hairline);
      border-radius: var(--pd-radius-pill);
      background: transparent;
      color: var(--pd-text-muted);
      font: inherit;
    }
    .chip-btn.on {
      border-color: var(--pd-accent);
      background: var(--pd-accent-tint);
      color: var(--pd-text);
    }
  `,
})
export class ExtensionChips {
  /** The comma separated list in the form. */
  readonly value = input('');
  readonly valueChange = output<string>();

  protected readonly chips = EXTENSION_CHIPS;
  protected readonly active = computed(() => extensionList(this.value()));
  protected readonly custom = computed(() => this.active().filter((e) => !(EXTENSION_CHIPS as readonly string[]).includes(e)));

  protected toggle(ext: string): void {
    this.valueChange.emit(toggleExtension(this.value(), ext, !this.active().includes(ext)));
  }
}
