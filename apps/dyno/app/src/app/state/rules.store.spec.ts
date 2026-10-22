import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../core/backend';
import { MockBackend } from '../core/mock-backend';
import { Notifier } from '../shared/notifier';
import { ActivityStore } from './activity.store';
import { RulesStore } from './rules.store';

const NOW = 1_700_000_000_000;

function setup() {
  const mock = new MockBackend({ latencyMs: 0, now: () => NOW });
  TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: mock }] });
  return { mock, store: TestBed.inject(RulesStore), notifier: TestBed.inject(Notifier), activity: TestBed.inject(ActivityStore) };
}
const last = (n: Notifier) => n.toasts().at(-1);

describe('RulesStore', () => {
  it('has safe defaults before loading', () => {
    const { store } = setup();
    expect(store.loaded()).toBe(false);
    expect(store.profile()).toBe('normal');
    expect(store.paused()).toBe(false);
    expect(store.profiles()).toEqual([]);
    expect(store.activeProfile()).toBeNull();
  });
  it('load fills the rule set and computed values', async () => {
    const { store } = setup();
    await store.load();
    expect(store.loaded()).toBe(true);
    expect(store.profiles().length).toBeGreaterThan(1);
    expect(store.activeProfile()?.id).toBe('normal');
  });
  it('load failure records the error', async () => {
    const { store, mock } = setup();
    vi.spyOn(mock, 'rules').mockRejectedValue({ code: 'busy', message: 'b' });
    await store.load();
    expect(store.error()).toEqual({ code: 'busy', message: 'b' });
    expect(store.loading()).toBe(false);
    expect(store.loaded()).toBe(false);
  });
  it('ensure loads once', async () => {
    const { store, mock } = setup();
    const spy = vi.spyOn(mock, 'rules');
    await store.ensure();
    await store.ensure();
    expect(spy).toHaveBeenCalledTimes(1);
  });
  it('setProfile switches and announces it', async () => {
    const { store, notifier } = setup();
    await store.load();
    expect(await store.setProfile('gaming')).toBe(true);
    expect(store.profile()).toBe('gaming');
    expect(last(notifier)?.text).toMatch(/Gaming profile on/);
    expect(store.saving()).toBe(false);
  });
  it('setProfile failure toasts and keeps the state', async () => {
    const { store, mock, notifier } = setup();
    await store.load();
    vi.spyOn(mock, 'setProfile').mockRejectedValue({ code: 'badRequest', message: 'nope' });
    expect(await store.setProfile('gaming')).toBe(false);
    expect(store.profile()).toBe('normal');
    expect(store.saving()).toBe(false);
    expect(last(notifier)).toMatchObject({ tone: 'bad', text: 'nope' });
  });
  it('pause and resume', async () => {
    const { store, notifier } = setup();
    await store.load();
    await store.pause(true);
    expect(store.paused()).toBe(true);
    expect(last(notifier)?.text).toMatch(/paused/);
    await store.pause(false);
    expect(store.paused()).toBe(false);
    expect(last(notifier)?.text).toBe('Rules resumed');
  });
  it('rule mutations are refused until loaded', async () => {
    const { store } = setup();
    const rule = { id: 'r', name: 'r' } as never;
    expect(await store.saveRule(rule)).toBe(false);
    expect(await store.deleteRule('r')).toBe(false);
    expect(await store.toggleRule('r', true)).toBe(false);
    expect(await store.setProtected([])).toBe(false);
  });
  it('saves a changed rule and adds a new one', async () => {
    const { store, notifier } = setup();
    await store.load();
    const existing = structuredClone(store.ruleSet()!.rules[0]!);
    existing.name = 'Renamed';
    expect(await store.saveRule(existing)).toBe(true);
    expect(store.ruleSet()!.rules.find((r) => r.id === existing.id)?.name).toBe('Renamed');
    expect(last(notifier)?.text).toBe('Saved “Renamed”');
    const count = store.ruleSet()!.rules.length;
    expect(await store.saveRule({ ...existing, id: 'brand-new', name: 'Fresh' })).toBe(true);
    expect(store.ruleSet()!.rules).toHaveLength(count + 1);
    expect(last(notifier)?.text).toBe('Added “Fresh”');
  });
  it('rejects an invalid rule with the engine message', async () => {
    const { store, notifier } = setup();
    await store.load();
    const bad = { ...structuredClone(store.ruleSet()!.rules[0]!), name: '  ' };
    expect(await store.saveRule(bad)).toBe(false);
    expect(last(notifier)).toMatchObject({ tone: 'bad', text: 'Every rule needs a name.' });
  });
  it('deletes a rule', async () => {
    const { store, notifier } = setup();
    await store.load();
    const rule = store.ruleSet()!.rules[0]!;
    const count = store.ruleSet()!.rules.length;
    expect(await store.deleteRule(rule.id)).toBe(true);
    expect(store.ruleSet()!.rules).toHaveLength(count - 1);
    expect(last(notifier)?.text).toBe(`Deleted “${rule.name}”`);
  });
  it('deleting an unknown id still reports a generic name', async () => {
    const { store, notifier } = setup();
    await store.load();
    await store.deleteRule('ghost');
    expect(last(notifier)?.text).toBe('Deleted “rule”');
  });
  it('toggles a rule silently', async () => {
    const { store, notifier } = setup();
    await store.load();
    const rule = store.ruleSet()!.rules[0]!;
    expect(await store.toggleRule(rule.id, !rule.enabled)).toBe(true);
    expect(store.ruleSet()!.rules[0]!.enabled).toBe(!rule.enabled);
    expect(notifier.toasts()).toEqual([]);
  });
  it('saves the protected list', async () => {
    const { store, notifier } = setup();
    await store.load();
    expect(await store.setProtected(['Foo'])).toBe(true);
    expect(store.ruleSet()!.protected).toEqual(['Foo']);
    expect(last(notifier)?.text).toBe('Protected list saved');
  });
  it('release logs the entry and toasts', async () => {
    const { store, mock, activity, notifier } = setup();
    await store.load();
    const live = await mock.liveUpdate('summary');
    const target = live.activeActions[0]!.target;
    expect(await store.release(target)).toBe(true);
    expect(activity.entries()[0]?.title).toMatch(/Released/);
    expect(last(notifier)?.text).toMatch(/Released/);
  });
  it('release failure toasts the error', async () => {
    const { store, notifier } = setup();
    expect(await store.release({ pid: 1, started: 1 })).toBe(false);
    expect(last(notifier)).toMatchObject({ tone: 'bad', text: 'That action was already released.' });
  });
});
