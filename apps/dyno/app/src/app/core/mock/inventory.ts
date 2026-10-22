import type { Hint, MemoryModule, SystemInventory } from '../api.types';
import type { MockOs } from './os';

const GB = 1024 ** 3;

const stick = (slot: string, gb: number, kind: MemoryModule['kind'], mts: number, configured: number, maker: string, part: string, form: MemoryModule['formFactor'] = 'dimm'): MemoryModule => ({
  slot,
  bank: null,
  capacityBytes: gb * GB,
  speedMts: mts,
  configuredSpeedMts: configured,
  kind,
  formFactor: form,
  manufacturer: maker,
  partNumber: part,
});

/** What the mock reports as installed: a desktop with two of four slots filled and a mismatch (Windows), Apple silicon unified memory (macOS), a full dual-channel laptop (Linux). */
export function mockInventory(os: MockOs): SystemInventory {
  if (os === 'macos') {
    return {
      os: 'macOS 15',
      hostName: 'MacBook-Pro',
      cpu: { name: 'Apple M4 Pro', physicalCores: 14, logicalCores: 14, frequencyMhz: 4500 },
      memory: { unified: { total: 48 * GB, kind: 'lpddr5', manufacturer: 'Apple', upgradeable: false } },
      disks: [{ name: 'Apple SSD AP1024', totalBytes: 994 * GB, removable: false }],
      hints: [{ kind: 'notUpgradeable', message: 'The memory is part of the chip package and cannot be upgraded. Choose the size when buying.', slot: null }],
    };
  }
  if (os === 'linux') {
    const memory = { slots: { slots: 2, maxCapacityBytes: 64 * GB, modules: [stick('SODIMM A', 16, 'ddr5', 5600, 5600, 'Crucial', 'CT16G56C46S5.M8G1', 'soDimm'), stick('SODIMM B', 16, 'ddr5', 5600, 5600, 'Crucial', 'CT16G56C46S5.M8G1', 'soDimm')] } };
    return {
      os: 'Linux 6.8',
      hostName: 'dev-laptop',
      cpu: { name: 'AMD Ryzen 9 7940HS', physicalCores: 8, logicalCores: 16, frequencyMhz: 4000 },
      memory,
      disks: [{ name: 'WD Blue SN570', totalBytes: 954 * GB, removable: false }],
      hints: [
        { kind: 'multiChannel', message: 'Both sticks match and run as a dual-channel pair.', slot: null },
        { kind: 'maxCapacity', message: 'You can add up to 32 GB more before the board maximum of 64 GB, by replacing both sticks with 32 GB ones.', slot: null },
      ],
    };
  }
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
        maxCapacityBytes: 128 * GB,
        modules: [stick('DIMM_A1', 16, 'ddr4', 3200, 2666, 'Corsair', 'CMK16GX4M1E3200C16'), stick('DIMM_A2', 8, 'ddr4', 2666, 2666, 'Kingston', 'KVR26N19S6/8')],
      },
    },
    disks: [{ name: 'Samsung PM9A1 NVMe', totalBytes: 954 * GB, removable: false }],
    hints,
  };
}
