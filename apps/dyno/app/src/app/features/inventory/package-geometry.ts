import { techLabel } from './dimm-geometry';
import type { MemoryTech } from './hardware-models';

export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

/** Apple's tiers, from the plain chip up to two dies joined into one. */
export type Tier = 'base' | 'pro' | 'max' | 'ultra';

export interface AppleChip {
  generation: number;
  tier: Tier;
}

export type RegionKind = 'cpu-perf' | 'cpu-eff' | 'gpu' | 'npu' | 'fabric' | 'io';

/** A block of the die floorplan; `cells` are the cores (or compute units) drawn inside it. */
export interface Region extends Rect {
  kind: RegionKind;
  cells: Rect[];
}

export interface DieLayout extends Rect {
  regions: Region[];
}

export interface LegendEntry {
  kind: RegionKind | 'memory';
  label: string;
  detail: string;
}

/** Everything the package drawing needs: the board or substrate, the dies, the memory packages and the legend. */
export interface PackageLayout {
  /** An Apple-style package (memory beside the die on one substrate) or memory soldered to a mainboard next to the SoC. */
  kind: 'apple' | 'soldered';
  width: number;
  height: number;
  substrate: Rect;
  /** The SoC package lid (soldered boards) or the stiffener ring around the dies (Apple). */
  lid: Rect;
  dies: DieLayout[];
  memory: Rect[];
  /** Capacitors and other passives along the edges. */
  passives: Rect[];
  legend: LegendEntry[];
}

/** "Apple M4 Pro" is generation 4, tier pro. Null for anything that is not a recognisable Apple chip name. */
export function parseAppleChip(name: string): AppleChip | null {
  const m = /\bApple\s+M(\d{1,2})(?:\s+(Pro|Max|Ultra))?\b/i.exec(name);
  if (!m) return null;
  const tier = (m[2] ?? 'base').toLowerCase();
  return { generation: Number(m[1]), tier: tier === 'pro' || tier === 'max' || tier === 'ultra' ? tier : 'base' };
}

/** Is this an Apple chip by name, even one newer than the tables here? */
export const isAppleSilicon = (name: string): boolean => /\bApple\b/i.test(name);

/**
 * The performance/efficiency split of the CPU cores. The data only has the total, so the efficiency count comes from what each
 * generation and tier ships; null (draw one block) when the tables do not cover the chip or do not add up to the total.
 */
export function coreSplit(chip: AppleChip | null, physical: number): { performance: number; efficiency: number } | null {
  if (!chip || physical < 2) return null;
  const g = chip.generation;
  const e = (() => {
    switch (chip.tier) {
      case 'base':
        return g >= 4 ? 6 : 4;
      case 'pro':
        return g === 1 ? 2 : g === 3 ? 6 : 4;
      case 'max':
        return g === 1 ? 2 : 4;
      case 'ultra':
        return g === 1 ? 4 : 8;
    }
  })();
  return physical > e ? { performance: physical - e, efficiency: e } : null;
}

/** `n` cells in a grid that fits the rectangle, as square as it can be. Pure. */
export function grid(r: Rect, n: number, pad = 3, gap = 2): Rect[] {
  if (n <= 0 || r.w <= pad * 2 || r.h <= pad * 2) return [];
  const cols = Math.max(1, Math.min(n, Math.round(Math.sqrt((n * (r.w - pad * 2)) / (r.h - pad * 2)))));
  const rows = Math.ceil(n / cols);
  const cw = (r.w - pad * 2 - gap * (cols - 1)) / cols;
  const ch = (r.h - pad * 2 - gap * (rows - 1)) / rows;
  return Array.from({ length: n }, (_, i) => ({ x: round(r.x + pad + (i % cols) * (cw + gap)), y: round(r.y + pad + Math.floor(i / cols) * (ch + gap)), w: round(cw), h: round(ch) }));
}

const round = (v: number): number => Math.round(v * 100) / 100;

const DIE: Record<Tier, { w: number; h: number; gpu: number; gpuShare: number; memory: number }> = {
  base: { w: 132, h: 100, gpu: 10, gpuShare: 0.5, memory: 2 },
  pro: { w: 152, h: 112, gpu: 20, gpuShare: 0.56, memory: 2 },
  max: { w: 184, h: 124, gpu: 32, gpuShare: 0.62, memory: 4 },
  ultra: { w: 150, h: 124, gpu: 32, gpuShare: 0.62, memory: 8 },
};

