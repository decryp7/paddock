import { ChangeDetectionStrategy, Component, computed, inject, input, output } from '@angular/core';
import { MatButton } from '@angular/material/button';
import { DialogService } from '@decryp7/paddock-ui/shell/dialogs';
import { SETTINGS_DIALOG_ID } from '@decryp7/paddock-ui/shell/settings';
import { SETTINGS_PAGE_ID } from '../shell/navigate';
import type { NextStep, ReadVia } from './api.types';

/** What the source line shows for a reading: nothing for a plain local reading, else words (never colour alone). */
export type SourceView =
  | { kind: 'none' }
  | { kind: 'helper'; text: string }
  | { kind: 'hint'; text: string }
  | { kind: 'needsHelper'; reason: string; step: 'turnOn' | 'update' | null }
  | { kind: 'unavailable'; reason: string; step: 'retry' | 'settings' | null };

/** The chip's words for a reading the helper read. */
export const HELPER_CHIP = 'Read by the administrator helper';
/** The button's words (verb plus object) for a reading that needs the helper. */
export const TURN_ON_LABEL = 'Turn on administrator readings';

/** The button's words for a reading whose helper is older than this Dyno accepts. */
export const UPDATE_LABEL = 'Update the helper';

/** The button's words for a reading the helper must be repaired for (Repair is in Settings > Helper). */
export const OPEN_SETTINGS_LABEL = 'Open helper settings';
/** The button's words for a reading that may work if asked again. */
export const RETRY_LABEL = 'Try again';

/** What the button does for `next`: turn the helper on, update it (both in Settings > Helper), or nothing. */
function stepOf(next: NextStep): 'turnOn' | 'update' | null {
  if (next === 'updateHelper') return 'update';
  return typeof next === 'object' && 'openSettings' in next && next.openSettings === 'helper' ? 'turnOn' : null;
}

/**
 * What to show for `via`. `hint` is the one-line note for a local reading where the helper would add more (the caller says it);
 * without one a local reading shows nothing. The engine decides the reasons; this only places its words.
 */
export function sourceView(via: ReadVia | null | undefined, hint = ''): SourceView {
  if (via === null || via === undefined || via === 'local') return hint ? { kind: 'hint', text: hint } : { kind: 'none' };
  if (via === 'helper') return { kind: 'helper', text: HELPER_CHIP };
  if ('needsHelper' in via) return { kind: 'needsHelper', reason: via.needsHelper.reason, step: stepOf(via.needsHelper.next) };
  const next = via.unavailable.next;
  const step = next === 'retry' ? 'retry' : next !== null && typeof next === 'object' && 'openSettings' in next && next.openSettings === 'helper' ? 'settings' : null;
  return { kind: 'unavailable', reason: via.unavailable.reason, step };
}

/**
 * Where a reading came from (family ADR 0052 outcome contract), shown beside the reading on every page: a neutral chip when the
 * administrator helper read it, the reason and "Turn on administrator readings" when it needs the helper, the reason alone when
 * neither path can read it here. A reading's value is live data, not a job, so this is not an outcome.
 */
@Component({
  selector: 'dy-reading-source',
  imports: [MatButton],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @switch (view().kind) {
      @case ('helper') { <span class="chip" data-testid="reading-source-helper">{{ text() }}</span> }
      @case ('hint') { <p class="note" data-testid="reading-source-hint">{{ text() }}</p> }
      @case ('needsHelper') {
        <p class="note" data-testid="reading-source-needs">{{ text() }}</p>
        @if (step(); as s) { <button type="button" matButton="outlined" (click)="openSettings()" data-testid="reading-source-turn-on">{{ s === 'update' ? updateLabel : label }}</button> }
      }
      @case ('unavailable') {
        <p class="note" data-testid="reading-source-unavailable">{{ text() }}</p>
        @if (unavailableStep() === 'retry' && canRetry()) { <button type="button" matButton="outlined" (click)="retry.emit()" data-testid="reading-source-step">{{ retryLabel }}</button> }
        @if (unavailableStep() === 'settings') { <button type="button" matButton="outlined" (click)="openSettings()" data-testid="reading-source-step">{{ settingsLabel }}</button> }
      }
    }
  `,
  styles: `
    :host { display: contents; }
    .chip { display: inline-block; padding: 2px 10px; border-radius: 999px; border: 1px solid var(--border-strong); font-size: var(--fs-xs); color: var(--text-2); }
    .note { margin: 0; display: flex; gap: var(--sp-2); align-items: center; flex-wrap: wrap; color: var(--text-2); font-size: var(--fs-sm); }
  `,
})
export class ReadingSource {
  /** The reading's source, as the engine sent it. */
  readonly via = input<ReadVia | null | undefined>();
  /** Whether the page can read again when asked (it handles `retry`); only then is "Try again" offered. */
  readonly canRetry = input(false);
  /** The one-line note for a local reading where the helper would add more. */
  readonly hint = input('');

  private readonly dialogs = inject(DialogService);
  protected readonly label = TURN_ON_LABEL;
  protected readonly updateLabel = UPDATE_LABEL;
  protected readonly retryLabel = RETRY_LABEL;
  protected readonly settingsLabel = OPEN_SETTINGS_LABEL;
  /** Asked for when the reading may work if read again; the page reads it again. */
  readonly retry = output<void>();
  protected readonly view = computed(() => sourceView(this.via(), this.hint()));
  protected readonly text = computed(() => {
    const v = this.view();
    return v.kind === 'helper' || v.kind === 'hint' ? v.text : v.kind === 'needsHelper' || v.kind === 'unavailable' ? v.reason : '';
  });
  protected readonly unavailableStep = computed(() => {
    const v = this.view();
    return v.kind === 'unavailable' ? v.step : null;
  });
  protected readonly step = computed(() => {
    const v = this.view();
    return v.kind === 'needsHelper' ? v.step : null;
  });

  /** Opens Settings at the Administrator readings card, where the helper is turned on or updated. */
  protected openSettings(): void {
    void this.dialogs.open(SETTINGS_DIALOG_ID, { inputs: { page: SETTINGS_PAGE_ID, anchor: { id: 'helper', at: Date.now() } } });
  }
}
