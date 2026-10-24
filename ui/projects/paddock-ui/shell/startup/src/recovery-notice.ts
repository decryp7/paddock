import { ChangeDetectionStrategy, Component, computed, inject } from '@angular/core';
import { PdGlyph } from '@decryp7/paddock-ui/icons';
import { RecoveryStore } from './recovery.store';

/**
 * The notice after an unclean shutdown (plan X3): the app did not close properly last time, with what can be done about
 * it, as Restore (the last session), Safe mode (extensions, plugins and the last session off) and Start fresh. Which are
 * offered is Rust's decision (a crash in safe mode does not offer safe mode again). Shown under the title bar until the
 * user chooses; a failed choice stays visible with its reason. Needs a `RecoveryStore` (the pages frame provides it).
 */
@Component({
  selector: 'pd-recovery-notice',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [PdGlyph],
  template: `
    @if (store.unclean(); as u) {
      <section class="pd-recovery" role="alert" data-testid="recovery-notice" aria-labelledby="pd-recovery-title">
        <pd-glyph class="pd-recovery__icon" name="alert" [size]="16" />
        <div class="pd-recovery__text">
          <strong id="pd-recovery-title">{{ t.title }}</strong>
          @if (store.restarting()) {
            <span data-testid="recovery-restarting">{{ t.restarting }}</span>
          } @else {
            <span>{{ detail() }}</span>
          }
          @if (u.session.problem; as problem) {
            <span class="pd-recovery__problem" data-testid="recovery-session-problem">{{ problem }}</span>
          }
          @if (store.requestError(); as e) {
            <span class="pd-recovery__problem" role="status" data-testid="recovery-error">{{ e }}</span>
          }
        </div>
        <div class="pd-recovery__actions">
          @if (u.offers.restore) {
            <button type="button" class="pd-recovery__primary" data-testid="recovery-restore" [disabled]="busy()" (click)="store.restore()">{{ t.restore }}</button>
          }
          @if (u.offers.safeMode) {
            <button type="button" [class.pd-recovery__primary]="!u.offers.restore" data-testid="recovery-safe-mode" [disabled]="busy()" (click)="store.safeModeRestart()">{{ t.safeMode }}</button>
          }
          @if (u.offers.startFresh) {
            <button type="button" [class.pd-recovery__primary]="!u.offers.restore && !u.offers.safeMode" data-testid="recovery-fresh" [disabled]="busy()" (click)="store.startFresh()">{{ t.fresh }}</button>
          }
        </div>
      </section>
    }
  `,
  styles: `
    :host { display: block; min-width: 0; }
    .pd-recovery { display: flex; align-items: center; gap: var(--pd-space-3); flex-wrap: wrap; box-sizing: border-box; padding: var(--pd-space-2) var(--pd-space-3); border-bottom: 1px solid var(--pd-hairline); border-inline-start: 3px solid var(--pd-warning); background: var(--pd-surface-2); color: var(--pd-text); font-size: var(--pd-text-body); }
    .pd-recovery__icon { flex: none; color: var(--pd-warning); }
    .pd-recovery__text { display: flex; flex-direction: column; gap: 2px; flex: 1 1 24ch; min-width: 0; overflow-wrap: anywhere; }
    .pd-recovery__problem { color: var(--pd-text-muted); font-size: var(--pd-text-caption); }
    .pd-recovery__actions { display: flex; flex-wrap: wrap; gap: var(--pd-space-2); }
    button { min-height: 28px; padding: 0 var(--pd-space-3); border: 1px solid var(--pd-border-control); border-radius: var(--pd-radius-control); background: transparent; color: var(--pd-text); font: inherit; cursor: pointer; }
    button.pd-recovery__primary { border-color: transparent; background: var(--pd-accent-fill); color: var(--pd-on-accent); font-weight: var(--pd-weight-medium); }
    button:hover:not(:disabled) { background: var(--pd-hover); }
    button.pd-recovery__primary:hover:not(:disabled) { background: var(--pd-accent-fill); filter: brightness(1.08); }
    button:disabled { opacity: 0.55; cursor: default; }
    button:focus-visible { outline: 2px solid var(--pd-focus-ring); outline-offset: 2px; }
  `,
})
export class PdRecoveryNotice {
  protected readonly store = inject(RecoveryStore);
  protected readonly busy = computed(() => this.store.acting() !== null);
  protected readonly t = {
    title: $localize`:@@pd.startup.recovery-title:The app did not close properly last time.`,
    restarting: $localize`:@@pd.startup.recovery-restarting:Restarting in safe mode…`,
    restore: $localize`:@@pd.startup.recovery-restore:Restore last session`,
    safeMode: $localize`:@@pd.startup.recovery-safe-mode:Start in safe mode`,
    fresh: $localize`:@@pd.startup.recovery-fresh:Start fresh`,
    restorable: $localize`:@@pd.startup.recovery-detail-restore:Your last session can be restored, or you can start in safe mode (extensions and plugins off) or fresh.`,
    plain: $localize`:@@pd.startup.recovery-detail-plain:There is no session to restore. You can start in safe mode (extensions and plugins off) or fresh.`,
    inSafeMode: $localize`:@@pd.startup.recovery-detail-in-safe-mode:It stopped while running in safe mode, so extensions and plugins are probably not the cause. Restore the session or start fresh.`,
    noSessionInSafeMode: $localize`:@@pd.startup.recovery-detail-no-session-in-safe-mode:It stopped while running in safe mode, so extensions and plugins are probably not the cause. There is no session to restore; start fresh.`,
  };

  /** One sentence for what happened and what is on offer. */
  protected readonly detail = computed(() => {
    const u = this.store.unclean();
    if (!u) return '';
    if (u.crash?.inSafeMode) return u.offers.restore ? this.t.inSafeMode : this.t.noSessionInSafeMode;
    return u.offers.restore ? this.t.restorable : this.t.plain;
  });
}
