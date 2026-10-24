import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { SuStatusPill } from '@decryp7/paddock-ui';
import { SetupStore, statusView } from '@decryp7/paddock-ui/shell/setup';
import { SETUP_STYLES } from './styles';

/**
 * The checklist itself, shared by the Set-up window and the Settings page: every step with its status word, what it does and
 * why, the one primary button (the step's action, or Repair when something is wrong), Skip for now / Take it up again, a
 * step's working state with "Still working", and an inline error with Retry. It needs the window's `SetupStore`
 * (`provideShellSetup`). It never depends on what it sets up (rule 19): when a step is broken Repair is there, and when the
 * steps cannot be asked at all the page says so with Retry.
 */
@Component({
  selector: 'pd-setup-checklist',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [SuStatusPill],
  template: `
    @if (store.setup(); as s) {
      <p class="pd-setup__muted" data-testid="setup-lede">{{ store.allSettled() ? t.settled : t.lede }}</p>
      @if (store.failure(); as f) {
        <div class="pd-setup__notice pd-setup__notice--danger" role="alert" data-testid="setup-failure">
          <span class="pd-setup__title">{{ t.failed }}</span>
          <span>{{ f.error.message }}</span>
          @if (f.error.retryable) {
            <span class="pd-setup__row"><button type="button" class="pd-setup__btn" data-testid="setup-retry" (click)="store.retry()">{{ t.retry }}</button></span>
          }
        </div>
      }
      <ul class="pd-setup__list" data-testid="setup-list" [attr.aria-label]="t.steps">
        @for (item of s.items; track item.id) {
          @let working = store.isWorking(item.id);
          @let v = view(item);
          <li class="pd-setup__item" [attr.data-testid]="'setup-item-' + item.id" [attr.data-status]="item.status">
            <div class="pd-setup__body">
              <div class="pd-setup__head">
                <h3>{{ item.title }}</h3>
                <pd-status-pill [status]="v.status" [label]="v.label" [attr.data-testid]="'setup-status-' + item.id" />
              </div>
              <p class="pd-setup__muted">{{ item.detail }}</p>
              @if (item.message && item.status !== 'done') {
                <p [attr.data-testid]="'setup-message-' + item.id">{{ item.message }}</p>
              } @else if (item.needsAi && item.status !== 'done') {
                <p [attr.data-testid]="'setup-message-' + item.id">{{ item.needsAi.message }}</p>
              }
              @if (item.hint) {
                <p class="pd-setup__title" [attr.data-testid]="'setup-hint-' + item.id">{{ item.hint }}</p>
              }
              @if (working) {
                <progress [attr.aria-label]="t.working" [attr.data-testid]="'setup-working-' + item.id"></progress>
                <span class="pd-setup__muted" data-testid="setup-still-working">{{ t.stillWorking }}</span>
              }
            </div>
            <div class="pd-setup__actions" role="group" [attr.aria-label]="item.title">
              @if (item.status === 'problem') {
                <button type="button" class="pd-setup__btn pd-setup__btn--primary" [attr.data-testid]="'setup-repair-' + item.id" [disabled]="working" (click)="store.repair(item.id)">{{ t.repair }}</button>
              } @else if (item.status === 'todo') {
                <button type="button" class="pd-setup__btn" [class.pd-setup__btn--primary]="!item.skipped" [attr.data-testid]="'setup-act-' + item.id" [disabled]="working" (click)="store.act(item.id)">{{ item.action }}</button>
              }
              @if (item.status !== 'done') {
                <button type="button" class="pd-setup__btn pd-setup__btn--quiet" [attr.data-testid]="'setup-skip-' + item.id" [disabled]="working" (click)="store.skip(item.id, !item.skipped)">{{ item.skipped ? t.unskip : t.skip }}</button>
              }
            </div>
          </li>
        } @empty {
          <li class="pd-setup__muted" data-testid="setup-empty">{{ t.empty }}</li>
        }
      </ul>
    } @else if (store.requestError(); as problem) {
      <div class="pd-setup__notice pd-setup__notice--danger" role="alert" data-testid="setup-error">
        <span class="pd-setup__title">{{ t.noState }}</span>
        <span>{{ problem }}</span>
        <span class="pd-setup__row"><button type="button" class="pd-setup__btn" data-testid="setup-load-retry" (click)="store.load()">{{ t.retry }}</button></span>
      </div>
    } @else {
      <p class="pd-setup__muted" role="status" data-testid="setup-loading">{{ t.loading }}</p>
    }
  `,
  styles: SETUP_STYLES,
})
export class PdSetupChecklist {
  protected readonly store = inject(SetupStore);

  protected readonly t = {
    lede: $localize`:@@pd.setup.lede:A few things the app can set up for you. Each is optional, you can skip any of them, and you can come back to this list later in Settings.`,
    settled: $localize`:@@pd.setup.settled:Everything is set up or skipped. You can change any of it later in Settings.`,
    steps: $localize`:@@pd.setup.steps:Set-up steps`,
    loading: $localize`:@@pd.setup.loading:Looking at what is set up…`,
    noState: $localize`:@@pd.setup.no-state-title:The set-up steps could not be asked.`,
    retry: $localize`:@@pd.setup.retry:Retry`,
    repair: $localize`:@@pd.setup.repair:Repair`,
    skip: $localize`:@@pd.setup.skip:Skip for now`,
    unskip: $localize`:@@pd.setup.unskip:Take it up again`,
    working: $localize`:@@pd.setup.working:Working…`,
    stillWorking: $localize`:@@pd.setup.still-working:Still working. This can take a minute; nothing here gives up on it.`,
    failed: $localize`:@@pd.setup.failed:That did not work.`,
    empty: $localize`:@@pd.setup.empty:There is nothing to set up on this computer.`,
  };

  protected readonly view = statusView;
}
