import { Dialog } from '@angular/cdk/dialog';
import { Injectable, inject } from '@angular/core';

/** Opens the registration wizard (loaded on first use). */
@Injectable({ providedIn: 'root' })
export class RegisterLauncher {
  private readonly dialog = inject(Dialog);

  async open(): Promise<void> {
    const { RegisterWizard } = await import('../features/register/register-wizard');
    this.dialog.open(RegisterWizard, {
      ariaLabelledBy: 'register-title',
      panelClass: 'pc-dialog-panel',
      backdropClass: 'pc-dialog-backdrop',
      disableClose: true,
    });
  }
}
