import type { SystemSample } from '../../core/api.types';
import { MockBackend } from '../../core/mock-backend';
import { builtinPresets, layoutFromPreset } from '../../core/mock/widget-presets';
import { accepts, applyPreset, defaultMetric, isPercent, metricLabel, moveWidget, newTrayItem, newWidget, readCounter, readMetric, removeWidget, resizeWidget, seriesFor, styleFromId, thresholdTone, trayStyleId, sentenceCase } from './widget-model';

const sample = (): SystemSample => new MockBackend({ latencyMs: 0, now: () => 1_700_000_000_000 }).liveUpdate('summary').system;

describe('performance counter widgets', () => {
  it('reads a counter reading with its unit, a percentage scale only for percent counters', () => {
    const r = readCounter({ path: '\\A\\b', unit: 'bytesPerSec', value: { available: 1572864 } });
    expect(r).toEqual({ value: 1572864, text: ['1.5', 'MB/s'], percent: null, reason: null });
    expect(readCounter({ path: 'p', unit: 'percent', value: { available: 130 } }).percent).toBe(100);
    expect(readCounter({ path: 'p', unit: 'percent', value: { available: 42 } }).text).toEqual(['42', '%']);
  });

  it('says why a counter has no value', () => {
    expect(readCounter(undefined).reason).toMatch(/Waiting/);
    const gone = readCounter({ path: 'p', unit: 'count', value: { unavailable: 'The device is gone.' } });
    expect([gone.value, gone.reason]).toEqual([null, 'The device is gone.']);
  });

  it('titles a counter widget by the counter, not by its path', () => {
    expect(metricLabel({ perfCounter: '\\PhysicalDisk(0 C:)\\Disk Read Bytes/sec' })).toBe('Disk Read Bytes/sec (0 C:)');
    expect(metricLabel({ perfCounter: 'x' })).toBe('Counter x');
  });
});

