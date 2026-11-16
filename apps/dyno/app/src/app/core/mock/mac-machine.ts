import { formatStorage } from '../../shared/format';

/**
 * The one declared fact sheet of the mock's Mac, so every reply and every text that names it agrees: before this the startup disk read 182 of 994 GB
 * in the tray, 210 of 953 in Health and 225 of 1 TB on the disk map. Storage is decimal, as Finder and the app's `formatStorage` print it
 * (`994_662_584_320` is "995 GB"), and a text is built from the number with that same formatter, never typed.
 */
/** The startup disk's capacity in bytes: the 1 TB volume a Mac reports as 995 GB. */
export const MAC_DISK_TOTAL = 994_662_584_320;
/** Free bytes of the startup disk at the start of a session: "225 GB". */
export const MAC_DISK_FREE = 225_000_000_000;
/** The share used, whole percent (77). */
export const MAC_DISK_USED_PERCENT = Math.round((1 - MAC_DISK_FREE / MAC_DISK_TOTAL) * 100);
/** `225 GB free of 995 GB`, as the alarm and issue texts say it. */
export const MAC_DISK_FREE_TEXT = `${formatStorage(MAC_DISK_FREE)} free of ${formatStorage(MAC_DISK_TOTAL)}`;
