import { ChangeDetectionStrategy, Component, computed, input } from '@angular/core';
import { splitVersion } from '../identity';

/** A version in the UI font with its build or pre-release tail dimmed (`R3.10.00` + `T32`). */
@Component({
  selector: 'sp-ver',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `<span class="ver">{{ parts().main }}</span
    >@if (parts().suffix) {<span class="sfx">{{ parts().suffix }}</span>}`,
  styles: `
    :host {
      white-space: nowrap;
    }
    .sfx {
      color: var(--pd-text-subtle);
      font-weight: var(--pd-weight-regular);
    }
  `,
})
export class Version {
  readonly value = input.required<string>();
  protected readonly parts = computed(() => splitVersion(this.value()));
}
