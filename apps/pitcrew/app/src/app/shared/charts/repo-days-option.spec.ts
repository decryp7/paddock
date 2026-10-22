import type { ChartTokens } from '../../core/theme.service';
import { repoDaysOption } from './repo-days-option';

describe('repoDaysOption', () => {
  it('stacks succeeded under failed jobs per day without axes', () => {
    const t = { success: '#0a0', danger: '#a00' } as ChartTokens;
    const o = repoDaysOption(
      [
        { start: 1, success: 4, failure: 1 },
        { start: 2, success: 0, failure: 3 },
      ],
      t,
    ) as { series: { data: number[]; itemStyle: { color: string } }[]; xAxis: { show: boolean } };
    expect(o.series.map((s) => s.data)).toEqual([
      [4, 0],
      [1, 3],
    ]);
    expect(o.series.map((s) => s.itemStyle.color)).toEqual(['#0a0', '#a00']);
    expect(o.xAxis.show).toBe(false);
  });
});
