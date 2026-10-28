import { DialogRef } from '@angular/cdk/dialog';
import { ChangeDetectionStrategy, Component, computed, inject } from '@angular/core';
import { DialogFrame } from '../../shared/dialog';
import { Icon } from '../../shared/icon';
import { HelperStore } from '../../state/helper.store';

/**
 * Explains the privileged helper before the system asks for permission, installs it on the confirm click and closes with `true`
 * once it answers. "Not now" (or a dismissed system prompt) changes nothing and Dyno keeps working.
 */
@Component({
  selector: 'dy-helper-dialog',
  imports: [DialogFrame, Icon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <dy-dialog [title]="title()" subtitle="Some readings need administrator rights. Dyno itself never runs as administrator." icon="shield">
      @if (unsupported()) {
        <p class="msg" role="status">{{ store.status()?.message }}</p>
      } @else {
        <p class="msg">A small separate helper answers only those questions, for your account only. It changes nothing on your computer.</p>
        <ul>
          <li>It can read a fixed list of things, such as programs' network traffic on Windows, and remove itself.</li>
          <li>Every request is written to the helper's own log, which Settings shows.</li>
          <li>You can remove it any time under Settings, Helper. Nothing else in Dyno needs it.</li>
        </ul>
        <p class="prompt"><dy-icon name="info" /> {{ store.status()?.prompt }}</p>
        @if (store.working()) {
          <p class="wait" role="status" aria-live="polite">Waiting for you to answer the system's permission prompt…</p>
        }
        @if (problem(); as p) {
          <p class="problem" role="alert">{{ p }}</p>
        }
      }
      <ng-container footer>
        <button type="button" class="btn" [disabled]="store.working()" (click)="ref.close(false)">{{ unsupported() ? 'Close' : 'Not now' }}</button>
        @if (!unsupported()) {
          <button type="button" class="btn primary" [disabled]="store.working()" (click)="install()">{{ action() }}</button>
        }
      </ng-container>
    </dy-dialog>
  `,
  styles: `
    .msg { color: var(--text-2); }
    ul { margin: 0; padding-left: var(--sp-4); display: grid; gap: var(--sp-1); font-size: var(--fs-sm); color: var(--text-2); }
    .prompt { display: flex; gap: var(--sp-2); align-items: flex-start; font-size: var(--fs-sm); padding: var(--sp-3); border: 1px solid var(--border); border-radius: var(--r-md); background: var(--surface-inset); }
    .wait { color: var(--text-2); font-size: var(--fs-sm); }
    .problem { color: var(--bad); font-size: var(--fs-sm); }
  `,
})
export class HelperDialog {
  protected readonly store = inject(HelperStore);
  protected readonly ref = inject<DialogRef<boolean>>(DialogRef);
  protected readonly unsupported = computed(() => this.store.status()?.state === 'unsupported');
  protected readonly title = computed(() => {
    const state = this.store.status()?.state;
    return state === 'outdated' ? 'Update the Dyno helper' : state === 'unreachable' ? 'Repair the Dyno helper' : 'Install the Dyno helper';
  });
  protected readonly action = computed(() => {
    if (this.problem()) return 'Try again';
    const state = this.store.status()?.state;
    return state === 'outdated' ? 'Update the helper' : state === 'unreachable' ? 'Repair the helper' : 'Install the helper';
  });
  protected readonly problem = computed(() => this.store.error()?.message ?? this.store.status()?.error ?? null);

  protected async install(): Promise<void> {
    if (await this.store.install()) this.ref.close(true);
  }
}