/** One die's floorplan: CPU clusters on top, a fabric strip, then the GPU, the neural engine and the media/IO blocks. */
function dieLayout(at: Rect, tier: Tier, split: { performance: number; efficiency: number } | null, physical: number): DieLayout {
  const cfg = DIE[tier];
  const pad = 6;
  const gap = 4;
  const inner = { x: at.x + pad, y: at.y + pad, w: at.w - pad * 2, h: at.h - pad * 2 };
  const topH = Math.round((inner.h - 8 - gap * 2) * 0.42);
  const botH = inner.h - topH - 8 - gap * 2;
  const regions: Region[] = [];
  const io = Math.round(inner.w * 0.14);
  const cpuW = inner.w - io - gap * 2;
  const share = tier === 'ultra' ? 2 : 1;
  if (split) {
    const p = Math.ceil(split.performance / share);
    const e = Math.ceil(split.efficiency / share);
    const pw = Math.round((cpuW * p) / (p + e * 0.6));
    const perf: Rect = { x: inner.x, y: inner.y, w: pw, h: topH };
    const eff: Rect = { x: inner.x + pw + gap, y: inner.y, w: cpuW - pw, h: topH };
    regions.push({ kind: 'cpu-perf', ...perf, cells: grid(perf, p) }, { kind: 'cpu-eff', ...eff, cells: grid(eff, e) });
  } else {
    const all: Rect = { x: inner.x, y: inner.y, w: cpuW, h: topH };
    regions.push({ kind: 'cpu-perf', ...all, cells: grid(all, Math.max(1, Math.ceil(physical / share))) });
  }
  regions.push({ kind: 'io', x: inner.x + cpuW + gap, y: inner.y, w: io, h: topH, cells: [] });
  regions.push({ kind: 'fabric', x: inner.x, y: inner.y + topH + gap, w: inner.w, h: 8, cells: [] });
  const by = inner.y + topH + gap * 2 + 8;
  const bw = inner.w - gap * 2;
  const gw = Math.round(bw * cfg.gpuShare);
  const nw = Math.round(bw * 0.22);
  const gpu: Rect = { x: inner.x, y: by, w: gw, h: botH };
  const npu: Rect = { x: inner.x + gw + gap, y: by, w: nw, h: botH };
  regions.push({ kind: 'gpu', ...gpu, cells: grid(gpu, cfg.gpu) }, { kind: 'npu', ...npu, cells: grid(npu, 16, 3, 2) });
  regions.push({ kind: 'io', x: npu.x + nw + gap, y: by, w: bw - gw - nw, h: botH, cells: [] });
  return { ...at, regions };
}

/** Evenly spaced stack of `n` packages in a column. */
function column(x: number, y: number, w: number, h: number, n: number, gap: number): Rect[] {
  const ph = (h - gap * (n - 1)) / n;
  return Array.from({ length: n }, (_, i) => ({ x, y: round(y + i * (ph + gap)), w, h: round(ph) }));
}

/** A row of tiny capacitors along an edge. */
function passiveRow(x: number, y: number, count: number, step: number): Rect[] {
  return Array.from({ length: count }, (_, i) => ({ x: x + i * step, y, w: 6, h: 3 }));
}

const memoryLabel = (bytes: number): string => `${Math.round(bytes / 1024 ** 3).toString()} GB`;

/**
 * The package drawing of an Apple-silicon chip: the die (or two joined dies for an Ultra) in the middle of the substrate with the
 * unified-memory packages beside it, proportions following the tier. A chip the tables do not know gets the same drawing with
 * one undivided CPU block. Pure.
 */
