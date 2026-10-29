import { TestBed } from '@angular/core/testing';
import { ChipPackage } from './chip-package';
import { DeviceArt } from './device-art';
import type { DimmSlotModel, SocPackageModel } from './hardware-models';
import { MemorySlots } from './memory-slots';

const GB = 1024 ** 3;
const mod = (tech: 'ddr4' | 'ddr5', form: 'dimm' | 'soDimm' = 'dimm') => ({ tech, form, sizeLabel: '16 GB', capacityBytes: 16 * GB, ratedMts: 3200, runningMts: 2666, maker: 'Corsair', part: 'CMK16' });

function slots(list: DimmSlotModel[], alt = 'Two slots') {
  const f = TestBed.createComponent(MemorySlots);
  f.componentRef.setInput('slots', list);
  f.componentRef.setInput('alt', alt);
  f.detectChanges();
  return f.nativeElement as HTMLElement;
}

/** The tooltip text of slot `i`: Material describes the slot with a hidden copy of its message. */
function tip(el: HTMLElement, i: number): string {
  const ids = el.querySelectorAll('.slot')[i]!.getAttribute('aria-describedby')!.split(' ');
  return ids.map((id) => document.getElementById(id)?.textContent ?? '').join('\n');
}

describe('MemorySlots', () => {
  const full: DimmSlotModel = { name: 'DIMM_A1', channel: 'A', module: mod('ddr5'), tags: [{ label: 'Dual channel', tone: 'ok' }], note: 'Runs below its rated speed.' };
  const free: DimmSlotModel = { name: 'DIMM_B1', channel: 'B', module: null, tags: [], note: null };

  it('draws a socket per slot, grouped by channel, with a text alternative', () => {
    const el = slots([full, free], 'Memory: 1 of 2 slots filled');
    expect(el.querySelector('svg[role=img]')!.getAttribute('aria-label')).toBe('Memory: 1 of 2 slots filled');
    expect([...el.querySelectorAll('.channel')].map((h) => h.textContent?.trim())).toEqual(['Channel A', 'Channel B']);
    expect(el.querySelectorAll('.slot')).toHaveLength(2);
    expect(el.querySelectorAll('.slot.free')).toHaveLength(1);
  });

  it('prints size, type and the rated to running speed beside the drawing and keeps maker and part in the tooltip', () => {
    const el = slots([full]);
    const info = el.querySelector('.info')!.textContent!;
    expect(info).toContain('16 GB');
    expect(info).toContain('DDR5');
    expect(info).toContain('3200 → 2666 MT/s');
    expect(info).toContain('Dual channel');
    expect(tip(el, 0)).toContain('CMK16');
    expect(tip(el, 0)).toContain('Runs below its rated speed.');
    expect(el.querySelector('.speed.below')).not.toBeNull();
  });

  it('makes each slot keyboard focusable with a spoken description', () => {
    const el = slots([full, free]);
    const rows = [...el.querySelectorAll<HTMLElement>('.slot')];
    expect(rows.every((r) => r.tabIndex === 0)).toBe(true);
    expect(rows[0]!.getAttribute('aria-label')).toBe('DIMM_A1: 16 GB DDR5, 3200 → 2666 MT/s, Corsair. Runs below its rated speed.');
    expect(rows[1]!.getAttribute('aria-label')).toBe('DIMM_B1: free slot');
    expect(tip(el, 0)).toContain('Part number');
    expect(tip(el, 1)).toContain('Nothing is installed in this slot.');
  });

  it('draws an empty socket with its latches open and a module with them closed', () => {
    const el = slots([full, free]);
    const rows = el.querySelectorAll('.slot');
    expect(rows[0]!.querySelectorAll('.latch.open')).toHaveLength(0);
    expect(rows[1]!.querySelectorAll('.latch.open')).toHaveLength(2);
    expect(rows[1]!.querySelector('.ghost')).not.toBeNull();
  });

  it('draws DDR5 with a PMIC and DDR4 without; keys empty sockets like the installed modules', () => {
    expect(slots([full]).querySelectorAll('.inductor')).toHaveLength(2);
    const el = slots([{ ...full, module: mod('ddr4', 'soDimm'), tags: [] }, free]);
    expect(el.querySelectorAll('.inductor')).toHaveLength(0);
    const so = el.querySelectorAll('.art');
    expect(so[0]!.getAttribute('viewBox')).toBe(so[1]!.getAttribute('viewBox'));
  });

  it('falls back to a DIMM for an empty bank, a missing speed and an unknown maker', () => {
    const el = slots([{ name: 'Slot 1', channel: null, module: { ...mod('ddr4'), ratedMts: null, runningMts: null, maker: null, part: null }, tags: [], note: null }]);
    expect(el.querySelector('.channel')).toBeNull();
    expect(el.querySelector('.speed')).toBeNull();
    expect(el.querySelector('.maker')!.textContent).toBe('Maker unknown');
    expect(tip(el, 0)).toContain('Maker: Unknown');
    expect(slots([]).querySelectorAll('.slot')).toHaveLength(0);
  });
});

