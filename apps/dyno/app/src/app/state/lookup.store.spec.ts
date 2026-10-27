import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../core/backend';
import { MockBackend } from '../core/mock-backend';
import { LookupStore } from './lookup.store';

function setup() {
  const mock = new MockBackend({ latencyMs: 0, now: () => 1_700_000_000_000 });
  TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: mock }] });
  return { mock, store: TestBed.inject(LookupStore) };
}

describe('LookupStore', () => {
  it('loads the redacted starting text of a search', async () => {
    const { store } = setup();
    await store.loadSeed('Slack for alex at 10.0.0.5');
    expect(store.seed()).toEqual({ text: 'Slack for at', removed: ['1 user name', '1 IP address'] });
    expect(store.busy()).toBe(false);
    expect(store.error()).toBeNull();
  });

  it('opens a search for exactly the confirmed text, and reports a refusal', async () => {
    const { store, mock } = setup();
    expect(await store.search('slack high cpu')).toBe(true);
    expect(mock.opened).toEqual(['https://duckduckgo.com/?q=slack+high+cpu']);
    expect(await store.search('   ')).toBe(false);
    expect(store.error()?.code).toBe('badRequest');
    expect(mock.opened).toHaveLength(1);
  });

  it('computes a hash and opens VirusTotal with only the hash', async () => {
    const { store, mock } = setup();
    await store.loadHash({ path: '/Applications/Slack.app/slack' });
    const h = store.hash()!;
    expect(h.sha256).toMatch(/^[0-9a-f]{64}$/);
    expect(await store.openVirusTotal(h.sha256)).toBe(true);
    expect(mock.opened).toEqual([`https://www.virustotal.com/gui/file/${h.sha256}`]);
    expect(await store.openVirusTotal('nope')).toBe(false);
    expect(store.error()?.message).toContain('64 hexadecimal');
  });

  it('a failed hash keeps no stale result, and reset clears everything', async () => {
    const { store } = setup();
    await store.loadHash({ path: '/a/b' });
    expect(store.hash()).not.toBeNull();
    await store.loadHash({ path: 'relative' });
    expect(store.hash()).toBeNull();
    expect(store.error()?.code).toBe('notFound');
    store.reset();
    expect(store.error()).toBeNull();
    expect(store.seed()).toBeNull();
  });
});
