import { Injectable, inject } from '@angular/core';
import { firstValueFrom } from 'rxjs';
import { Dialogs } from '../../shared/dialog';
import { Notifier } from '../../shared/notifier';
import { HelperStore } from '../../state/helper.store';
import { HelperDialog } from './helper-dialog';

/**
 * Called before a change that needs administrator rights: resolves `true` when the helper is ready, and otherwise offers to install
 * it (the explanation, then the system's own prompt). Resolves `false` when the person says no or it cannot be installed; the caller
 * then does nothing.
 */
@Injectable({ providedIn: 'root' })
export class HelperGate {
  private readonly store = inject(HelperStore);
  private readonly dialogs = inject(Dialogs);
  private readonly notifier = inject(Notifier);

  async ensure(): Promise<boolean> {
    await this.store.load();
    const status = this.store.status();
    if (!status) {
      const e = this.store.error();
      if (e) this.notifier.error(e);
      return false;
    }
    if (status.state === 'ready') return true;
    const ref = this.dialogs.open<boolean>(HelperDialog);
    return (await firstValueFrom(ref.closed)) === true;
  }
}
