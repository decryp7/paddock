import { type MockOs, osProfile } from './os';

const OSES: MockOs[] = ['windows', 'macos', 'linux'];

describe.each(OSES)('osProfile(%s)', (os) => {
  const p = osProfile(os);
  it('is self-consistent', () => {
    expect(p.os).toBe(os);
    expect(p.specs.length).toBeGreaterThan(10);
    expect(p.cores).toBeGreaterThan(0);
    expect(p.cpuMaxMhz).toBeGreaterThan(p.cpuBaseMhz);
    expect(p.memoryTotal).toBeGreaterThan(0);
  });
  it('has every role process in its catalogue', () => {
    const names = new Set(p.specs.map((s) => s.name));
    for (const role of ['rustc', 'cargo', 'chrome', 'code', 'node', 'indexer', 'agent'] as const) expect(names.has(p.names[role])).toBe(true);
  });
  it('lists two disks, the free space shrinking with time', () => {
    const now = p.disks(0);
    expect(now).toHaveLength(2);
    expect(p.disks(10)[0]!.free).toBeLessThan(now[0]!.free);
    expect(p.disks(10)[1]!.free).toBe(now[1]!.free);
  });
  it('lists two NICs that follow the wobble', () => {
    const lo = p.network(() => -1);
    const hi = p.network(() => 1);
    expect(lo).toHaveLength(2);
    expect(hi[0]!.rxBps).toBeGreaterThan(lo[0]!.rxBps);
  });
  it('starts with a root process that has no parent', () => {
    expect(p.specs[0]!.parent).toBeUndefined();
  });
});

describe('OS differences', () => {
  it('only Windows uses backslash paths and .exe names', () => {
    expect(osProfile('windows').home).toContain('\\');
    expect(osProfile('windows').names.rustc).toBe('rustc.exe');
    expect(osProfile('macos').home).toBe('/Users/alex');
    expect(osProfile('linux').home).toBe('/home/alex');
    expect(osProfile('macos').rootPid).toBe(1);
    expect(osProfile('windows').rootPid).toBe(4);
  });
  it('the primary disk and boot wording differ', () => {
    expect(osProfile('windows').primaryDisk).toBe('Drive C:');
    expect(osProfile('macos').bootLabel).toBe('macOS started');
    expect(osProfile('linux').bootLabel).toBe('Linux started');
  });
});