describe('ChipPackage', () => {
  const render = (m: SocPackageModel) => {
    const f = TestBed.createComponent(ChipPackage);
    f.componentRef.setInput('model', m);
    f.detectChanges();
    return f.nativeElement as HTMLElement;
  };
  const apple: SocPackageModel = { style: 'apple', name: 'Apple M4 Pro', physicalCores: 14, memoryBytes: 48 * GB, memoryTech: 'lpddr5', alt: 'Unified memory, 48 GB', title: 'Apple M4 Pro' };

  it('draws the die, its cores and the memory packages, with a legend', () => {
    const el = render(apple);
    expect(el.querySelector('svg')!.getAttribute('aria-label')).toBe('Unified memory, 48 GB');
    expect(el.querySelectorAll('.dram')).toHaveLength(2);
    expect(el.querySelectorAll('.cell[data-kind=cpu-perf]')).toHaveLength(10);
    expect(el.querySelectorAll('.legend li')).toHaveLength(4);
    expect(el.querySelector('h3')!.textContent).toBe('Apple M4 Pro');
    expect(el.querySelector('.bridge')).toBeNull();
  });

  it('joins two dies for an Ultra', () => {
    expect(render({ ...apple, name: 'Apple M2 Ultra', physicalCores: 24 }).querySelector('.bridge')).not.toBeNull();
  });

  it('draws soldered packages on a board with solder-ball outlines', () => {
    const el = render({ ...apple, style: 'soldered', name: 'Intel Core Ultra', memoryBytes: 16 * GB, title: 'Memory soldered to the mainboard' });
    expect(el.querySelectorAll('.dram')).toHaveLength(4);
    expect(el.querySelector('.substrate.board')).not.toBeNull();
    expect(el.querySelectorAll('.balls').length).toBeGreaterThan(4);
  });
});

describe('DeviceArt', () => {
  const render = (kind: 'cpu' | 'nvme' | 'sata' | 'soldered' | 'removable', bytes = 0) => {
    const f = TestBed.createComponent(DeviceArt);
    f.componentRef.setInput('kind', kind);
    f.componentRef.setInput('totalBytes', bytes);
    f.detectChanges();
    return f.nativeElement as HTMLElement;
  };

  it('draws each device and hides the decoration from assistive technology', () => {
    for (const k of ['cpu', 'nvme', 'sata', 'soldered', 'removable'] as const) {
      const svg = render(k).querySelector('svg')!;
      expect(svg.getAttribute('aria-hidden')).toBe('true');
      expect(svg.querySelectorAll('rect,path').length).toBeGreaterThan(5);
    }
  });

  it('puts more NAND on bigger M.2 sticks', () => {
    const nand = (gb: number) => render('nvme', gb * GB).querySelectorAll('rect.chip:not(.small)').length;
    expect(nand(250)).toBe(nand(1000) - 1);
    expect(nand(2000)).toBe(nand(1000) + 1);
  });
});
