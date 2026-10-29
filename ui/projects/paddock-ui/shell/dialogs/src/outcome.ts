import { answered, DISMISSED, type DialogOutcome } from '@decryp7/paddock-ui/shell';
import { isRecord } from './mount';

/*
 * A dialog's outcome on the wire (`dialog_close { result }` to Rust, `paddock://dialog-result` to the opener; ADR 0026 §2).
 * `null` is a dismissal (also what Rust sends when it closed a window whose page did not answer); an answer is
 * `{ "answered": true, "value": ... }`, with no `value` for an answer of `undefined`, so an answer is never mistaken for a
 * dismissal.
 */

/** The JSON a dialog closes with. */
export function encodeOutcome(outcome: DialogOutcome<unknown>): unknown {
  if (outcome.kind === 'dismissed') return null;
  return outcome.value === undefined ? { answered: true } : { answered: true, value: outcome.value };
}

/** The outcome of what a dialog closed with; anything that is not an answer is a dismissal. */
export function decodeOutcome(result: unknown): DialogOutcome<unknown> {
  return isRecord(result) && result['answered'] === true ? answered(result['value']) : DISMISSED;
}
