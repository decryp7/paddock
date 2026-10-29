import { ChangeDetectionStrategy, Component, computed, inject } from '@angular/core';
import { MatButton } from '@angular/material/button';
import { MatProgressBar } from '@angular/material/progress-bar';
import { SuStatusPill } from '@decryp7/paddock-ui';
import { PdSettingsSection } from '@decryp7/paddock-ui/shell/settings/window';
import { conditionView, LOGIN_SECTION, ServiceStore } from '@decryp7/paddock-ui/shell/service';
import { SERVICE_STYLES } from './styles';

/**
 * Settings > Background service (plan X4): how the service is doing, the buttons that apply (Install, Repair, Open Login
 * Items; only what the engine says applies, one of them primary), the live phases of a repair, an inline error with Retry,
 * and beneath it the "Start at login" section in the settings window's generic form. The launch-at-login switch is an
 * ordinary setting served by the settings commands, so it works while the service is down (rule 19).
 */
@Component({
  selector: 'pd-service-page',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [MatButton, MatProgressBar, SuStatusPill, PdSettingsSection],
  template: `
    <section class="pd-service__block" aria-labelledby="pd-service-title" data-testid="service-page">
      <h2 id="pd-service-title">{{ t.title }}</h2>
      <p class="pd-service__muted">{{ t.lede }}</p>
    </section>

    @if (store.status(); as s) {
      <div class="pd-service__status" data-testid="service-status">
        <pd-status-pill [status]="view().status" [label]="view().label" data-testid="service-condition" />
        @if (s.message) {
          <p data-testid="service-message">{{ s.message }}</p>
        }
      </div>

      @if (store.working()) {
        <div class="pd-service__notice" role="status" data-testid="service-working">
          <span class="pd-service__title">{{ store.notice()?.message ?? t.working }}</span>
          @if (store.step(); as step) {
            <span data-testid="service-step">{{ step }}</span>
          }
          <mat-progress-bar mode="indeterminate" [attr.aria-label]="t.working" />
          <span class="pd-service__muted" data-testid="service-still-working">{{ t.stillWorking }}</span>
        </div>
      } @else if (store.notice(); as n) {
        @if (n.state === 'succeeded') {
          <div class="pd-service__notice pd-service__notice--success" role="status" data-testid="service-outcome">
            <span class="pd-service__title">{{ n.message }}</span>
          </div>
        } @else if (n.state === 'failed' && !store.failure()) {
          <div class="pd-service__notice pd-service__notice--danger" role="alert" data-testid="service-outcome">
            <span class="pd-service__title">{{ n.message }}</span>
          </div>
        }
      }

      @if (store.failure(); as f) {
        <div class="pd-service__notice pd-service__notice--danger" role="alert" data-testid="service-failure">
          <span class="pd-service__title">{{ failureTitle(f.action) }}</span>
          <span>{{ f.error.message }}</span>
          @if (store.retryable()) {
            <span class="pd-service__row"><button matButton="outlined" type="button" data-testid="service-retry" (click)="store.retry()">{{ t.retry }}</button></span>
          }
        </div>
      }

      @if (store.canInstall() || store.canRepair() || store.canOpenLoginItems()) {
        <div class="pd-service__actions" role="group" [attr.aria-label]="t.actions">
          @if (store.canInstall()) {
            <button [matButton]="primary() === 'install' ? 'filled' : 'outlined'" type="button" data-testid="service-install" [disabled]="store.working()" (click)="store.install()">{{ t.install }}</button>
          }
          @if (store.canRepair()) {
            <button [matButton]="primary() === 'repair' ? 'filled' : 'outlined'" type="button" data-testid="service-repair" [disabled]="store.working()" (click)="store.repair()">{{ t.repair }}</button>
          }
          @if (store.canOpenLoginItems()) {
            <button [matButton]="primary() === 'openLoginItems' ? 'filled' : 'outlined'" type="button" data-testid="service-open-login-items" (click)="store.openLoginItems()">{{ t.openLoginItems }}</button>
          }
        </div>
      }
    } @else if (store.requestError(); as problem) {
      <div class="pd-service__notice pd-service__notice--danger" role="alert" data-testid="service-error">
        <span class="pd-service__title">{{ t.noStatus }}</span>
        <span>{{ problem }}</span>
        <span class="pd-service__row"><button matButton="outlined" type="button" data-testid="service-load-retry" (click)="store.load()">{{ t.retry }}</button></span>
      </div>
    } @else {
      <p class="pd-service__muted" role="status" data-testid="service-loading">{{ t.loading }}</p>
    }

    <pd-settings-section [section]="loginSection" />
  `,
  styles: SERVICE_STYLES,
})
export class PdServicePage {
  protected readonly store = inject(ServiceStore);
  protected readonly loginSection = LOGIN_SECTION;

  protected readonly t = {
    title: $localize`:@@pd.service.title:Background service`,
    lede: $localize`:@@pd.service.lede:A small helper that keeps working while the window is closed. This page shows how it is doing and repairs it when it is not.`,
    loading: $localize`:@@pd.service.loading:Asking the service how it is doing…`,
    noStatus: $localize`:@@pd.service.no-status-title:The service could not be asked how it is doing.`,
    retry: $localize`:@@pd.service.retry:Retry`,
    install: $localize`:@@pd.service.install:Install`,
    repair: $localize`:@@pd.service.repair:Repair`,
    openLoginItems: $localize`:@@pd.service.open-login-items:Open Login Items…`,
    actions: $localize`:@@pd.service.actions:Service actions`,
    working: $localize`:@@pd.service.working:Working…`,
    stillWorking: $localize`:@@pd.service.still-working:Still working. This can take a minute; nothing here gives up on it.`,
    repairFailed: $localize`:@@pd.service.repair-failed:The repair did not work.`,
    installFailed: $localize`:@@pd.service.install-failed:The install did not work.`,
    loginItemsFailed: $localize`:@@pd.service.login-items-not-opened:Login Items did not open.`,
  };

  protected readonly view = computed(() => conditionView(this.store.status()?.condition ?? 'unavailable'));

  /** The one primary button: the first that applies, in the order of what fixes most. */
  protected readonly primary = computed<'install' | 'repair' | 'openLoginItems' | null>(() => {
    if (this.store.canInstall()) return 'install';
    if (this.store.canRepair()) return 'repair';
    if (this.store.canOpenLoginItems()) return 'openLoginItems';
    return null;
  });

  protected failureTitle(action: 'repair' | 'install' | 'openLoginItems'): string {
    return action === 'install' ? this.t.installFailed : action === 'repair' ? this.t.repairFailed : this.t.loginItemsFailed;
  }
}
