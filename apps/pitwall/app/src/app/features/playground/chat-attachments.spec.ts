import { TestBed } from '@angular/core/testing';
import { describe, expect, it } from 'vitest';
import { BACKEND } from '../../core/backend';
import { MockBackend } from '../../core/mock-backend';
import { attachedDetail, PitwallAttachments, sizeText } from './chat-attachments';

const source = (name: string, bytes: Uint8Array) => ({ name, type: '', size: bytes.length, bytes: () => Promise.resolve(bytes) });

describe('PitwallAttachments (ADR 0023 WP-4b, WP-5)', () => {
  const port = (): PitwallAttachments => {
    TestBed.resetTestingModule();
    TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: new MockBackend({ latencyMs: 0 }) }, PitwallAttachments] });
    return TestBed.inject(PitwallAttachments);
  };

  it("reads Rust's rules and names a taken file's chip with its size and lines", async () => {
    const p = port();
    expect((await p.rules()).maxFileBytes).toBe(25 * 1024 * 1024);
    const chip = await p.add(source('notes.md', new TextEncoder().encode('one\ntwo\n')));
    expect(chip).toMatchObject({ label: 'notes.md', detail: '8 B · 2 lines', attachment: { kind: 'file', name: 'notes.md' } });
  });

  it("names an image's chip with the file's name and the size Rust kept (ADR 0023 WP-5)", async () => {
    // A PNG header claiming 4000 × 3000: the browser build reads the size from it and fits it to the default budget.
    const png = new Uint8Array(33);
    png.set([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13, 0x49, 0x48, 0x44, 0x52]);
    new DataView(png.buffer).setUint32(16, 4000);
    new DataView(png.buffer).setUint32(20, 3000);
    const chip = await port().add(source('photo.png', png));
    expect(chip.label).toBe('photo.png');
    expect(chip.attachment).toMatchObject({ kind: 'image', width: 1034, height: 775 });
    expect(chip.detail).toMatch(/^1034 × 775 · \d+ B$/);
  });

  it("passes Rust's refusal on in its own words", async () => {
    await expect(port().add(source('photo.png', new Uint8Array([0x89, 0x50, 0x4e, 0x47])))).rejects.toThrow("can't be read as an image");
  });

  it('writes an image chip\'s detail with its pixels and a text chip\'s with its lines', () => {
    const image = { kind: 'image' as const, sha256: 'a'.repeat(64), mime: 'image/jpeg', width: 1280, height: 720 };
    expect(attachedDetail({ attachment: image, name: 'a.jpg', bytes: 240_000, lines: 0, thumbnail: null, original: null })).toBe('1280 × 720 · 240 KB');
    const file = { kind: 'file' as const, sha256: 'b'.repeat(64), name: 'a.txt' };
    expect(attachedDetail({ attachment: file, name: 'a.txt', bytes: 12, lines: 1, thumbnail: null, original: null })).toBe('12 B · 1 line');
  });

  it('tells Rust to forget a file taken off the question (review P2-1)', async () => {
    const mock = new MockBackend({ latencyMs: 0 });
    TestBed.resetTestingModule();
    TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: mock }, PitwallAttachments] });
    await TestBed.inject(PitwallAttachments).remove({ kind: 'file', sha256: 'ab'.repeat(32), name: 'a.txt' });
    expect(mock.detached).toEqual(['ab'.repeat(32)]);
  });

  it('writes sizes as people read them', () => {
    expect([sizeText(8), sizeText(12_400), sizeText(1_250_000)]).toEqual(['8 B', '12 KB', '1.3 MB']);
  });
});