describe('widget model', () => {
  it('mirrors the engine rules for which kind accepts which metric', () => {
    expect(accepts('heatmap', 'cpuPerCore')).toBe(true);
    expect(accepts('heatmap', 'cpuTotal')).toBe(false);
    expect(accepts('ring', 'memory')).toBe(true);
    expect(accepts('ring', 'networkUp')).toBe(false);
    expect(accepts('topProcesses', 'gpuLoad')).toBe(true);
    expect(accepts('topProcesses', 'battery')).toBe(false);
    expect(accepts('graph', 'fan')).toBe(true);
    expect(isPercent('battery')).toBe(true);
    expect(isPercent('fan')).toBe(false);
    expect(defaultMetric('heatmap')).toBe('cpuPerCore');
    expect(defaultMetric('graph')).toBe('cpuTotal');
  });

  it('creates widgets with unique ids and sensible sizes', () => {
    const a = newWidget('graph', 'cpuTotal', []);
    const b = newWidget('graph', 'memory', [a]);
    expect(a.id).not.toBe(b.id);
    expect(a.size).toBe('m');
    expect(newWidget('topProcesses', 'cpuTotal', []).size).toBe('l');
    expect(newWidget('bigNumber', 'swap', []).size).toBe('s');
    expect(a.thresholds).toEqual({ warning: null, critical: null });
  });

  it('moves, resizes and removes without changing the input', () => {
    const list = ['a', 'b', 'c'];
    expect(moveWidget(list, 0, 2)).toEqual(['b', 'c', 'a']);
    expect(moveWidget(list, 2, 0)).toEqual(['c', 'a', 'b']);
    expect(moveWidget(list, 9, 0)).toEqual(list);
    expect(moveWidget(list, 0, 99)).toEqual(['b', 'c', 'a']);
    expect(list).toEqual(['a', 'b', 'c']);
    const ws = layoutFromPreset('overview').widgets;
    expect(resizeWidget(ws, 'cpu', 'l').find((w) => w.id === 'cpu')!.size).toBe('l');
    expect(removeWidget(ws, 'cpu').some((w) => w.id === 'cpu')).toBe(false);
  });

  it('applies a preset without touching the others and ignores an unknown id', () => {
    const layout = layoutFromPreset('overview');
    const dev = applyPreset(layout, 'developer');
    expect(dev.widgets.map((w) => w.id)).toContain('cpu-cores');
    expect(dev.presets).toEqual(layout.presets);
    expect(applyPreset(layout, 'nope')).toBe(layout);
    expect(builtinPresets().map((p) => p.id)).toEqual(['overview', 'developer', 'gaming', 'battery']);
  });

  it('colours by threshold only when crossed', () => {
    expect(thresholdTone(50, { warning: 80, critical: 92 })).toBe('ok');
    expect(thresholdTone(85, { warning: 80, critical: 92 })).toBe('warn');
    expect(thresholdTone(95, { warning: 80, critical: 92 })).toBe('bad');
    expect(thresholdTone(95, { warning: null, critical: null })).toBe('ok');
    expect(thresholdTone(null, { warning: 1, critical: 2 })).toBe('ok');
  });

  it('reads metrics from a live sample, with reasons when unavailable', () => {
    const s = sample();
    expect(readMetric('cpuTotal', s).percent).toBeCloseTo(s.cpu.total);
    expect(readMetric('memory', s).text[1]).toBe('%');
    expect(readMetric('diskActivity', s).text[1]).toMatch(/\/s$/);
    expect(readMetric('networkDown', s).value).toBeGreaterThan(0);
    expect(readMetric('temperature', s).text[1]).toBe('°C');
    expect(readMetric('cpuPerCore', s).value).not.toBeNull();
    expect(readMetric('memoryPressure', s).text[0]).toBe('Normal');
    expect(readMetric('cpuTotal', null).reason).toMatch(/Waiting/);
    expect(readMetric({ perfCounter: 'x' }, s).reason).toMatch(/not available/);
    const none: SystemSample = { ...s, gpu: { unavailable: 'No GPU.' }, battery: { unavailable: 'No battery.' }, temperatures: { unavailable: 'No sensors.' }, fans: { unavailable: 'No fans.' }, disks: [] };
    expect(readMetric('gpuLoad', none).reason).toBe('No GPU.');
    expect(readMetric('battery', none).reason).toBe('No battery.');
    expect(readMetric('temperature', none).reason).toBe('No sensors.');
    expect(readMetric('fan', none).reason).toBe('No fans.');
    expect(readMetric('diskFree', none).reason).toMatch(/No drive/);
    expect(readMetric('swap', { ...s, memory: { ...s.memory, swapTotal: 0 } }).reason).toMatch(/No swap/);
    expect(readMetric('fan', sample()).value ?? null).toBeNull();
  });

  it('maps metrics to live series where one is kept', () => {
    const series = { cpu: [1], memory: [2], diskRead: [3], diskWrite: [4], netRx: [5], netTx: [6], gpu: [7] };
    expect(seriesFor('cpuTotal', series)!.values).toEqual([1]);
    expect(seriesFor('diskActivity', series)!.values2).toEqual([4]);
    expect(seriesFor('networkUp', series)!.values).toEqual([6]);
    expect(seriesFor('fan', series)).toBeNull();
  });

  it('labels metrics and handles tray item styles', () => {
    expect(metricLabel('cpuTotal')).toBe('CPU');
    expect(metricLabel({ process: 'node' })).toBe('Process node');
    expect(metricLabel({ perfCounter: 'x' })).toBe('Counter x');
    expect(trayStyleId({ id: 'a', metric: 'cpuTotal', style: { number: { unit: '%', label: 'C' } } })).toBe('number');
    expect(styleFromId('ring')).toBe('ring');
    expect(styleFromId('number', '%', 'CPU')).toEqual({ number: { unit: '%', label: 'CPU' } });
    const items = [newTrayItem('cpuTotal', 'lineGraph', [])];
    expect(newTrayItem('cpuTotal', 'ring', items).id).not.toBe(items[0]!.id);
  });
});

describe('sentenceCase', () => {
  it('lower-cases a label inside a sentence but keeps an acronym', () => {
    expect(sentenceCase('Memory')).toBe('memory');
    expect(sentenceCase('CPU')).toBe('CPU');
    expect(sentenceCase('GPU load')).toBe('GPU load');
    expect(sentenceCase('Network down')).toBe('network down');
    expect(sentenceCase('')).toBe('');
  });
});
