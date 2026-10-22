import type { DashboardLayout, NamedPreset, TrayItems, Widget, WidgetKind, WidgetMetric, WidgetSize } from '../../core/api.types';
import { newWidget } from '../../features/widgets/widget-model';

/**
 * The built-in presets, as the engine generates them (`dyno_core::widgets::Preset`). The engine regenerates them on every
 * load; this copy exists for the mock backend and tests only, never as the source of truth.
 */
const w = (id: string, kind: WidgetKind, metric: WidgetMetric, size: WidgetSize, thresholds?: [number, number]): Widget => ({
  ...newWidget(kind, metric, []),
  id,
  size,
  thresholds: thresholds ? { warning: thresholds[0], critical: thresholds[1] } : { warning: null, critical: null },
});

export const PRESET_WIDGETS: Record<string, Widget[]> = {
  overview: [w('health', 'health', 'cpuTotal', 's'), w('cpu', 'graph', 'cpuTotal', 'm'), w('memory', 'ring', 'memory', 's'), w('disk', 'graph', 'diskActivity', 'm'), w('network', 'graph', 'networkDown', 'm'), w('top', 'topProcesses', 'cpuTotal', 'l'), w('issues', 'issues', 'cpuTotal', 'm')],
  developer: [w('cpu-cores', 'heatmap', 'cpuPerCore', 'l'), w('cpu', 'graph', 'cpuTotal', 'm'), w('memory', 'graph', 'memory', 'm'), w('swap', 'bigNumber', 'swap', 's'), w('disk', 'graph', 'diskActivity', 'm'), w('disk-free', 'ring', 'diskFree', 's'), w('top', 'topProcesses', 'cpuTotal', 'l')],
  gaming: [w('gpu', 'graph', 'gpuLoad', 'l'), w('cpu', 'graph', 'cpuTotal', 'm'), w('temp', 'bigNumber', 'temperature', 's', [80, 92]), w('fan', 'bigNumber', 'fan', 's'), w('memory', 'ring', 'memory', 's'), w('network', 'graph', 'networkDown', 'm'), w('top', 'topProcesses', 'cpuTotal', 'm')],
  battery: [w('battery', 'ring', 'battery', 'm'), w('cpu', 'graph', 'cpuTotal', 'm'), w('temp', 'bigNumber', 'temperature', 's', [80, 92]), w('top', 'topProcesses', 'cpuTotal', 'l'), w('issues', 'issues', 'cpuTotal', 'm')],
};

const NAMES: Record<string, string> = { overview: 'Overview', developer: 'Developer', gaming: 'Gaming', battery: 'Battery' };

export function builtinPresets(): NamedPreset[] {
  return Object.keys(PRESET_WIDGETS).map((id) => ({ id, name: NAMES[id]!, builtin: true, widgets: structuredClone(PRESET_WIDGETS[id]!) }));
}

export function layoutFromPreset(id: string): DashboardLayout {
  return { version: 1, widgets: structuredClone(PRESET_WIDGETS[id] ?? PRESET_WIDGETS['overview']!), presets: builtinPresets() };
}

export function defaultTrayItems(): TrayItems {
  return {
    version: 1,
    items: [
      { id: 'cpu', metric: 'cpuTotal', style: 'verticalBars', opens: 'ownPopover' },
      { id: 'memory', metric: 'memory', style: 'verticalBars', opens: 'ownPopover' },
    ],
  };
}
