import { SizeLoop } from './size-loop';
import type { DialogMeasure } from './io';

const measure = (height: number, minHeight = 100): DialogMeasure => ({ content: { width: 400, height }, minContent: { width: 320, height: minHeight }, overflow: false });

describe('SizeLoop (ADR 0043 §2)', () => {
  function loop() {
    let current = measure(200);
    const sent: { epoch: number; seq: number; height: number }[] = [];
    const warnings: string[] = [];
    const l = new SizeLoop(
      7,
      () => current,
      async (epoch, seq, m) => void sent.push({ epoch, seq, height: m.content.height }),
      (w) => warnings.push(w),
    );
    return { l, sent, warnings, set: (m: DialogMeasure) => (current = m) };
  }

  it('sends a measure only when it differs from the last one sent, numbered within its epoch', () => {
    const { l, sent, set } = loop();
    expect(l.run()).toBe(1);
    expect(l.run()).toBeNull();
    set(measure(260));
    expect(l.run()).toBe(2);
    set(measure(260, 120));
    expect(l.run()).toBe(3);
    expect(sent).toEqual([
      { epoch: 7, seq: 1, height: 200 },
      { epoch: 7, seq: 2, height: 260 },
      { epoch: 7, seq: 3, height: 260 },
    ]);
    expect(l.seq).toBe(3);
  });

  it('allows one follow-up after a change, and stops a second as a loop until the next trigger', () => {
    const { l, sent, warnings, set } = loop();
    l.run();
    set(measure(300));
    expect(l.run('trigger')).toBe(2);
    set(measure(302));
    expect(l.run('follow-up')).toBe(3);
    set(measure(300));
    expect(l.run('follow-up')).toBeNull();
    expect(l.stopped).toBe(true);
    expect(warnings).toHaveLength(1);
    expect(warnings[0]).toContain('did not settle');
    expect(warnings[0]).toContain('400x302/320x100');
    expect(sent).toHaveLength(3);
    // The next trigger starts over.
    expect(l.run('trigger')).toBe(4);
    expect(l.stopped).toBe(false);
  });

  it('tells the sender after every send and survives a sender that fails', async () => {
    const told: number[] = [];
    const l = new SizeLoop(
      1,
      () => measure(210),
      () => Promise.reject(new Error('no window')),
      undefined,
      (seq) => told.push(seq),
    );
    expect(l.run()).toBe(1);
    await Promise.resolve();
    expect(told).toEqual([1]);
  });

  it('keeps only the last three measures for the warning and warns on the console by default', () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    let h = 200;
    const l = new SizeLoop(1, () => measure(h), async () => undefined);
    for (const next of [201, 202, 203]) {
      h = next;
      l.run('trigger');
    }
    h = 204;
    l.run('follow-up');
    h = 205;
    l.run('follow-up');
    expect(warn).toHaveBeenCalledTimes(1);
    expect(String(warn.mock.calls[0]![0]).split(' | ')).toHaveLength(3);
    warn.mockRestore();
  });
});
