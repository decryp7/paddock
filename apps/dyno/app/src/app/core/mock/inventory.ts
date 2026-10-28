import type { Hint, MemoryModule, SystemInventory } from '../api.types';
import type { MockOs } from './os';

const GB = 1000 ** 3;
/** Memory sizes are binary (16 GiB reads "16 GB", like Activity Monitor). */
const GiB = 1024 ** 3;

// Read when the module loads: the router drops the query string soon after.
const OPENED_WITH = typeof location === 'undefined' ? '' : location.search;

/**
 * The machines the mock can pretend to be, to exercise every hardware drawing. The OS picks a default; `?inv=<name>` in the page's
 * address picks another one in the browser build.
 */
export type InventoryVariant = 'ddr4' | 'ddr5' | 'sodimm' | 'sodimm-dual' | 'soldered' | 'apple' | 'generic';

export const INVENTORY_VARIANTS: readonly InventoryVariant[] = ['ddr4', 'ddr5', 'sodimm', 'sodimm-dual', 'soldered', 'apple', 'generic'];

const DEFAULT_VARIANT: Record<MockOs, InventoryVariant> = { windows: 'ddr4', macos: 'apple', linux: 'sodimm' };

/** The `?inv=` variant of a query string, or null. */
export function inventoryVariantFromSearch(search: string): InventoryVariant | null {
  const v = new URLSearchParams(search).get('inv');
  return INVENTORY_VARIANTS.find((x) => x === v) ?? null;
}

const stick = (slot: string, gb: number, kind: MemoryModule['kind'], mts: number, configured: number, maker: string, part: string, form: MemoryModule['formFactor'] = 'dimm'): MemoryModule => ({
  slot,
  bank: null,
  capacityBytes: gb * GiB,
  speedMts: mts,
  configuredSpeedMts: configured,
  kind,
  formFactor: form,
  manufacturer: maker,
  partNumber: part,
});

const disk = (name: string, gb: number) => ({ name, totalBytes: gb * GB, removable: false });

function appleSilicon(): SystemInventory {
  return {
    os: 'macOS 15',
    hostName: 'MacBook-Pro',
    cpu: { name: 'Apple M4 Pro', physicalCores: 14, logicalCores: 14, frequencyMhz: 4500 },
    memory: { unified: { total: 48 * GiB, kind: 'lpddr5', manufacturer: 'Apple', upgradeable: false } },
    disks: [disk('Apple SSD AP1024', 994)],
    hints: [{ kind: 'notUpgradeable', message: 'The memory is part of the chip package and cannot be upgraded. Choose the size when buying.', slot: null }],
  };
}

function solderedLaptop(): SystemInventory {
  return {
    os: 'Windows 11',
    hostName: 'ULTRABOOK',
    cpu: { name: 'Intel Core Ultra 7 155H', physicalCores: 16, logicalCores: 22, frequencyMhz: 3800 },
    memory: { unified: { total: 32 * GiB, kind: 'lpddr5', manufacturer: 'Samsung', upgradeable: false } },
    disks: [disk('WD PC SN740 NVMe', 954)],
    hints: [{ kind: 'notUpgradeable', message: 'The memory is soldered to the mainboard and cannot be upgraded.', slot: null }],
  };
}

function ddr5Desktop(): SystemInventory {
  const part = 'KF560C40-32';
  return {
    os: 'Windows 11',
    hostName: 'STUDIO-PC',
    cpu: { name: 'Intel Core i9-13900K', physicalCores: 24, logicalCores: 32, frequencyMhz: 3000 },
    memory: {
      slots: {
        slots: 4,
        maxCapacityBytes: 192 * GiB,
        modules: [stick('DIMM_A2', 32, 'ddr5', 6000, 6000, 'Kingston', part), stick('DIMM_B2', 32, 'ddr5', 6000, 6000, 'Kingston', part)],
      },
    },
    disks: [disk('Samsung 990 PRO NVMe', 1863), disk('Seagate BarraCuda SATA', 3726)],
    hints: [
      { kind: 'multiChannel', message: 'Both sticks match and run as a dual-channel pair.', slot: null },
      { kind: 'freeSlots', message: 'Two slots are free. Fill DIMM_A1 and DIMM_B1 with another matching pair to reach 128 GB; four sticks often run slower than two.', slot: 'DIMM_A1' },
      { kind: 'maxCapacity', message: 'The board supports up to 192 GB.', slot: null },
    ],
  };
}

