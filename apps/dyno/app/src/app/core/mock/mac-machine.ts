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
/** The Mac's unified memory in bytes: 48 GB (`formatBytes` prints it "48 GB"). */
export const MAC_MEMORY_TOTAL = 48 * 1024 ** 3;
/** The share used, whole percent (77). */
export const MAC_DISK_USED_PERCENT = Math.round((1 - MAC_DISK_FREE / MAC_DISK_TOTAL) * 100);
/** `225 GB free of 995 GB`, as the alarm and issue texts say it. */
export const MAC_DISK_FREE_TEXT = `${formatStorage(MAC_DISK_FREE)} free of ${formatStorage(MAC_DISK_TOTAL)}`;

const MIB = 1024 ** 2;
/** What the system itself uses besides the processes (kernel time the table does not list), in percent of the CPU. */
export const MAC_CPU_OVERHEAD = 1.4;
/** The whole CPU's load at the start of a session, whole percent: the dashboard, Processes, the tray, Why slow and Throttling all say "75%". */
export const MAC_CPU_TOTAL = 75;
/** The Rust build's load (all its rustc processes and cargo), percent: "40%" on Processes, Why slow, the tray and the alarm. */
export const MAC_BUILD_CPU = 40;
/** What cargo itself uses of {@link MAC_BUILD_CPU}; the rustc processes use the rest. */
export const MAC_CARGO_CPU = 0.4;
/** What the build reads and writes together, bytes a second ("223 MB/s" on Processes); the rustc processes write 65% of it. */
export const MAC_BUILD_DISK_BPS = 223 * MIB;
/** What every process reads and writes together, bytes a second ("239 MB/s" on the dashboard). */
export const MAC_DISK_IO_BPS = 239 * MIB;
/** Memory in use at the start of a session: "24.8 GB" of {@link MAC_MEMORY_TOTAL}. */
export const MAC_MEMORY_USED = 24.8 * 1024 ** 3;
/** The share of memory in use, whole percent (52). */
export const MAC_MEMORY_PERCENT = Math.round((MAC_MEMORY_USED / MAC_MEMORY_TOTAL) * 100);
/** Writes are this share of a process's disk traffic in the mock (the rest is reads). */
export const MAC_WRITE_SHARE = 0.65;
/** What the whole machine writes, bytes a second: the Why slow, Unusual and recorder pages quote writes, the dashboard reads plus writes. */
export const MAC_DISK_WRITE_BPS = MAC_DISK_IO_BPS * MAC_WRITE_SHARE;
/** The CPU's top clock in MHz: "4.50 GHz". */
export const MAC_CPU_MAX_MHZ = 4500;
/** The CPU's clock at the start of a session in MHz: "4.40 GHz" on the dashboard, CPU and Throttling. */
export const MAC_CPU_CLOCK_MHZ = 4400;
/** The CPU package's temperature at the start of a session, degrees Celsius: "74 °C" on Throttling and Is this normal. */
export const MAC_CPU_TEMP_C = 74;
