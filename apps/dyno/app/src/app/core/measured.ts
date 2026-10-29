import type { Measured, UnavailableReason, UnreadableReason } from './api.types';

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

/** The reason a folder was not read, in one sentence (the engine chose it from the OS error and the OS). */
export function unreadableReasonText(reason: UnreadableReason): string {
  switch (reason) {
    case 'needsFullDiskAccess':
      return 'Needs Full Disk Access: allow Dyno in System Settings › Privacy & Security › Full Disk Access';
    case 'needsAdmin':
      return reasonText('needsAdmin');
    case 'protected':
      return 'macOS protects this folder';
    case 'other':
      return 'The system could not read this folder';
  }
}

/** The same in a few words, for the tag beside the folder. */
export function unreadableShort(reason: UnreadableReason | null): string {
  switch (reason) {
    case 'needsFullDiskAccess':
      return 'Needs Full Disk Access';
    case 'needsAdmin':
      return 'Needs administrator rights';
    case 'protected':
      return 'Protected by macOS';
    default:
      return 'Could not be read';
  }
}

/** The tooltip and aria text of an unreadable folder: "Unavailable: <reason>". */
export function unreadableText(reason: UnreadableReason | null): string {
  return `${UNAVAILABLE}: ${unreadableReasonText(reason ?? 'other')}`;
}
