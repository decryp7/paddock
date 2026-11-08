import { TestBed } from '@angular/core/testing';
import { describe, expect, it } from 'vitest';
import { BACKEND } from '../../core/backend';
import { MockBackend } from '../../core/mock-backend';
import { PitwallAttachments, sizeText } from './chat-attachments';

const source = (name: string, bytes: Uint8Array) => ({ name, type: '', size: bytes.length, bytes: () => Promise.resolve(bytes) });

describe('PitwallAttachments (ADR 0023 WP-4b)', () => {
  const port = (): PitwallAttachments => {
    TestBed.resetTestingModule();
    TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: new MockBackend({ latencyMs: 0 }) }, PitwallAttachments] });
    return TestBed.inject(PitwallAttachments);
  };

  it("reads Rust's rules and names a taken file's chip with its size and lines", async () => {
    const p = port();
    expect((await p.rules()).maxFileBytes).toBe(256 * 1024);
    const chip = await p.add(source('notes.md', new TextEncoder().encode('one\ntwo\n')));
    expect(chip).toMatchObject({ label: 'notes.md', detail: '8 B · 2 lines', attachment: { kind: 'file', name: 'notes.md' } });
  });

  it("passes Rust's refusal on in its own words", async () => {
    await expect(port().add(source('photo.png', new Uint8Array([0x89, 0x50, 0x4e, 0x47])))).rejects.toThrow('is an image');
  });

  it('writes sizes as people read them', () => {
    expect([sizeText(8), sizeText(12_400), sizeText(1_250_000)]).toEqual(['8 B', '12 KB', '1.3 MB']);
  });
});
