import type { DimmForm, MemoryTech } from './hardware-models';

const TECH_LABEL: Record<MemoryTech, string> = { ddr2: 'DDR2', ddr3: 'DDR3', ddr4: 'DDR4', ddr5: 'DDR5', lpddr3: 'LPDDR3', lpddr4: 'LPDDR4', lpddr5: 'LPDDR5', unknown: 'Memory' };

/** "DDR4", "LPDDR5"; "Memory" when the type is unknown. */
export const techLabel = (t: MemoryTech): string => TECH_LABEL[t];

/** The speed line of a label: "3200 → 2666 MT/s" when running below rated, "2666 MT/s" otherwise, null when unknown. */
export function speedText(ratedMts: number | null, runningMts: number | null): string | null {
  if (ratedMts && runningMts && ratedMts !== runningMts) return `${ratedMts.toString()} → ${runningMts.toString()} MT/s`;
  const one = runningMts ?? ratedMts;
  return one ? `${one.toString()} MT/s` : null;
}

export interface Span {
  x: number;
  w: number;
}

/** A module as drawn, in SVG user units with the module's top-left corner at the origin. Everything is derived from the data. */
export interface ModuleGeometry {
  form: DimmForm;
  width: number;
  height: number;
  /** Centre and width of the key notch cut into the contact edge: its position tells the generations apart. */
  notch: Span & { depth: number };
  /** Height of the gold contact strip along the bottom edge. */
  contactsH: number;
  chipY: number;
  chipH: number;
  chips: Span[];
  spd: Span;
  pmic: Span | null;
  /** Tiny passives between the chips and the contacts. */
  passives: number[];
}

/** The 133.35 mm DIMM is drawn 400 wide and the 67.6 mm SO-DIMM 204 wide, both 72 tall (compressed a little, like a diagram). */
export const MODULE_HEIGHT = 72;
const WIDTH: Record<DimmForm, number> = { dimm: 400, soDimm: 204 };
const CONTACTS_H = 8;
const MARGIN = 8;

/**
 * Where the key notch sits along the contact edge, as a fraction of the module's width. Real keys differ by generation so a
 * module cannot be fitted to the wrong socket: DDR2 near the middle, DDR3 left of it, DDR4 a little closer to the middle and
 * DDR5 further left. The values are illustrative (diagram accuracy), not tolerances.
 */
const NOTCH_FRACTION: Record<DimmForm, Record<'ddr2' | 'ddr3' | 'ddr4' | 'ddr5', number>> = {
  dimm: { ddr2: 0.53, ddr3: 0.4, ddr4: 0.46, ddr5: 0.35 },
  soDimm: { ddr2: 0.5, ddr3: 0.42, ddr4: 0.37, ddr5: 0.3 },
};

/** The generation a kind is keyed as: LPDDR modules (CAMM and friends) borrow the DDR key of the same number. */
export function keyGeneration(kind: MemoryTech): 'ddr2' | 'ddr3' | 'ddr4' | 'ddr5' {
  switch (kind) {
    case 'ddr2':
    case 'ddr3':
    case 'ddr4':
    case 'ddr5':
      return kind;
    case 'lpddr3':
      return 'ddr3';
    case 'lpddr4':
      return 'ddr4';
    case 'lpddr5':
      return 'ddr5';
    default:
      return 'ddr4';
  }
}

/**
 * The DRAM packages visible on the front of a module. The data has no rank or side count, so it is estimated from the size: a
 * full-size module shows 8 (a 64-bit bus of x8 parts; bigger modules put the rest on the back), an SO-DIMM 4, or 8 from 32 GB.
 */
export function chipCount(form: DimmForm, capacityBytes: number): number {
  if (form === 'dimm') return 8;
  return capacityBytes >= 32 * 1024 ** 3 ? 8 : 4;
}

/** Spreads `count` packages evenly over a span, each with a gutter, packages no wider than `maxW`. */
function spread(from: number, to: number, count: number, maxW: number): Span[] {
  if (count <= 0 || to <= from) return [];
  const pitch = (to - from) / count;
  const w = Math.min(maxW, Math.floor(pitch - 6));
  return Array.from({ length: count }, (_, i) => ({ x: Math.round(from + pitch * i + (pitch - w) / 2), w }));
}

/** The full drawing geometry of one module. Pure and total: any combination of kind, form and size gives a valid layout. */
export function moduleGeometry(kind: MemoryTech, form: DimmForm, capacityBytes: number): ModuleGeometry {
  const generation = keyGeneration(kind);
  const width = WIDTH[form];
  const notchW = form === 'dimm' ? 8 : 6;
  const notchX = Math.round(width * NOTCH_FRACTION[form][generation]);
  const hasPmic = generation === 'ddr5';
  // The service strip around the notch holds the SPD (and the PMIC on DDR5); the DRAM packages split either side of it.
  const strip = hasPmic ? (form === 'dimm' ? 60 : 44) : form === 'dimm' ? 40 : 30;
  const left = notchX - strip / 2;
  const right = notchX + strip / 2;
  const n = chipCount(form, capacityBytes);
  const leftSpan = left - MARGIN;
  const rightSpan = width - MARGIN - right;
  const nLeft = Math.min(n, Math.max(0, Math.round((n * leftSpan) / (leftSpan + rightSpan))));
  const maxW = form === 'dimm' ? 36 : 24;
  const chips = [...spread(MARGIN, left, nLeft, maxW), ...spread(right, width - MARGIN, n - nLeft, maxW)];
  const spd: Span = { x: Math.round(notchX + (hasPmic ? 8 : -7)), w: 14 };
  const pmic: Span | null = hasPmic ? { x: Math.round(notchX - 24), w: 24 } : null;
  const passives: number[] = [];
  for (let x = MARGIN + 6; x < width - MARGIN - 4; x += 14) {
    if (Math.abs(x - notchX) > notchW + 4) passives.push(x);
  }
  return {
    form,
    width,
    height: MODULE_HEIGHT,
    notch: { x: notchX, w: notchW, depth: 7 },
    contactsH: CONTACTS_H,
    chipY: 14,
    chipH: form === 'dimm' ? 28 : 26,
    chips,
    spd,
    pmic,
    passives,
  };
}

/** The outline of the module face: rounded top corners, the key notch cut into the bottom edge and a latch recess at each end. */
export function facePath(g: ModuleGeometry): string {
  const { width: w, height: h, notch } = g;
  const x1 = notch.x - notch.w / 2;
  const x2 = notch.x + notch.w / 2;
  const cut1 = h - 22;
  const cut2 = h - 14;
  return [
    `M3 0H${(w - 3).toString()}A3 3 0 0 1 ${w.toString()} 3V${cut1.toString()}A4 4 0 0 0 ${w.toString()} ${cut2.toString()}V${h.toString()}`,
    `H${x2.toString()}V${(h - notch.depth).toString()}H${x1.toString()}V${h.toString()}H0`,
    `V${cut2.toString()}A4 4 0 0 0 0 ${cut1.toString()}V3A3 3 0 0 1 3 0Z`,
  ].join('');
}
