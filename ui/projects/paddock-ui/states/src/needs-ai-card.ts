import { ChangeDetectionStrategy, Component, input } from '@angular/core';
import { PdStatusGlyph } from '@decryp7/paddock-ui/icons';

/**
 * The status of the card, drawn from the suite's one status map: `info` for a state the user can move on from, `warning` for one
 * that blocks (incompatible, unavailable).
 */
export type NeedsAiIcon = 'info' | 'warning';

let nextId = 0;

/**
 * The look of the family's "this feature needs local AI" state (ADR 0004), without any source of truth: an icon by kind, a title
 * ("Ask about this log needs local AI"), the message, an optional pairing code with its hint, the actions (project them as the
 * content: the first button is the primary one), an optional hint under them and an optional error. `<pd-needs-ai>` fills it from the
 * window's `PitwallStore`; `<pd-view>` fills it with the plugin's reason when the host has no Pitwall state. Never put the repair of
 * a missing Pitwall on a plugin's action when the app can run it itself (rule 19).
 *
 * ```html
 * <pd-needs-ai-card title="Ask about this log needs local AI" message="Pitwall is not running." icon="info">
 *   <button type="button" class="pd-btn pd-btn--primary">Open Pitwall</button>
 * </pd-needs-ai-card>
 * ```
 */
@Component({
  selector: 'pd-needs-ai-card',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [PdStatusGlyph],
  template: `
    <section class="pd-needs-ai" [attr.aria-labelledby]="title() ? titleId : null" [attr.aria-label]="title() ? null : (name() || null)" [attr.data-testid]="testid()" [attr.data-kind]="kind() || null">
      <pd-status-glyph class="pd-needs-ai__icon" [class.pd-needs-ai__icon--blocked]="icon() === 'warning'" [status]="icon()" [size]="20" />
      <div class="pd-needs-ai__body">
        @if (title()) {
          <p class="pd-needs-ai__title" [id]="titleId">{{ title() }}</p>
        }
        <p class="pd-needs-ai__message" data-testid="needs-ai-message">{{ message() }}</p>
        @if (code(); as c) {
          <p class="pd-needs-ai__code" data-testid="needs-ai-code" [attr.aria-label]="codeLabel()">{{ c }}</p>
          <p class="pd-needs-ai__hint" data-testid="needs-ai-approve">{{ approve() }}</p>
        }
        <div class="pd-needs-ai__actions"><ng-content /></div>
        @if (hint()) {
          <p class="pd-needs-ai__hint" data-testid="needs-ai-on-builtin">{{ hint() }}</p>
        }
        @if (error(); as err) {
          <p class="pd-needs-ai__error" role="alert" data-testid="needs-ai-error">{{ err }}</p>
        }
      </div>
    </section>
  `,
  styles: `
    :host { display: block; container-type: inline-size; }
    .pd-needs-ai { display: flex; gap: var(--pd-space-3); align-items: flex-start; padding: var(--pd-space-4); border: 1px solid var(--pd-border-subtle, var(--pd-hairline)); border-radius: var(--pd-radius-card); background: var(--pd-surface-2); color: var(--pd-text); }
    .pd-needs-ai__icon { color: var(--pd-text-muted); margin-top: 2px; }
    .pd-needs-ai__icon--blocked { color: var(--pd-danger-text, var(--pd-danger)); }
    .pd-needs-ai__body { display: flex; flex-direction: column; gap: var(--pd-space-2); min-width: 0; flex: 1; }
    p { margin: 0; overflow-wrap: break-word; }
    .pd-needs-ai__title { font-weight: var(--pd-weight-semibold); }
    .pd-needs-ai__hint { color: var(--pd-text-muted); font-size: var(--pd-text-label); }
    .pd-needs-ai__code { font-size: var(--pd-text-title); font-weight: var(--pd-weight-semibold); letter-spacing: 0.15em; font-variant-numeric: tabular-nums; }
    .pd-needs-ai__actions { display: flex; flex-wrap: wrap; gap: var(--pd-space-2); }
    .pd-needs-ai__actions:empty { display: none; }
    .pd-needs-ai__error { color: var(--pd-danger-text, var(--pd-danger)); }
    @container (max-width: 360px) { .pd-needs-ai { flex-direction: column; padding: var(--pd-space-3); } }
  `,
})
export class PdNeedsAiCard {
  protected readonly titleId = `pd-needs-ai-card-${++nextId}`;
  /** The heading ("Ask about this log needs local AI"); empty for a card that has only a message (the accessible name is `name`). */
  readonly title = input('');
  /** The accessible name of a card without a title. */
  readonly name = input('');
  /** What is wrong and what to do, in the user's words. */
  readonly message = input.required<string>();
  readonly icon = input<NeedsAiIcon>('info');
  /** The pairing code to compare in Pitwall, and the sentence that says where. */
  readonly code = input('');
  readonly codeLabel = input('');
  readonly approve = input('');
  /** A quiet sentence under the actions. */
  readonly hint = input('');
  /** A failed action, as an alert under everything else. */
  readonly error = input('');
  /** The `data-testid` of the card. */
  readonly testid = input('needs-ai');
  /** The kind of state (`data-kind`), for the apps' styling and tests. */
  readonly kind = input('');
}
