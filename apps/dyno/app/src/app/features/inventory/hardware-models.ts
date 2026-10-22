/**
 * The plain input models of the hardware drawings. They carry what to draw and nothing about where the data came from, so the
 * drawings depend on no store, service or API type: an adapter maps the application's data onto these.
 */

/** The memory technology, which decides the connector key, the DRAM package count and the extras (PMIC) on a module. */
export type MemoryTech = 'ddr2' | 'ddr3' | 'ddr4' | 'ddr5' | 'lpddr3' | 'lpddr4' | 'lpddr5' | 'unknown';

/** The connector of a module: SO-DIMM (laptops) or full-size DIMM (desktops). */
export type DimmForm = 'dimm' | 'soDimm';

/** A short tag on a slot ("Size mismatch"). The tone picks the colour; the words carry the meaning. */
export interface SlotTag {
  label: string;
  tone: 'neutral' | 'info' | 'ok' | 'warn';
}

/** A module in a socket. */
export interface DimmModuleModel {
  tech: MemoryTech;
  form: DimmForm;
  /** Formatted size, shown beside the drawing ("16 GB"). */
  sizeLabel: string;
  capacityBytes: number;
  /** Speeds in MT/s; null when unknown. A running speed below the rated one is shown as "3200 → 2666 MT/s". */
  ratedMts: number | null;
  runningMts: number | null;
  maker: string | null;
  part: string | null;
}

/** One socket: empty (`module` null) or holding a module. */
export interface DimmSlotModel {
  name: string;
  /** "A", "B"…; slots with the same value are grouped under a channel heading, null draws no heading. */
  channel: string | null;
  module: DimmModuleModel | null;
  tags: readonly SlotTag[];
  /** Free text for the details popover (an upgrade hint, say). */
  note: string | null;
}

/** A processor package with memory beside it. */
export interface SocPackageModel {
  /** "apple": die with memory on one substrate. "soldered": SoC and memory packages soldered to a mainboard. */
  style: 'apple' | 'soldered';
  /** The chip's name ("Apple M4 Pro"); it picks the tier of an Apple package. */
  name: string;
  physicalCores: number;
  memoryBytes: number;
  memoryTech: MemoryTech;
  /** Text alternative of the drawing. */
  alt: string;
  /** Heading beside the drawing. */
  title: string;
}
