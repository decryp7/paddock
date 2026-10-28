import type { Measured, UnavailableReason } from './api.types';

/** Every reason, in the order of the wire contract (`contract/measured-wire.json`). */
export const ALL_REASONS: readonly UnavailableReason[] = ['needsAdmin', 'notPermitted', 'notSupported', 'residentMemoryOnly'];

/** The words shown for a value that was not read. */
export const UNAVAILABLE = 'Unavailable';

/** The value, when it was read; `null` when not. Never `0` for a value that was not read. */
export function readValue<T>(m: Measured<T>): T | null {
  return 'value' in m ? m.value : null;
}

/** The reason, when the value was not read. */
export function readReason<T>(m: Measured<T>): UnavailableReason | null {
  return 'unavailable' in m ? m.unavailable.reason : null;
}

/** A readable value. */
export function measured<T>(value: T): Measured<T> {
  return { value };
}

/** A value that was not read, with the reason. */
export function unavailable<T>(reason: UnavailableReason): Measured<T> {
  return { unavailable: { reason } };
}

/** One sentence saying why a value is unavailable (tooltip and screen-reader text). */
export function reasonText(reason: UnavailableReason): string {
  switch (reason) {
    case 'needsAdmin':
      return 'Needs administrator rights to read';
    case 'notPermitted':
      return 'The system does not let Dyno read this process';
    case 'notSupported':
      return 'This system does not give Dyno this value';
    case 'residentMemoryOnly':
      return 'Linux gives Dyno only resident memory for these';
  }
}

/** The tooltip and aria text for a value: its reason when it was not read, else null. */
export function unavailableText<T>(m: Measured<T>): string | null {
  const r = readReason(m);
  return r === null ? null : `${UNAVAILABLE}: ${reasonText(r)}`;
}

/** A value formatted with `format`, or "Unavailable" when it was not read. */
export function showMeasured<T>(m: Measured<T>, format: (v: T) => string): string {
  const v = readValue(m);
  return v === null ? UNAVAILABLE : format(v);
}

/** The value for a bar width or a sort: `0` when it was not read (the text beside it says Unavailable). */
export function barValue(m: Measured<number>): number {
  return readValue(m) ?? 0;
}
