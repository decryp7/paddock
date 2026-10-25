import { TestBed } from '@angular/core/testing';
import { describe, expect, it } from 'vitest';
import type { ViewDoc } from '../generated/view';
import { sampleViewDoc } from './sample';
import { ViewStore } from './view.store';

function store() {
  TestBed.configureTestingModule({ providers: [ViewStore] });
  return TestBed.inject(ViewStore);
}
const replace = (baseEtag: number, etag: number) => ({ replace: { baseEtag, etag, id: 'cap', node: { id: 'cap', kind: { text: { text: 'now', tone: 'neutral', style: 'caption' } } } } });

describe('ViewStore', () => {
  it('starts idle with nothing', () => {
    const s = store();
    expect(s.requestStatus()).toBe('idle');
    expect(s.doc()).toBeNull();
    expect(s.requestError()).toBeNull();
  });

  it('shows a valid document', () => {
    const s = store();
    s.show(sampleViewDoc());
    expect(s.isLoaded()).toBe(true);
    expect(s.doc()?.title).toBe('Release readiness');
    expect(s.failure()).toBeNull();
  });

  it('shows no document and an error for an invalid one, and recovers with a valid one', () => {
    const s = store();
    s.show(sampleViewDoc());
    s.show({ ...sampleViewDoc(), schema: 9 });
    expect(s.doc()).toBeNull();
    expect(s.requestError()).toContain('Update the app');
    expect(s.failure()?.kind).toBe('invalid');
    expect(s.isStale()).toBe(false);
    s.show(sampleViewDoc(2));
    expect(s.requestError()).toBeNull();
    expect(s.doc()?.etag).toBe(2);
  });

  it('applies a patch and moves to the new etag', () => {
    const s = store();
    s.show(sampleViewDoc());
    const r = s.applyPatch(replace(1, 2));
    expect(r.ok).toBe(true);
    expect(s.doc()?.etag).toBe(2);
  });

  it('keeps the last good document on a stale patch and says the view is out of date', () => {
    const s = store();
    s.show(sampleViewDoc(5));
    const r = s.applyPatch(replace(4, 6));
    expect(r.ok).toBe(false);
    expect(s.doc()?.etag).toBe(5);
    expect(s.isStale()).toBe(true);
    expect(s.requestError()).toContain('Reload');
    // A fresh document clears it.
    s.show(sampleViewDoc(6));
    expect(s.isStale()).toBe(false);
    expect(s.requestError()).toBeNull();
  });

  it('refuses a patch before there is a document', () => {
    const s = store();
    const r = s.applyPatch(replace(1, 2));
    expect(!r.ok && r.problem.kind).toBe('malformed');
    expect(s.requestError()).toContain('no view');
  });

  it('shows loading while the host fetches and keeps the document', () => {
    const s = store();
    s.show(sampleViewDoc());
    s.begin();
    expect(s.isLoading()).toBe(true);
    expect(s.doc()).not.toBeNull();
  });

  it('resets', () => {
    const s = store();
    s.show(sampleViewDoc());
    s.reset();
    expect(s.doc()).toBeNull();
    expect(s.requestStatus()).toBe('idle');
    const d: ViewDoc = sampleViewDoc();
    expect(d.schema).toBe(1);
  });
});
