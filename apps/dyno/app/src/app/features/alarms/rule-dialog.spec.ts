import { TestBed } from '@angular/core/testing';
import { renderFramed } from '@decryp7/paddock-ui/shell/testing';
import type { CustomAlertRule } from '../../core/api.types';
import { BACKEND } from '../../core/backend';
import { MockBackend } from '../../core/mock-backend';
import { provideIcons } from '../../shared/icons';
import { RuleDialog } from './rule-dialog';

const GPU: CustomAlertRule = { id: 'gpu-load', name: 'GPU load', metric: 'gpuLoad', op: 'above', limit: 90, forSecs: 120, severity: 'warning', enabled: true };

async function open(inputs: Record<string, unknown>, backend = new MockBackend({ latencyMs: 0, now: () => 1_700_000_000_000 })) {
  const d = await renderFramed(RuleDialog, { id: 'dy.alert-rule', kind: 'form', modality: 'modal', inputs, providers: [provideIcons(), { provide: BACKEND, useValue: backend }] });
  const settle = async () => {
    for (let i = 0; i < 3; i++) {
      await d.stable();
      await new Promise((r) => setTimeout(r));
    }
  };
  await settle();
  const set = async (sel: string, v: string) => {
    const i = d.el.querySelector<HTMLInputElement>(sel)!;
    i.value = v;
    i.dispatchEvent(new Event('input'));
    await settle();
  };
  const click = async (label: string) => {
    [...d.el.querySelectorAll<HTMLButtonElement>('button')].find((b) => b.textContent?.trim() === label || b.getAttribute('aria-label') === label)!.click();
    await settle();
  };
  return { d, backend, settle, set, click };
}
const stored = async (b: MockBackend) => (await b.settings()).customAlerts ?? [];

describe('RuleDialog (dy.alert-rule)', () => {
  it('has Cancel and one affirmative that says what it does', async () => {
    const add = await open({ rule: null, takenIds: [] });
    expect(add.d.actions().sort()).toEqual(['cancel', 'save']);
    expect(add.d.action('save')!.textContent).toContain('Add rule');
    TestBed.resetTestingModule();
    const edit = await open({ rule: GPU, takenIds: [] });
    expect(edit.d.action('save')!.textContent).toContain('Save');
  });

  it('adds a rule with a generated unique id and answers true', async () => {
    const { d, backend, set, click, settle } = await open({ rule: null, takenIds: ['gpu-load'] });
    await set('[aria-label="Name"]', 'GPU load');
    await click('Critical');
    await click('Below');
    await click('Increase the limit');
    await click('min');
    await d.press('save');
    await settle();
    const rules = await stored(backend);
    expect(rules.at(-1)).toMatchObject({ id: 'gpu-load-2', name: 'GPU load', op: 'below', severity: 'critical', limit: 95, forSecs: 120 });
    expect(d.outcome()).toEqual({ kind: 'answered', value: true });
  });

  it('validates first and shows a refusal in the frame without closing; Try again stores it', async () => {
    const { d, backend, set, settle } = await open({ rule: null, takenIds: [] });
    await d.press('save');
    await settle();
    expect(d.el.textContent).toContain('Give the rule a name.');
    expect(d.outcome()).toBeNull();
    await set('[aria-label="Name"]', 'Hot');
    await set('[aria-label="Limit"]', 'abc');
    await d.press('save');
    await settle();
    expect(d.el.textContent).toContain('The limit must be a number.');
    await set('[aria-label="Limit"]', '85');
    await set('[aria-label="Duration"]', '2.5');
    await d.press('save');
    await settle();
    expect(d.el.textContent).toContain('whole number');
    await set('[aria-label="Duration"]', '10');
    vi.spyOn(backend, 'upsertAlertRule').mockRejectedValueOnce({ code: 'badRequest', message: 'Limit must be 0 to 100.' });
    await d.press('save');
    await settle();
    expect(d.el.textContent).toContain('Limit must be 0 to 100.');
    expect(d.outcome()).toBeNull();
    await d.press('save');
    await settle();
    expect(d.outcome()).toEqual({ kind: 'answered', value: true });
  });

  it('edits a rule keeping its id, showing minutes when divisible, and converts units', async () => {
    const { d, backend, set, click, settle } = await open({ rule: GPU, takenIds: [] });
    expect(d.el.querySelector<HTMLInputElement>('[aria-label="Duration"]')!.value).toBe('2');
    await click('sec');
    expect(d.el.querySelector<HTMLInputElement>('[aria-label="Duration"]')!.value).toBe('120');
    await click('Decrease the limit');
    await set('[aria-label="Name"]', 'GPU hot');
    await d.press('save');
    await settle();
    expect((await stored(backend)).find((r) => r.id === 'gpu-load')).toMatchObject({ name: 'GPU hot', limit: 85, forSecs: 120 });
  });

  it('steps temperatures by one degree within bounds', async () => {
    const { d, set, click, settle } = await open({ rule: { ...GPU, metric: 'temperature', limit: 80 }, takenIds: [] });
    expect(d.el.querySelector('.tools')!.textContent).toContain('°C');
    await set('[aria-label="Limit"]', '150');
    await click('Increase the limit');
    await settle();
    expect(d.el.querySelector<HTMLInputElement>('[aria-label="Limit"]')!.value).toBe('150');
    await set('[aria-label="Limit"]', 'x');
    await click('Decrease the limit');
    expect(d.el.querySelector<HTMLInputElement>('[aria-label="Limit"]')!.value).toBe('-1');
  });
});
