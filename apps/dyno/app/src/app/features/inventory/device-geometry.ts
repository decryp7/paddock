/** What a drive looks like. */
export type DriveKind = 'nvme' | 'sata' | 'soldered' | 'removable';

/**
 * Picks the drawing of a drive from what the OS calls it: a removable one is a stick, Apple's internal SSD is NAND soldered to
 * the logic board, anything named like an NVMe/M.2 part is an M.2 2280 stick, and the rest is drawn as a 2.5" SATA drive.
 */
export function driveKind(name: string, removable: boolean): DriveKind {
  if (removable) return 'removable';
  if (/\bapple\b/i.test(name)) return 'soldered';
  if (/nvme|m\.2|pcie|\bpm9\w*|\bsn\d{3}\b|\b(?:9[5-9]\d|980|990)\s*pro\b|\bpc\s+sn\d+/i.test(name)) return 'nvme';
  return 'sata';
}

/** NAND packages on an M.2 stick: one up to 256 GB, two up to 1 TB, three above. The data has no package count. */
export function nandCount(totalBytes: number): number {
  const gb = totalBytes / 1000 ** 3;
  if (gb <= 280) return 1;
  return gb <= 1100 ? 2 : 3;
}

/** The x positions of `n` NAND packages of width `w` laid out left to right from `from`. */
export function nandSlots(n: number, from: number, w: number, gap: number): number[] {
  return Array.from({ length: n }, (_, i) => from + i * (w + gap));
}