export function applePackage(cpuName: string, physicalCores: number, memoryBytes: number, memoryKind: MemoryTech): PackageLayout {
  const chip = parseAppleChip(cpuName);
  const tier = chip?.tier ?? 'base';
  const cfg = DIE[tier];
  const split = coreSplit(chip, physicalCores);
  const pad = 24;
  const gapDie = 14;
  const memW = 52;
  const dieCount = tier === 'ultra' ? 2 : 1;
  const bridge = tier === 'ultra' ? 10 : 0;
  const dieBlock = cfg.w * dieCount + bridge;
  const sides = tier === 'max' || tier === 'ultra' ? 2 : 1;
  const contentW = dieBlock + (gapDie + memW) * sides;
  const width = contentW + pad * 2;
  const height = cfg.h + pad * 2;
  const dieX = pad + (sides === 2 ? memW + gapDie : 0);
  const dies = Array.from({ length: dieCount }, (_, i) => dieLayout({ x: dieX + i * (cfg.w + bridge), y: pad, w: cfg.w, h: cfg.h }, tier, split, physicalCores));
  const perSide = cfg.memory / sides;
  const gap = perSide > 2 ? 5 : 8;
  const memory: Rect[] = [];
  if (sides === 2) memory.push(...column(pad, pad, memW, cfg.h, perSide, gap));
  memory.push(...column(dieX + dieBlock + gapDie, pad, memW, cfg.h, perSide, gap));
  const passives = [...passiveRow(pad, 9, Math.floor((width - pad * 2) / 14), 14), ...passiveRow(pad, height - 12, Math.floor((width - pad * 2) / 14), 14)];
  const cpuCount = Math.round(physicalCores);
  const legend: LegendEntry[] = [
    {
      kind: 'cpu-perf',
      label: 'CPU',
      detail: split ? `${cpuCount.toString()} cores: ${split.performance.toString()} performance, ${split.efficiency.toString()} efficiency` : `${cpuCount.toString()} cores`,
    },
    { kind: 'gpu', label: 'GPU', detail: 'Graphics, on the same die' },
    { kind: 'npu', label: 'Neural Engine', detail: 'Machine-learning accelerator' },
    { kind: 'memory', label: 'Unified memory', detail: `${memoryLabel(memoryBytes)} ${techLabel(memoryKind)} in ${cfg.memory.toString()} packages` },
  ];
  return {
    kind: 'apple',
    width,
    height,
    substrate: { x: 0.5, y: 0.5, w: width - 1, h: height - 1 },
    lid: { x: pad - 8, y: pad - 8, w: contentW + 16, h: cfg.h + 16 },
    dies,
    memory,
    passives,
    legend,
  };
}

/**
 * Memory soldered to a laptop's mainboard: the SoC package on the left and LPDDR packages beside it, two to a column, on a board
 * area the drawing marks as soldered. The package count follows the size (one per 4 GB, between 2 and 8). Pure.
 */
export function solderedBoard(memoryBytes: number, memoryKind: MemoryTech): PackageLayout {
  const n = Math.max(2, Math.min(8, Math.round(memoryBytes / (4 * 1024 ** 3))));
  const colCount = Math.ceil(n / 2);
  const pw = 56;
  const ph = 36;
  const gx = 14;
  const socW = 96;
  const pad = 28;
  const width = pad * 2 + socW + 20 + colCount * pw + (colCount - 1) * gx;
  const height = 188;
  const socY = (height - socW) / 2;
  const memory: Rect[] = [];
  for (let c = 0; c < colCount; c++) {
    const inCol = Math.min(2, n - c * 2);
    const total = 2 * ph + 14;
    const top = (height - total) / 2;
    const rows = column(pad + socW + 20 + c * (pw + gx), top, pw, inCol === 2 ? total : ph, inCol, 14);
    memory.push(...(inCol === 2 ? rows : [{ ...rows[0]!, y: (height - ph) / 2 }]));
  }
  const soc: Rect = { x: pad, y: socY, w: socW, h: socW };
  return {
    kind: 'soldered',
    width,
    height,
    substrate: { x: 0.5, y: 0.5, w: width - 1, h: height - 1 },
    lid: { x: soc.x + 12, y: soc.y + 12, w: soc.w - 24, h: soc.h - 24 },
    dies: [{ ...soc, regions: [] }],
    memory,
    passives: [...passiveRow(pad, 12, Math.floor((width - pad * 2) / 14), 14), ...passiveRow(pad, height - 15, Math.floor((width - pad * 2) / 14), 14)],
    legend: [
      { kind: 'cpu-perf', label: 'Processor', detail: 'Soldered SoC package' },
      { kind: 'memory', label: 'Memory', detail: `${memoryLabel(memoryBytes)} ${techLabel(memoryKind)} in ${n.toString()} packages, soldered` },
    ],
  };
}
