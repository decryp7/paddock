import { MockBackend } from '../../core/mock-backend';
import { FakeChart, button, render, settle } from '../../../testing/render';
import { BenchmarksPage } from './benchmarks-page';

const NOW = 1_800_000_000_000;
const wait = (ms = 0) => new Promise((r) => setTimeout(r, ms));

describe('BenchmarksPage', () => {
  beforeEach(() => (FakeChart.all = []));
  const mock = (o = {}) => new MockBackend({ now: () => NOW, ...o });

  it('lists runnable models with their last run, and the results table newest first', async () => {
    const { el } = await render(BenchmarksPage, mock());
    const items = Array.from(el.querySelectorAll('ul[aria-label="Models to benchmark"] li'));
    expect(items.length).toBeGreaterThanOrEqual(3);
    const qwen = items.find((li) => li.textContent?.includes('Qwen3 8B Instruct'))!;
    expect(qwen.textContent).toContain('last run');
    expect(qwen.textContent).toContain('61.8');
    expect(items.some((li) => li.textContent?.includes('never run'))).toBe(false);
    expect(items.some((li) => li.textContent?.includes('nomic'))).toBe(false);
    const rows = el.querySelectorAll('tbody tr');
    expect(rows).toHaveLength(5);
    expect(rows[0]!.textContent).toContain('Qwen3 8B Instruct');
    expect(el.querySelector('.panel, pw-panel[heading="Results"]')?.textContent).toContain('5 runs');
  });

  it('draws the speed chart with one row per model and build', async () => {
    const { fixture, el } = await render(BenchmarksPage, mock());
    await wait(40);
    await settle(fixture);
    expect(el.querySelector('pw-echart')).not.toBeNull();
    expect(el.querySelector('pw-echart')!.getAttribute('aria-label')).toContain('tokens per second');
    const chart = FakeChart.all.at(-1)!;
    const option = chart.options.at(-1)!.option as any;
    expect(option.series[0].data).toHaveLength(5);
    expect(option.yAxis[0].data.at(-1)).toContain('Qwen2.5 Coder 1.5B');
    expect(el.querySelector('pw-echart')!.getAttribute('style')).toContain('264px');
  });

  it('shows empty states when nothing was measured and models say never run', async () => {
    const be = mock({ empty: true });
    be.localModels = async () => [{ id: 'm1', name: 'Model One', quant: null, paramsB: null, capabilities: ['chat'] } as never];
    const { el } = await render(BenchmarksPage, be);
    expect(el.textContent).toContain('No results yet');
    expect(el.textContent).toContain('No runs yet');
    expect(el.textContent).toContain('never run');
    expect(el.textContent).toContain('GGUF');
    expect(el.querySelector('pw-echart')).toBeNull();
  });

  it('says there are no chat models to benchmark', async () => {
    const be = mock({ empty: true });
    be.localModels = async () => [];
    const { el } = await render(BenchmarksPage, be);
    expect(el.textContent).toContain('No chat models');
  });

  it('shows the load error in each panel', async () => {
    const { el } = await render(BenchmarksPage, mock({ failing: ['benchmarks'] }));
    expect(el.textContent).toContain('not responding');
    expect(el.querySelector('pw-echart')).toBeNull();
  });

  it('runs a benchmark from the list: disables the button while running, then shows the new result', async () => {
    const { fixture, el } = await render(BenchmarksPage, mock({ latencyMs: 15 }));
    await wait(80);
    await settle(fixture);
    const run = el.querySelector<HTMLButtonElement>('button[aria-label="Run benchmark for Qwen3 8B Instruct"]')!;
    run.click();
    fixture.detectChanges();
    expect(run.disabled).toBe(true);
    expect(run.textContent).toContain('Running');
    await wait(60);
    await settle(fixture);
    expect(run.disabled).toBe(false);
    expect(run.textContent).toContain('Run');
    expect(el.querySelectorAll('tbody tr')).toHaveLength(6);
    expect(el.querySelector('tbody tr')!.textContent).toContain('Qwen3 8B Instruct');
  });

  it('retries loading from the error state', async () => {
    const { fixture, el } = await render(BenchmarksPage, mock({ failing: ['benchmarks'] }));
    const retry = Array.from(el.querySelectorAll('button')).find((b) => /retry/i.test(b.textContent ?? ''));
    expect(retry).toBeTruthy();
    retry!.click();
    await settle(fixture);
    expect(button(el, 'Retry')).toBeTruthy();
  });
});
