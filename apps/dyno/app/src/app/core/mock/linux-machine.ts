import { formatStorage } from '../../shared/format';

/**
 * The one declared fact sheet of the mock's Linux laptop (the Mac and the Windows PC have theirs): the root filesystem and the memory its live
 * readings, its inventory, its Health reason, its Why slow card and its weekly report name. Storage is decimal, as the app's `formatStorage`
 * prints it, and every text is built from the numbers with that formatter, never typed.
 */
/** The root filesystem as titles name it. */
export const LINUX_DISK_NAME = 'Root filesystem';
/** Capacity of the root filesystem in bytes: "953 GB". */
export const LINUX_DISK_TOTAL = 953_000_000_000;
/** Free bytes of the root filesystem at the start of a session: "210 GB". */
export const LINUX_DISK_FREE = 210_000_000_000;
/** What the build output takes from it each day, in bytes. */
export const LINUX_DAILY_FILL = 5.1e9;
/** The share used, whole percent (78). */
export const LINUX_DISK_USED_PERCENT = Math.round((1 - LINUX_DISK_FREE / LINUX_DISK_TOTAL) * 100);
/** `210 GB free of 953 GB`. */
export const LINUX_DISK_FREE_TEXT = `${formatStorage(LINUX_DISK_FREE)} free of ${formatStorage(LINUX_DISK_TOTAL)}`;
/** The data disk mounted at /mnt/data in bytes: "2 TB" with 1.29 TB free. */
export const LINUX_DATA_TOTAL = Math.round(1863 * 1024 ** 3);
/** Free bytes of the data disk. */
export const LINUX_DATA_FREE = Math.round(1204 * 1024 ** 3);
/** The Linux laptop's memory in bytes: two 16 GB SO-DIMMs, 32 GB. */
export const LINUX_MEMORY_TOTAL = 32 * 1024 ** 3;
