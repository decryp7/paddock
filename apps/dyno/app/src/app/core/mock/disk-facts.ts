import { MAC_DISK_FREE, MAC_DISK_FREE_TEXT, MAC_DISK_TOTAL, MAC_DISK_USED_PERCENT } from './mac-machine';
import { LINUX_DAILY_FILL, LINUX_DATA_FREE, LINUX_DATA_TOTAL, LINUX_DISK_FREE, LINUX_DISK_FREE_TEXT, LINUX_DISK_NAME, LINUX_DISK_TOTAL, LINUX_DISK_USED_PERCENT } from './linux-machine';
import type { MockOs } from './os';
import { WIN_DAILY_FILL, WIN_DATA_FREE, WIN_DATA_TOTAL, WIN_DISK_FREE, WIN_DISK_FREE_TEXT, WIN_DISK_NAME, WIN_DISK_TOTAL, WIN_DISK_USED_PERCENT } from './windows-machine';

/** The startup disk of a mock machine as its fact sheet declares it. */
export interface StartupDisk {
  name: string;
  total: number;
  free: number;
  /** Bytes the disk fills a day with build output (the forecast and the trend). */
  fillPerDay: number;
  usedPercent: number;
  /** `225 GB free of 995 GB`. */
  freeText: string;
}

/** The startup disk of the machine the mock pretends to be: the one place the pages choose between the sheets. */
export function startupDisk(os: MockOs): StartupDisk {
  switch (os) {
    case 'macos':
      return { name: 'Macintosh HD', total: MAC_DISK_TOTAL, free: MAC_DISK_FREE, fillPerDay: 5.1e9, usedPercent: MAC_DISK_USED_PERCENT, freeText: MAC_DISK_FREE_TEXT };
    case 'linux':
      return { name: LINUX_DISK_NAME, total: LINUX_DISK_TOTAL, free: LINUX_DISK_FREE, fillPerDay: LINUX_DAILY_FILL, usedPercent: LINUX_DISK_USED_PERCENT, freeText: LINUX_DISK_FREE_TEXT };
    default:
      return { name: WIN_DISK_NAME, total: WIN_DISK_TOTAL, free: WIN_DISK_FREE, fillPerDay: WIN_DAILY_FILL, usedPercent: WIN_DISK_USED_PERCENT, freeText: WIN_DISK_FREE_TEXT };
  }
}

/** The second disk (D: or /mnt/data) of the machines that have one, or null. */
export function dataDisk(os: MockOs): { total: number; free: number } | null {
  if (os === 'windows') return { total: WIN_DATA_TOTAL, free: WIN_DATA_FREE };
  if (os === 'linux') return { total: LINUX_DATA_TOTAL, free: LINUX_DATA_FREE };
  return null;
}
