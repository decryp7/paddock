import { ApplicationInitStatus, InjectionToken } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { describe, expect, it, vi } from 'vitest';
import { backendKind, chooseBackend, inTauri, provideBackend, type BackendEnv } from './backend-selection';

const env = (over: Partial<BackendEnv<string>> = {}): BackendEnv<string> => ({ inTauri: false, search: '?mock=a', allowMock: true, mockOptions: (s) => `options${s}`, ...over });

describe('backendKind', () => {
  it('is tauri inside Tauri, even where the mock is allowed', () => {
    expect(backendKind({ inTauri: true, allowMock: true })).toBe('tauri');
    expect(backendKind({ inTauri: true, allowMock: false })).toBe('tauri');
  });
  it('is the mock in a dev browser, and an error in a release browser', () => {
    expect(backendKind({ inTauri: false, allowMock: true })).toBe('mock');
    expect(() => backendKind({ inTauri: false, allowMock: false })).toThrow(/needs the desktop app/);
  });
});

describe('inTauri', () => {
  it('looks for the bridge on the given globals', () => {
    expect(inTauri({})).toBe(false);
    expect(inTauri({ __TAURI_INTERNALS__: {} })).toBe(true);
    expect(inTauri()).toBe(false);
  });
});

describe('chooseBackend', () => {
  it('makes only the backend it needs, the mock with the options from the URL', async () => {
    const tauri = vi.fn(() => 'real');
    const mock = vi.fn(async (o: string) => `mock ${o}`);
    expect(await chooseBackend({ tauri, mock }, env())).toEqual({ kind: 'mock', backend: 'mock options?mock=a' });
    expect(tauri).not.toHaveBeenCalled();
    expect(await chooseBackend({ tauri, mock }, env({ inTauri: true }))).toEqual({ kind: 'tauri', backend: 'real' });
    expect(mock).toHaveBeenCalledTimes(1);
  });
  it('refuses a release build outside Tauri without making either', async () => {
    const tauri = vi.fn(() => 'real');
    const mock = vi.fn(() => 'mock');
    await expect(chooseBackend({ tauri, mock }, env({ allowMock: false }))).rejects.toThrow(/needs the desktop app/);
    expect(tauri).not.toHaveBeenCalled();
    expect(mock).not.toHaveBeenCalled();
  });
});

describe('provideBackend', () => {
  const TOKEN = new InjectionToken<string>('backend');
  const providers = () => [provideBackend(TOKEN, { tauri: () => 'real', mock: async (o: string) => `mock ${o}` }, () => env())];

  it('provides the chosen backend once the app initializer has run', async () => {
    TestBed.configureTestingModule({ providers: providers() });
    await TestBed.inject(ApplicationInitStatus).donePromise;
    expect(TestBed.inject(TOKEN)).toBe('mock options?mock=a');
  });

  it('refuses an injection before the app finished starting', () => {
    TestBed.configureTestingModule({ providers: providers() });
    expect(() => TestBed.inject(TOKEN)).toThrow(/before the app finished starting/);
  });
});
