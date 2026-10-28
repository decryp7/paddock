import { driveKind, nandCount, nandSlots } from './device-geometry';

describe('driveKind', () => {
  it('picks the drawing from the name and the removable flag', () => {
    expect(driveKind('Samsung PM9A1 NVMe', false)).toBe('nvme');
    expect(driveKind('WD Blue SN570', false)).toBe('nvme');
    expect(driveKind('Samsung 990 PRO', false)).toBe('nvme');
    expect(driveKind('Crucial MX500 SATA', false)).toBe('sata');
    expect(driveKind('Apple SSD AP1024', false)).toBe('soldered');
    expect(driveKind('Anything', true)).toBe('removable');
  });

  it('counts NAND packages by size and lays them out left to right', () => {
    expect(nandCount(256 * 1000 ** 3)).toBe(1);
    expect(nandCount(954 * 1000 ** 3)).toBe(2);
    expect(nandCount(2 * 1000 ** 4)).toBe(3);
    expect(nandSlots(3, 130, 44, 6)).toEqual([130, 180, 230]);
  });
});
