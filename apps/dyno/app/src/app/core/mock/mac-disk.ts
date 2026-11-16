/**
 * The one startup disk of the mock's Mac (a 1 TB APFS volume), so every string and number that names it agrees: the tray's disk list, Health, the
 * disk map, the alarms and the machine profile showed 182 of 994, 210 of 953 and 225 of 1 TB for the same drive.
 */
const GB = 1024 ** 3;

/** Capacity in bytes: 994.7 GB (the volume a 1 TB Mac reports). */
export const MAC_DISK_TOTAL = Math.round(994.7 * GB);
/** Free bytes at the start of a session. */
export const MAC_DISK_FREE = 225 * GB;
/** The share used, whole percent (77). */
export const MAC_DISK_USED_PERCENT = Math.round((1 - MAC_DISK_FREE / MAC_DISK_TOTAL) * 100);
/** `225 GB free of 995 GB`, as the alarm and issue texts say it. */
export const MAC_DISK_FREE_TEXT = `${Math.round(MAC_DISK_FREE / GB)} GB free of ${Math.round(MAC_DISK_TOTAL / GB)} GB`;
