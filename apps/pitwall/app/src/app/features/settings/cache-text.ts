import type { CacheKindInfo } from '../../core/api.types';
import { ago, bytes, int } from '../../shared/format';
import type { SelectOption } from '../../shared/ui/select';

const DAY = 86_400;

/** The "expires after" choices; 0 means never (only the size cap removes entries). */
export const TTL_CHOICES: readonly { secs: number; label: string }[] = [
  { secs: 3600, label: '1 hour' },
  { secs: DAY, label: '1 day' },
  { secs: 7 * DAY, label: '7 days' },
  { secs: 30 * DAY, label: '30 days' },
  { secs: 90 * DAY, label: '90 days' },
  { secs: 0, label: 'Never (size cap only)' },
];

/** The choices for a select, with the current value added when it is not one of the standard ones (e.g. from settings.toml). */
export function ttlOptions(current: number, sliding: boolean): SelectOption[] {
  const suffix = sliding ? ' since last use' : ' after it is stored';
  const label = (c: { secs: number; label: string }) => (c.secs === 0 ? c.label : c.label + suffix);
  const out = TTL_CHOICES.map((c) => ({ value: String(c.secs), label: label(c) }));
  if (!TTL_CHOICES.some((c) => c.secs === current)) out.push({ value: String(current), label: `${int(current)} seconds${suffix}` });
  return out;
}

/** What a cache's expiry rule says, for the read-only stats. */
export function expiryText(k: CacheKindInfo): string {
  if (k.expiresAfterSecs === null) return 'Never (size cap only)';
  const choice = TTL_CHOICES.find((c) => c.secs === k.expiresAfterSecs);
  const span = choice ? choice.label : `${int(k.expiresAfterSecs)} seconds`;
  return `${span} ${k.sliding ? 'since last use' : 'after it is stored'}`;
}

/** `12.4 MB of 512 MB`. */
export function sizeText(k: CacheKindInfo): string {
  return `${bytes(k.bytes)} of ${bytes(k.maxBytes)}`;
}

/** Age of the oldest entry as a relative date, or `none` when the cache is empty. */
export function oldestText(k: CacheKindInfo, now: number): string {
  return k.oldestAt === null ? 'none' : ago(k.oldestAt, now);
}
