import type { CpuInfo, MemoryFormFactor, MemoryKind } from '../../core/api.types';
import { formatBytes } from '../../shared/format';
import type { DimmSlotModel, MemoryTech, SlotTag, SocPackageModel } from './hardware-models';
import { isAppleSilicon } from './package-geometry';
import { type SlotModel, hintLabel, kindLabel } from './slot-model';

/** The only place Dyno's inventory types meet the hardware drawings' plain models. */
export const memoryTech = (k: MemoryKind): MemoryTech => (k === 'other' ? 'unknown' : k);

const form = (f: MemoryFormFactor) => (f === 'soDimm' ? 'soDimm' : 'dimm');

/** The slots to draw: modules, tags from the engine's hints, and the hint text for the popover. */
export function toDimmSlots(model: SlotModel): DimmSlotModel[] {
  return model.slots.map((s) => {
    const tags: SlotTag[] = [];
    const label = hintLabel(s.hint);
    if (label) tags.push({ label, tone: s.hint?.kind === 'freeSlots' ? 'info' : 'warn' });
    if (s.paired) tags.push({ label: 'Dual channel', tone: 'ok' });
    const m = s.module;
    return {
      name: s.name,
      channel: s.channel,
      module: m && {
        tech: memoryTech(m.kind),
        form: form(m.formFactor),
        sizeLabel: formatBytes(m.capacityBytes),
        capacityBytes: m.capacityBytes,
        ratedMts: m.speedMts,
        runningMts: m.configuredSpeedMts,
        maker: m.manufacturer,
        part: m.partNumber,
      },
      tags,
      note: s.hint?.message ?? null,
    };
  });
}

/** Unified or soldered memory next to the processor. */
export function toSocPackage(cpu: CpuInfo, memory: { total: number; kind: MemoryKind }): SocPackageModel {
  const apple = isAppleSilicon(cpu.name);
  const what = `${formatBytes(memory.total)} ${kindLabel(memory.kind)}`;
  return {
    style: apple ? 'apple' : 'soldered',
    name: cpu.name,
    physicalCores: cpu.physicalCores,
    memoryBytes: memory.total,
    memoryTech: memoryTech(memory.kind),
    alt: apple ? `Unified memory, ${what}, part of the ${cpu.name} chip package` : `Soldered memory, ${what}, on the mainboard next to the processor`,
    title: apple ? cpu.name : 'Memory soldered to the mainboard',
  };
}
