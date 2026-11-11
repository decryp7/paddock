import { TestBed } from '@angular/core/testing';
import { describe, expect, it } from 'vitest';
import { BACKEND } from '../../core/backend';
import { MockBackend } from '../../core/mock-backend';
import { ChatImagesState, PitwallAttachments, sizeText } from './chat-attachments';

/** A PNG's signature and header for a `w` x `h` picture (the mock reads only that). */
const pngHeader = (w: number, h: number): Uint8Array => {
  const b = new Uint8Array(33);
  b.set([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13, 0x49, 0x48, 0x44, 0x52]);
  const v = new DataView(b.buffer);
  v.setUint32(16, w);
  v.setUint32(20, h);
  return b;
};

const source = (name: string, bytes: Uint8Array) => ({ name, type: '', size: bytes.length, bytes: () => Promise.resolve(bytes) });

describe('PitwallAttachments (ADR 0023 WP-4b)', () => {
  const port = (): PitwallAttachments => {
    TestBed.resetTestingModule();
    TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: new MockBackend({ latencyMs: 0 }) }, ChatImagesState, PitwallAttachments] });
    return TestBed.inject(PitwallAttachments);
  };

  it("reads Rust's rules and names a taken file's chip with its size and lines", async () => {
    const p = port();
    expect((await p.rules()).maxFileBytes).toBe(256 * 1024);
    const chip = await p.add(source('notes.md', new TextEncoder().encode('one\ntwo\n')));
    expect(chip).toMatchObject({ label: 'notes.md', detail: '8 B · 2 lines', attachment: { kind: 'file', name: 'notes.md' } });
  });

  it("passes Rust's refusal on in its own words", async () => {
    await expect(port().add(source('data.bin', new Uint8Array([0xc3, 0x28])))).rejects.toThrow("isn't a text file");
  });

  describe('pictures (WP-5)', () => {
    const withModel = async (model: string | null) => {
      const mock = new MockBackend({ latencyMs: 0 });
      TestBed.resetTestingModule();
      TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: mock }, ChatImagesState, PitwallAttachments] });
      const picker = model ? await mock.setChatModel(model) : await mock.chatModels();
      TestBed.inject(ChatImagesState).images.set(picker.images);
      return { mock, p: TestBed.inject(PitwallAttachments), picker };
    };

    it("takes a picture for a model that reads pictures: a chip with its size in pixels, its name and Rust's thumbnail", async () => {
      const { p } = await withModel('gemma-4-e4b-it-q4_k_m');
      expect((await p.rules()).maxImageBytes).toBe(25 * 1024 * 1024);
      const chip = await p.add(source('cat.png', pngHeader(640, 480)));
      expect(chip).toMatchObject({ label: 'cat.png', detail: '640 × 480 · 33 B', attachment: { kind: 'image', width: 640, height: 480, mime: 'image/png' } });
      expect(chip.thumbnail).toMatch(/^data:image\/png;base64,/);
      expect(await p.thumbnail(chip.attachment)).toBe(chip.thumbnail);
      expect(await p.thumbnail({ kind: 'image', sha256: 'f'.repeat(64), mime: 'image/png', width: 1, height: 1 })).toBeNull();
      expect(p.imageBlock()).toBeNull();
    });

    it("refuses a picture for a model without vision with Rust's words, which also say what to choose", async () => {
      const { p, picker } = await withModel('qwen3.5-4b-q4_k_m');
      expect(picker.images).toMatchObject({ allowed: false, suggest: { id: 'gemma-4-e4b-it-q4_k_m', name: 'Gemma 4 E4B' } });
      const words = "Qwen3.5 4B can't read images. Choose Gemma 4 E4B to ask about a picture.";
      expect(p.imageBlock()).toBe(words);
      await expect(p.add(source('cat.png', pngHeader(2, 2)))).rejects.toThrow(words);
      // Text still goes.
      expect((await p.add(source('a.txt', new TextEncoder().encode('hi')))).attachment.kind).toBe('file');
    });

    it('has no block before the models are known, and names a picture type the chat cannot read', async () => {
      TestBed.resetTestingModule();
      TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: new MockBackend({ latencyMs: 0 }) }, ChatImagesState, PitwallAttachments] });
      expect(TestBed.inject(PitwallAttachments).imageBlock()).toBeNull();
      const { p } = await withModel('gemma-4-e4b-it-q4_k_m');
      await expect(p.add(source('scan.tif', new Uint8Array([0x49, 0x49, 0x2a, 0x00, 1, 2])))).rejects.toThrow("is a picture type the chat can't read");
    });
  });

  it('tells Rust to forget a file taken off the question (review P2-1)', async () => {
    const mock = new MockBackend({ latencyMs: 0 });
    TestBed.resetTestingModule();
    TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: mock }, ChatImagesState, PitwallAttachments] });
    await TestBed.inject(PitwallAttachments).remove({ kind: 'file', sha256: 'ab'.repeat(32), name: 'a.txt' });
    expect(mock.detached).toEqual(['ab'.repeat(32)]);
  });

  it('writes sizes as people read them', () => {
    expect([sizeText(8), sizeText(12_400), sizeText(1_250_000)]).toEqual(['8 B', '12 KB', '1.3 MB']);
  });
});
