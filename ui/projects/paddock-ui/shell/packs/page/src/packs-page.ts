import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { MatButton } from '@angular/material/button';
import { MatProgressBar } from '@angular/material/progress-bar';
import { PdPathText } from '@decryp7/paddock-ui';
import { PdGlyph, PdStatusGlyph, type PdStatus } from '@decryp7/paddock-ui/icons';
import { PacksStore, type KindView } from '@decryp7/paddock-ui/shell/packs';
import { PACKS_STYLES } from './styles';

/**
 * Settings > Knowledge packs (plan X15/X23, rule 21): per kind of pack, the packs in use (built-in and the user's), which pack
 * was refused and why (what to fix), the keys that were ignored, and Reload. Changes the engine finds by itself arrive live.
 * Reload is the one primary button when a pack is refused. The page never depends on a pack (rule 19): a failed ask is an
 * inline error with Retry. File names are shortened in the middle with the full name as the tooltip.
 */
@Component({
  selector: 'pd-packs-page',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [MatButton, MatProgressBar, PdStatusGlyph, PdGlyph, PdPathText],
  template: `
    <section class="pd-packs__head" aria-labelledby="pd-packs-title" data-testid="packs-page">
      <div>
        <h2 id="pd-packs-title">{{ t.title }}</h2>
        <p class="pd-packs__muted">{{ t.lede }}</p>
      </div>
      @if (store.view()) {
        <button [matButton]="store.problemCount() > 0 ? 'filled' : 'outlined'" type="button" data-testid="packs-reload" [disabled]="store.reloading()" (click)="store.reload()">
          <pd-glyph name="refresh" [size]="16" />{{ t.reload }}
        </button>
      }
    </section>

    @if (store.reloading()) {
      <div class="pd-packs__notice" role="status" data-testid="packs-reloading">
        <span class="pd-packs__title">{{ t.reloading }}</span>
        <mat-progress-bar mode="indeterminate" [attr.aria-label]="t.reloading" />
        <span class="pd-packs__muted" data-testid="packs-still-working">{{ t.stillWorking }}</span>
      </div>
    }
    @if (store.reloadError(); as why) {
      <div class="pd-packs__notice" role="alert" data-testid="packs-reload-error">
        <span class="pd-packs__title">{{ t.reloadFailed }}</span>
        <span>{{ why }}</span>
      </div>
    }

    @if (store.view()) {
      @for (k of store.kinds(); track k.id) {
        <section class="pd-packs__kind" [attr.aria-labelledby]="'pd-packs-' + k.id" [attr.data-testid]="'packs-kind-' + k.id">
          <div class="pd-packs__kind-title">
            <pd-status-glyph [status]="status(k)" [size]="16" [class]="tone(k)" />
            <h3 [id]="'pd-packs-' + k.id">{{ k.label }}</h3>
            <span class="pd-packs__muted" [attr.data-testid]="'packs-state-' + k.id">{{ stateWord(k) }}</span>
          </div>
          <ul [attr.aria-label]="t.inUse">
            @for (p of k.packs; track p.name) {
              <li class="pd-packs__row">
                <pd-path-text [value]="p.name" [plain]="true" />
                <span class="pd-packs__origin">{{ p.origin === 'builtin' ? t.builtin : t.user }}</span>
              </li>
            }
          </ul>
          @if (k.problems.length > 0) {
            <ul [attr.aria-label]="t.refused" [attr.data-testid]="'packs-problems-' + k.id">
              @for (p of k.problems; track $index) {
                <li>
                <div class="pd-packs__problem" role="alert">
                  <pd-status-glyph status="error" [size]="16" class="pd-packs__bad" />
                  <div>
                    <span class="pd-packs__file">{{ p.file }}</span>
                    <span>{{ p.message }}</span>
                  </div>
                </div>
                </li>
              }
            </ul>
          }
          @if (k.warnings.length > 0) {
            <ul [attr.aria-label]="t.ignored" [attr.data-testid]="'packs-warnings-' + k.id">
              @for (w of k.warnings; track $index) {
                <li class="pd-packs__note">
                  <pd-status-glyph status="info" [size]="16" />
                  <span>{{ w.file }}: {{ w.message }}</span>
                </li>
              }
            </ul>
          }
        </section>
      } @empty {
        <p class="pd-packs__muted" data-testid="packs-none">{{ t.none }}</p>
      }
    } @else if (store.requestError(); as problem) {
      <div class="pd-packs__notice" role="alert" data-testid="packs-error">
        <span class="pd-packs__title">{{ t.noStatus }}</span>
        <span>{{ problem }}</span>
        <span><button matButton="outlined" type="button" data-testid="packs-load-retry" (click)="store.load()">{{ t.retry }}</button></span>
      </div>
    } @else {
      <p class="pd-packs__muted" role="status" data-testid="packs-loading">{{ t.loading }}</p>
    }
  `,
  styles: PACKS_STYLES,
})
export class PdPacksPage {
  protected readonly store = inject(PacksStore);

  protected readonly t = {
    title: $localize`:@@pd.packs.title:Knowledge packs`,
    lede: $localize`:@@pd.packs.lede:Lists the app knows are small files you can add to or change. A change is picked up by itself; a file the app cannot use is left out and named here.`,
    loading: $localize`:@@pd.packs.loading:Listing the packs…`,
    noStatus: $localize`:@@pd.packs.no-status-title:The packs could not be listed.`,
    retry: $localize`:@@pd.packs.retry:Retry`,
    reload: $localize`:@@pd.packs.reload:Reload`,
    reloading: $localize`:@@pd.packs.reloading:Reading the packs…`,
    stillWorking: $localize`:@@pd.packs.still-working:Still working. Nothing here gives up on it.`,
    reloadFailed: $localize`:@@pd.packs.reload-failed-title:The packs could not be reloaded.`,
    inUse: $localize`:@@pd.packs.in-use:Packs in use`,
    refused: $localize`:@@pd.packs.refused:Packs left out`,
    ignored: $localize`:@@pd.packs.ignored:Keys that were ignored`,
    builtin: $localize`:@@pd.packs.builtin:Built in`,
    user: $localize`:@@pd.packs.user:Yours`,
    none: $localize`:@@pd.packs.none:This app has no packs.`,
  };

  /** The word beside a kind's name; the status is never colour alone. */
  protected stateWord(k: KindView): string {
    switch (k.health.level) {
      case 'ok':
        return $localize`:@@pd.packs.state.ok:All loaded`;
      case 'notes':
        return $localize`:@@pd.packs.state.notes:Loaded, some keys ignored`;
      case 'degraded':
        return k.problems.length === 1 ? $localize`:@@pd.packs.state.one-left-out:One pack left out` : $localize`:@@pd.packs.state.left-out:${k.problems.length}:count: packs left out`;
    }
  }

  /** The status of a kind, drawn from the suite's one map: ok, some keys ignored, packs left out. */
  protected status(k: KindView): PdStatus {
    return k.health.level === 'ok' ? 'success' : k.health.level === 'notes' ? 'info' : 'warning';
  }

  protected tone(k: KindView): string {
    return k.health.level === 'ok' ? 'pd-packs__ok' : k.health.level === 'notes' ? 'pd-packs__muted' : 'pd-packs__bad';
  }
}
