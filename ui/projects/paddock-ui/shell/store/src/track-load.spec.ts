import { ApplicationRef, provideZonelessChangeDetection } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { describe, expect, it } from 'vitest';
import { injectTrackLoad } from './track-load';

describe('injectTrackLoad', () => {
  it('keeps the app unstable until the work ends, and passes its result and its failure through', async () => {
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection()] });
    const track = TestBed.runInInjectionContext(() => injectTrackLoad());
    let release!: (v: number) => void;
    const result = track(() => new Promise<number>((r) => (release = r)));
    let stable = false;
    void TestBed.inject(ApplicationRef).whenStable().then(() => (stable = true));
    await Promise.resolve();
    await new Promise<void>((r) => setTimeout(r, 0));
    expect(stable).toBe(false);
    release(7);
    expect(await result).toBe(7);
    await TestBed.inject(ApplicationRef).whenStable();
    expect(stable).toBe(true);
    await expect(track(() => Promise.reject(new Error('gone')))).rejects.toThrow('gone');
    await TestBed.inject(ApplicationRef).whenStable();
  });
});
