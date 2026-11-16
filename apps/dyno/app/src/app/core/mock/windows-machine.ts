import { formatStorage } from '../../shared/format';

/**
 * The one declared fact sheet of the mock's Windows PC (the Mac has its own in mac-machine.ts), so the Health reason, the alarm, the Why slow card,
 * the weekly report, the history trend, the live drive list and the machine profile say the same thing about drive C:. Storage is decimal, as the
 * app's `formatStorage` prints it, and every text is built from the numbers with that formatter, never typed.
 */
/** Drive C: as its own name reads in titles. */
export const WIN_DISK_NAME = 'Drive C:';
/** Capacity of drive C: in bytes: "953 GB". */
export const WIN_DISK_TOTAL = 953_000_000_000;
/** Free bytes of drive C: at the start of a session: "210 GB". */
export const WIN_DISK_FREE = 210_000_000_000;
/** What the build output takes from drive C: each day, in bytes ("about 5 GB a day"). */
export const WIN_DAILY_FILL = 5.1e9;
/** The share used, whole percent (78). */
export const WIN_DISK_USED_PERCENT = Math.round((1 - WIN_DISK_FREE / WIN_DISK_TOTAL) * 100);
/** `210 GB free of 953 GB`, as the Health reason, the alarm and the issue texts say it. */
export const WIN_DISK_FREE_TEXT = `${formatStorage(WIN_DISK_FREE)} free of ${formatStorage(WIN_DISK_TOTAL)}`;
/** The data drive D: in bytes: "2 TB" with 1.29 TB free (the media and archive drive). */
export const WIN_DATA_TOTAL = Math.round(1863 * 1024 ** 3);
/** Free bytes of drive D:. */
export const WIN_DATA_FREE = Math.round(1204 * 1024 ** 3);
/** The sizes in GiB of the sticks in the Windows PC: a 16 GB and an 8 GB one (the mismatched pair the inventory's hints explain). */
export const WIN_MEMORY_STICKS_GIB = [16, 8] as const;
/** The Windows PC's memory in bytes: the sticks added up, 24 GB. */
export const WIN_MEMORY_TOTAL = WIN_MEMORY_STICKS_GIB.reduce((a, g) => a + g, 0) * 1024 ** 3;
