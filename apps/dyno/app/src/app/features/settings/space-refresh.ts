import type { PdSelectOption } from '@decryp7/paddock-ui/select';

/** The intervals offered for "Free space refresh" (seconds; 0 is Auto, a minute). */
const OFFERED: readonly PdSelectOption[] = [
  { value: '0', label: 'Auto' },
  { value: '10', label: '10 seconds' },
  { value: '30', label: '30 seconds' },
  { value: '120', label: '2 minutes' },
  { value: '300', label: '5 minutes' },
  { value: '600', label: '10 minutes' },
];

/**
 * The options for the current value: the offered ones, plus the current value itself when it is not one of them (45 set in the
 * settings file), so the select never goes blank for a value the person or a file set. Shown as "45 s".
 */
export function spaceRefreshChoices(currentSecs: number | undefined): PdSelectOption[] {
  const current = String(currentSecs ?? 0);
  return OFFERED.some((o) => o.value === current) ? [...OFFERED] : [...OFFERED, { value: current, label: `${current} s` }];
}