function sodimmLaptop(dual: boolean): SystemInventory {
  const a = stick('SODIMM A', 16, 'ddr5', 5600, 5600, 'Crucial', 'CT16G56C46S5.M8G1', 'soDimm');
  const modules = dual ? [a, stick('SODIMM B', 16, 'ddr5', 5600, 5600, 'Crucial', 'CT16G56C46S5.M8G1', 'soDimm')] : [a];
  const hints: Hint[] = dual
    ? [
        { kind: 'multiChannel', message: 'Both sticks match and run as a dual-channel pair.', slot: null },
        { kind: 'maxCapacity', message: 'You can add up to 32 GB more before the board maximum of 64 GB, by replacing both sticks with 32 GB ones.', slot: null },
      ]
    : [
        { kind: 'singleChannel', message: 'Only one stick is installed, so memory runs single channel. A second matching 16 GB stick would roughly double memory bandwidth.', slot: null },
        { kind: 'freeSlots', message: 'One slot is free: add a matching 16 GB DDR5-5600 SO-DIMM in SODIMM B.', slot: 'SODIMM B' },
      ];
  return {
    os: 'Linux 6.8',
    hostName: 'dev-laptop',
    cpu: { name: 'AMD Ryzen 9 7940HS', physicalCores: 8, logicalCores: 16, frequencyMhz: 4000 },
    memory: { slots: { slots: 2, maxCapacityBytes: 64 * GiB, modules } },
    disks: [disk('WD Blue SN570 NVMe', 954)],
    hints,
  };
}

/** A machine whose firmware says almost nothing: unknown type, no maker, no slot count. */
function genericBoard(): SystemInventory {
  const m: MemoryModule = { slot: 'Memory slot', bank: null, capacityBytes: 8 * GiB, speedMts: null, configuredSpeedMts: null, kind: 'unknown', formFactor: 'unknown', manufacturer: null, partNumber: null };
  return {
    os: 'Windows 10',
    hostName: null,
    cpu: { name: 'Unknown processor', physicalCores: 4, logicalCores: 4, frequencyMhz: 2400 },
    memory: { slots: { slots: 0, maxCapacityBytes: null, modules: [m] } },
    disks: [{ name: 'Generic storage device', totalBytes: 240 * GB, removable: true }],
    hints: [],
  };
}

function ddr4Desktop(): SystemInventory {
  const hints: Hint[] = [
    { kind: 'mismatchedSize', message: 'The two sticks have different sizes (16 GB and 8 GB), so part of the memory cannot run dual channel.', slot: 'DIMM_A2' },
    { kind: 'mismatchedSpeed', message: 'The 16 GB stick is rated 3200 MT/s but runs at 2666 MT/s to match the 8 GB stick.', slot: 'DIMM_A1' },
    { kind: 'freeSlots', message: 'Two slots are free: add a matching 16 GB DDR4-3200 stick in DIMM_B1 and another in DIMM_B2 for dual channel.', slot: 'DIMM_B1' },
    { kind: 'maxCapacity', message: 'The board supports up to 128 GB.', slot: null },
  ];
  return {
    os: 'Windows 11',
    hostName: 'DEV-DESKTOP',
    cpu: { name: 'AMD Ryzen 7 5800X', physicalCores: 8, logicalCores: 16, frequencyMhz: 3800 },
    memory: {
      slots: {
        slots: 4,
        maxCapacityBytes: 128 * GiB,
        modules: [stick('DIMM_A1', 16, 'ddr4', 3200, 2666, 'Corsair', 'CMK16GX4M1E3200C16'), stick('DIMM_A2', 8, 'ddr4', 2666, 2666, 'Kingston', 'KVR26N19S6/8')],
      },
    },
    disks: [disk('Samsung PM9A1 NVMe', 954), disk('Crucial MX500 SATA', 931)],
    hints,
  };
}

/** What the mock reports as installed; see {@link InventoryVariant}. The OS sets the default machine. */
export function mockInventory(os: MockOs, variant?: InventoryVariant): SystemInventory {
  switch (variant ?? inventoryVariantFromSearch(OPENED_WITH) ?? DEFAULT_VARIANT[os]) {
    case 'ddr5':
      return ddr5Desktop();
    case 'sodimm':
      return sodimmLaptop(false);
    case 'sodimm-dual':
      return sodimmLaptop(true);
    case 'soldered':
      return solderedLaptop();
    case 'apple':
      return appleSilicon();
    case 'generic':
      return genericBoard();
    case 'ddr4':
      return ddr4Desktop();
  }
}
