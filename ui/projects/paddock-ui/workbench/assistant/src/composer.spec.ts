import { TestBed } from '@angular/core/testing';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { provideShellTesting, render, settle } from '@decryp7/paddock-ui/shell/testing';
import { PdAssistant } from './assistant';
import { AssistantBackend, MemoryAssistantBackend, type MemoryAssistantOptions } from './backend';
import { AssistantAttachments, AssistantSpeech } from './composer-ports';
import { MemoryAssistantAttachments, MemoryAssistantSpeech } from './memory-ports';
import type { Attachment, ContextUse } from '../generated/assistant';

/** The composer of ADR 0023 WP-4b: attachments (picker, paste, drop), push-to-talk and the context meter. */

interface Options extends MemoryAssistantOptions {
  readonly attachments?: MemoryAssistantAttachments | null;
  readonly speech?: MemoryAssistantSpeech | null;
  readonly context?: ContextUse | null;
}

class MeasuredBackend extends MemoryAssistantBackend {
  used: ContextUse | null = null;
  readonly asked: (string | null)[] = [];
  /** The attachments each question about the context carried (the composer's chips). */
  readonly pending: (readonly Attachment[])[] = [];
  override context(id: string | null, attachments: readonly Attachment[] = []): Promise<ContextUse | null> {
    this.asked.push(id);
    this.pending.push(attachments);
    return this.used ? Promise.resolve(this.used) : super.context(id, attachments);
  }
}

const use = (usedTokens: number, limitTokens: number, level: ContextUse['level'], exact = true): ContextUse => ({ usedTokens, limitTokens, reserveTokens: 1024, exact, level });

async function open(options: Options = {}) {
  const backend = new MeasuredBackend(options);
  if (options.context !== undefined) backend.used = options.context;
  const attachments = options.attachments === undefined ? new MemoryAssistantAttachments() : options.attachments;
  const speech = options.speech === undefined ? new MemoryAssistantSpeech() : options.speech;
  TestBed.resetTestingModule();
  TestBed.configureTestingModule({
    providers: [
      ...provideShellTesting({
        context: () => Promise.resolve(() => ({})),
        providers: [
          { provide: AssistantBackend, useValue: backend },
          ...(attachments ? [{ provide: AssistantAttachments, useValue: attachments }] : []),
          ...(speech ? [{ provide: AssistantSpeech, useValue: speech }] : []),
        ],
      }),
    ],
  });
  const r = await render(PdAssistant, {});
  await settle(r.fixture);
  return { r, backend, attachments, speech };
}

type R = Awaited<ReturnType<typeof open>>['r'];

const settleAll = async (r: R): Promise<void> => {
  for (let i = 0; i < 6; i++) await settle(r.fixture);
};

const text = (r: R, testid: string): string => r.all(`[data-testid=${testid}]`).map((e) => e.textContent?.replace(/\s+/g, ' ').trim()).join('|');

const file = (name: string, body: string, type = 'text/plain'): File => new File([body], name, { type });

const pick = async (r: R, files: File[]): Promise<void> => {
  const input = r.one<HTMLInputElement>('[data-testid=assistant-file-input]');
  Object.defineProperty(input, 'files', { configurable: true, value: files });
  input.dispatchEvent(new Event('change'));
  await settleAll(r);
};

const type = async (r: R, value: string): Promise<HTMLTextAreaElement> => {
  const box = r.one<HTMLTextAreaElement>('[data-testid=assistant-input]');
  box.value = value;
  box.dispatchEvent(new Event('input'));
  await settle(r.fixture);
  return box;
};

/** A drag or paste event carrying files (jsdom has no DataTransfer). */
const carrying = <E extends Event>(e: E, key: 'dataTransfer' | 'clipboardData', files: File[]): E => {
  Object.defineProperty(e, key, { value: { files, types: files.length ? ['Files'] : ['text/plain'], dropEffect: 'none' } });
  return e;
};

describe('PdAssistant composer (ADR 0023 WP-4b)', () => {
  it('attaches picked files as chips with their size, sends them with the question and shows them on the sent turn', async () => {
    const { r, backend } = await open();
    expect(r.one('[data-testid=assistant-attach]').getAttribute('aria-label')).toBe('Attach files');
    expect(r.one('[data-testid=assistant-attach]').getAttribute('title')).toContain('256 KiB');
    expect(r.one('[data-testid=assistant-file-input]').getAttribute('accept')).toContain('.rs');
    await pick(r, [file('notes.md', 'one\ntwo\n'), file('main.rs', 'fn main() {}')]);
    expect(r.all('[data-testid=assistant-file]').map((c) => c.getAttribute('title'))).toEqual(['notes.md (8 B · 2 lines)', 'main.rs (12 B · 1 line)']);
    await type(r, 'Summarise these');
    r.one<HTMLButtonElement>('[data-testid=assistant-send]').click();
    await settleAll(r);
    expect(backend.sent[0]?.attachments?.map((a) => (a.kind === 'file' ? a.name : a.kind))).toEqual(['notes.md', 'main.rs']);
    expect(r.all('[data-testid=assistant-file]')).toHaveLength(0);
    expect(text(r, 'assistant-turn-file')).toBe('notes.md|main.rs');
  });

  it('removes a file before sending and gives the focus back to the question', async () => {
    const { r, backend } = await open();
    await pick(r, [file('a.txt', 'a'), file('b.txt', 'b')]);
    const remove = r.all<HTMLButtonElement>('[data-testid=assistant-file-remove]');
    expect(remove[0]?.getAttribute('aria-label')).toBe('Remove a.txt');
    remove[0]?.click();
    await settleAll(r);
    expect(text(r, 'assistant-file')).toContain('b.txt');
    expect(text(r, 'assistant-file')).not.toContain('a.txt');
    expect(document.activeElement?.getAttribute('data-testid')).toBe('assistant-input');
    await type(r, 'q');
    r.one<HTMLButtonElement>('[data-testid=assistant-send]').click();
    await settleAll(r);
    expect(backend.sent[0]?.attachments).toHaveLength(1);
  });

  it("says the engine's refusal and the early size check in words, and attaches the rest", async () => {
    const { r, attachments } = await open({ attachments: new MemoryAssistantAttachments({ maxFileBytes: 10 }) });
    attachments?.blockImages('Tiny can\'t read images. Choose Eyes to ask about an image.');
    await pick(r, [file('photo.png', 'png', 'image/png'), file('huge.log', 'x'.repeat(11)), file('ok.txt', 'fine')]);
    const failure = text(r, 'assistant-failure');
    expect(failure).toContain("Tiny can't read images. Choose Eyes to ask about an image.");
    // The early size check uses the engine's own words, next step included (review 2 P2-D).
    expect(failure).toContain('"huge.log" is larger than 10 bytes. Attach a smaller file, or paste the part that matters.');
    // The host's gate refuses the picture before it is read; the engine would refuse it too.
    expect(attachments?.added).toEqual(['ok.txt']);
    expect(text(r, 'assistant-file')).toContain('ok.txt');
  });

  it('shows "Attaching…" while the engine reads a file and keeps Send off until it is done', async () => {
    const { r, attachments } = await open();
    attachments?.hold();
    await type(r, 'q');
    await pick(r, [file('slow.txt', 'slow')]);
    expect(text(r, 'assistant-attaching')).toContain('Attaching…');
    expect(r.one<HTMLButtonElement>('[data-testid=assistant-send]').disabled).toBe(true);
    attachments?.release();
    await settleAll(r);
    expect(r.all('[data-testid=assistant-attaching]')).toHaveLength(0);
    expect(r.one<HTMLButtonElement>('[data-testid=assistant-send]').disabled).toBe(false);
  });

  it('stops offering the button at the count the host allows', async () => {
    const { r } = await open({ attachments: new MemoryAssistantAttachments({ maxCount: 2 }) });
    await pick(r, [file('1.txt', '1'), file('2.txt', '2'), file('3.txt', '3')]);
    expect(r.all('[data-testid=assistant-file]')).toHaveLength(2);
    expect(text(r, 'assistant-failure')).toContain('at most 2 attachments');
    expect(r.one<HTMLButtonElement>('[data-testid=assistant-attach]').disabled).toBe(true);
  });

  it('attaches pasted and dropped files; pasted text stays text', async () => {
    const { r } = await open();
    const box = r.one<HTMLTextAreaElement>('[data-testid=assistant-input]');
    const textPaste = carrying(new Event('paste', { bubbles: true, cancelable: true }), 'clipboardData', []);
    box.dispatchEvent(textPaste);
    expect(textPaste.defaultPrevented).toBe(false);
    const filePaste = carrying(new Event('paste', { bubbles: true, cancelable: true }), 'clipboardData', [file('pasted.txt', 'p')]);
    box.dispatchEvent(filePaste);
    await settleAll(r);
    expect(filePaste.defaultPrevented).toBe(true);
    const composer = r.one<HTMLFormElement>('[data-testid=assistant-composer]');
    composer.dispatchEvent(carrying(new Event('dragover', { bubbles: true, cancelable: true }), 'dataTransfer', [file('x', 'x')]));
    await settle(r.fixture);
    expect(composer.classList.contains('dropping')).toBe(true);
    const drop = carrying(new Event('drop', { bubbles: true, cancelable: true }), 'dataTransfer', [file('dropped.txt', 'd')]);
    composer.dispatchEvent(drop);
    await settleAll(r);
    expect(drop.defaultPrevented).toBe(true);
    expect(composer.classList.contains('dropping')).toBe(false);
    expect(text(r, 'assistant-file')).toContain('pasted.txt');
    expect(text(r, 'assistant-file')).toContain('dropped.txt');
  });

  describe('images (ADR 0023 WP-5)', () => {
    const picture = (name = 'cat.png', body = 'pixels'): File => file(name, body, 'image/png');

    it('attaches an image as a chip with its thumbnail, sends it with the question and shows the thumbnail on the sent turn', async () => {
      const { r, backend } = await open();
      expect(r.one('[data-testid=assistant-file-input]').getAttribute('accept')).toContain('image/png');
      await pick(r, [picture()]);
      expect(r.all('[data-testid=assistant-file-thumb]')).toHaveLength(1);
      expect(r.one('[data-testid=assistant-file-thumb]').getAttribute('src')).toMatch(/^data:image\/png;base64,/);
      expect(r.one('[data-testid=assistant-file]').getAttribute('title')).toBe('cat.png (640 × 480)');
      await type(r, 'What is this?');
      r.one<HTMLButtonElement>('[data-testid=assistant-send]').click();
      await settleAll(r);
      expect(backend.sent[0]?.attachments?.map((a) => a.kind)).toEqual(['image']);
      expect(r.all('[data-testid=assistant-file]')).toHaveLength(0);
      expect(r.all('[data-testid=assistant-turn-thumb]')).toHaveLength(1);
    });

    it("asks the host for a sent image's thumbnail when its chip had none (a reopened chat)", async () => {
      class Plain extends MemoryAssistantAttachments {
        override async add(f: Parameters<MemoryAssistantAttachments['add']>[0]) {
          return { ...(await super.add(f)), thumbnail: null };
        }
      }
      const { r } = await open({ attachments: new Plain() });
      await pick(r, [picture()]);
      expect(r.all('[data-testid=assistant-file-thumb]')).toHaveLength(0);
      await type(r, 'q');
      r.one<HTMLButtonElement>('[data-testid=assistant-send]').click();
      await settleAll(r);
      expect(r.all('[data-testid=assistant-turn-thumb]')).toHaveLength(1);
    });

    it('refuses an image over its own size limit in words, before reading it, while a text file is judged by its own', async () => {
      const { r, attachments } = await open({ attachments: new MemoryAssistantAttachments({ maxFileBytes: 4, maxImageBytes: 3 * 1024 * 1024 }) });
      await pick(r, [picture('big.png', 'x'.repeat(3 * 1024 * 1024 + 1)), picture('ok.png', 'xxxxx')]);
      expect(text(r, 'assistant-failure')).toContain('"big.png" is larger than 3 MiB. Attach a smaller image, or export a smaller copy.');
      expect(attachments?.added).toEqual(['ok.png']);
    });

    it("keeps Send off, with the host's words, while an image waits for a model that cannot read images; text and removing the image clear it", async () => {
      const { r, attachments } = await open();
      await pick(r, [picture()]);
      await type(r, 'What is this?');
      const send = r.one<HTMLButtonElement>('[data-testid=assistant-send]');
      expect(send.disabled).toBe(false);
      expect(r.all('[data-testid=assistant-image-block]')).toHaveLength(0);
      attachments?.blockImages("Tiny can't read images. Choose Eyes to ask about an image.");
      await settleAll(r);
      expect(send.disabled).toBe(true);
      expect(text(r, 'assistant-image-block')).toBe("Tiny can't read images. Choose Eyes to ask about an image.");
      r.one<HTMLButtonElement>('[data-testid=assistant-file-remove]').click();
      await settleAll(r);
      expect(r.all('[data-testid=assistant-image-block]')).toHaveLength(0);
      expect(r.one<HTMLButtonElement>('[data-testid=assistant-send]').disabled).toBe(false);
      await pick(r, [file('notes.txt', 'text needs no vision')]);
      expect(r.one<HTMLButtonElement>('[data-testid=assistant-send]').disabled).toBe(false);
    });

    it('says an image limit under 1 MiB in KiB or bytes, never "0 MiB"', async () => {
      const { r } = await open({ attachments: new MemoryAssistantAttachments({ maxImageBytes: 3 * 1024 }) });
      await pick(r, [picture('big.png', 'x'.repeat(3 * 1024 + 1))]);
      expect(text(r, 'assistant-failure')).toContain('"big.png" is larger than 3 KiB. Attach a smaller image');
      const small = await open({ attachments: new MemoryAssistantAttachments({ maxImageBytes: 500 }) });
      await pick(small.r, [picture('big.png', 'x'.repeat(501))]);
      expect(text(small.r, 'assistant-failure')).toContain('"big.png" is larger than 500 bytes. Attach a smaller image');
    });

    it('treats only the four raster types as images: an SVG is text, judged by the text limit and never held back by the image gate', async () => {
      const { r, attachments } = await open({ attachments: new MemoryAssistantAttachments({ maxFileBytes: 10, maxImageBytes: 5 * 1024 * 1024 }) });
      attachments?.blockImages("Tiny can't read images.");
      await pick(r, [file('logo.svg', 'x'.repeat(11), 'image/svg+xml')]);
      expect(text(r, 'assistant-failure')).toContain('"logo.svg" is larger than 10 bytes. Attach a smaller file');
      expect(text(r, 'assistant-failure')).not.toContain("can't read images");
    });

    it("offers the host's button beside the words, in the note and in the refusal", async () => {
      const run = vi.fn();
      const { r, attachments } = await open();
      attachments?.blockImages("Tiny can't read images. Choose Eyes to ask about an image.", { label: 'Use Eyes for images', run });
      await pick(r, [picture()]);
      expect(text(r, 'assistant-failure-image-action')).toBe('Use Eyes for images');
      r.one<HTMLButtonElement>('[data-testid=assistant-failure-image-action]').click();
      expect(run).toHaveBeenCalledTimes(1);
      await settleAll(r);
      expect(r.all('[data-testid=assistant-failure]')).toHaveLength(0);
      // A text file's refusal is not about images: no button there.
      attachments?.blockImages(null, null);
      await settleAll(r);
      expect(r.all('[data-testid=assistant-failure-image-action]')).toHaveLength(0);
    });

    it('gives a sent image its name for the alt text, and the tray scrolls on whole rows', async () => {
      const { r } = await open();
      await pick(r, [picture('rack-photo.png')]);
      expect(r.one('[data-testid=assistant-file-thumb]').getAttribute('alt')).toBe('rack-photo.png');
      expect(r.one('[data-testid=assistant-files]').classList.contains('thumbs')).toBe(true);
    });

    it('attaches a pasted image and a dropped one like any file', async () => {
      const { r } = await open();
      const box = r.one<HTMLTextAreaElement>('[data-testid=assistant-input]');
      const paste = carrying(new Event('paste', { bubbles: true, cancelable: true }), 'clipboardData', [picture('image.png')]);
      box.dispatchEvent(paste);
      await settleAll(r);
      expect(paste.defaultPrevented).toBe(true);
      r.one<HTMLFormElement>('[data-testid=assistant-composer]').dispatchEvent(carrying(new Event('drop', { bubbles: true, cancelable: true }), 'dataTransfer', [picture('dropped.png', 'other')]));
      await settleAll(r);
      expect(r.all('[data-testid=assistant-file-thumb]')).toHaveLength(2);
    });
  });

  it('has no attach button, paste or drop in a host without attachments, and no microphone without speech', async () => {
    const { r } = await open({ attachments: null, speech: null });
    expect(r.all('[data-testid=assistant-attach]')).toHaveLength(0);
    expect(r.all('[data-testid=assistant-mic]')).toHaveLength(0);
    const composer = r.one<HTMLFormElement>('[data-testid=assistant-composer]');
    const drop = carrying(new Event('drop', { bubbles: true, cancelable: true }), 'dataTransfer', [file('x.txt', 'x')]);
    composer.dispatchEvent(drop);
    expect(drop.defaultPrevented).toBe(false);
  });

  it("push-to-talk: records with the engine's time, puts the transcript after the typed text, and Escape throws a recording away", async () => {
    const { r, speech, backend } = await open();
    const mic = r.one<HTMLButtonElement>('[data-testid=assistant-mic]');
    expect(mic.getAttribute('aria-label')).toBe('Speak your question');
    await type(r, 'Please answer:');
    mic.click();
    await settleAll(r);
    expect(mic.getAttribute('aria-pressed')).toBe('true');
    expect(mic.getAttribute('aria-label')).toBe('Stop recording');
    speech?.tick(7_400);
    await settleAll(r);
    expect(text(r, 'assistant-recording')).toBe('Recording 0:07');
    speech?.tick(60_000);
    await settleAll(r);
    expect(text(r, 'assistant-recording')).toBe('Recording 1:07');
    mic.click();
    await settleAll(r);
    expect(r.one<HTMLTextAreaElement>('[data-testid=assistant-input]').value).toBe('Please answer: How busy is this computer right now?');
    expect(backend.sent).toHaveLength(0);
    mic.click();
    await settleAll(r);
    r.one('[data-testid=assistant-input]').dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }));
    await settleAll(r);
    expect(speech?.now.kind).toBe('idle');
    expect(r.all('[data-testid=assistant-recording]')).toHaveLength(0);
  });

  it("says why speech input failed, in the engine's words", async () => {
    const speech = new MemoryAssistantSpeech();
    speech.fail('Pitwall may not use the microphone. Allow it in System Settings > Privacy > Microphone.');
    const { r } = await open({ speech });
    r.one<HTMLButtonElement>('[data-testid=assistant-mic]').click();
    await settleAll(r);
    expect(text(r, 'assistant-failure')).toContain('Privacy > Microphone');
  });

  it("shows the engine's context count, an estimate as about, and each level in words and colour", async () => {
    const { r, backend } = await open({ context: use(1234, 7168, 'ok', false) });
    expect(backend.asked).toContain(null);
    expect(text(r, 'assistant-context')).toBe('About 1.2k of 7.2k tokens');
    // A meter people with a screen reader hear, level included (review P1-1).
    const meter = r.one('[data-testid=assistant-context]');
    expect(meter.getAttribute('role')).toBe('meter');
    expect([meter.getAttribute('aria-valuenow'), meter.getAttribute('aria-valuemin'), meter.getAttribute('aria-valuemax')]).toEqual(['1234', '0', '7168']);
    expect(meter.getAttribute('aria-label')).toBe('Context used');
    expect(meter.getAttribute('aria-valuetext')).toBe('About 1.2k of 7.2k tokens');
    expect(r.one('[data-testid=assistant-context]').getAttribute('data-level')).toBe('ok');
    expect(r.one('[data-testid=assistant-context]').getAttribute('title')).toContain("tools' definitions");

    backend.used = use(6900, 7168, 'high');
    await type(r, 'go on');
    r.one<HTMLButtonElement>('[data-testid=assistant-send]').click();
    await settleAll(r);
    // Close to the limit says so in words and with a glyph, not by colour alone (review P1-1).
    expect(text(r, 'assistant-context')).toBe('6.9k of 7.2k tokens · close to the limit');
    expect(r.one('[data-testid=assistant-context]').getAttribute('aria-valuetext')).toBe('6.9k of 7.2k tokens, close to the limit');
    expect(r.all('[data-testid=assistant-context] pd-status-glyph')).toHaveLength(1);
    expect(r.one('[data-testid=assistant-context]').getAttribute('data-level')).toBe('high');
    expect(r.all('[data-testid=assistant-context-full]')).toHaveLength(0);

    backend.used = use(7300, 7168, 'full');
    await type(r, 'more');
    r.one<HTMLButtonElement>('[data-testid=assistant-send]').click();
    await settleAll(r);
    // Full: the words say by how much, Send is off with the note as its reason, the note's button starts a new chat (P2-8, 11, 12).
    expect(text(r, 'assistant-context')).toBe('Full: over by 132 of 7.2k tokens');
    expect(text(r, 'assistant-context-full')).toContain('This chat fills what the model can read.');
    expect(text(r, 'assistant-context-new')).toBe('Start a new chat');
    await type(r, 'one more');
    const send = r.one<HTMLButtonElement>('[data-testid=assistant-send]');
    expect(send.disabled).toBe(true);
    expect(send.getAttribute('title')).toContain('This chat fills what the model can read.');
    backend.used = use(7168, 7168, 'full');
    await r.fixture.componentInstance['store'].refreshContext();
    await settleAll(r);
    expect(text(r, 'assistant-context')).toBe('Full: 7.2k of 7.2k tokens');
    backend.used = { usedTokens: 40, limitTokens: 0, reserveTokens: 0, exact: false, level: 'unknown' };
    r.one<HTMLButtonElement>('[data-testid=assistant-context-new]').click();
    await settleAll(r);
    expect(r.all('[data-testid=assistant-turn]')).toHaveLength(0);
    expect(text(r, 'assistant-context')).toBe('About 40 tokens');
    expect(r.one('[data-testid=assistant-context]').getAttribute('role')).toBeNull();
  });

  it('says the files are too long, with no new chat to start, when files fill a new chat (review 2 P2-C)', async () => {
    const { r, backend } = await open({ context: use(200, 7168, 'ok', false) });
    await pick(r, [file('big.log', 'x'.repeat(100))]);
    backend.used = use(9000, 7168, 'full', false);
    await r.fixture.componentInstance['store'].refreshContext();
    await settleAll(r);
    expect(text(r, 'assistant-context-full')).toBe('These files are too long for the model. Remove a file.');
    expect(r.all('[data-testid=assistant-context-new]')).toHaveLength(0);
    await type(r, 'Summarise it');
    expect(r.one('[data-testid=assistant-send]').getAttribute('title')).toBe('These files are too long for the model. Remove a file.');
    // Taking the file off ends it.
    backend.used = use(200, 7168, 'ok', false);
    r.one<HTMLButtonElement>('[data-testid=assistant-file-remove]').click();
    await settleAll(r);
    expect(r.all('[data-testid=assistant-context-full]')).toHaveLength(0);
  });

  it('says the files are too long when a chat that fits overflows only with them; a chat full on its own offers a new chat', async () => {
    const { r, backend } = await open();
    await type(r, 'First');
    r.one<HTMLButtonElement>('[data-testid=assistant-send]').click();
    await settleAll(r);
    // The chat alone fits; with the waiting file it does not: the file is the overflow.
    let chatAlone = use(3000, 7168, 'ok');
    backend.used = null;
    (backend as { context: (id: string | null, a?: readonly Attachment[]) => Promise<ContextUse | null> }).context = (_id, a = []) =>
      Promise.resolve(a.length ? use(9000, 7168, 'full', false) : chatAlone);
    await pick(r, [file('big.log', 'x'.repeat(100))]);
    expect(text(r, 'assistant-context-full')).toBe('These files are too long for the model. Remove a file.');
    expect(r.all('[data-testid=assistant-context-new]')).toHaveLength(0);
    // Full on its own: the chat is too long, a new chat is the way on.
    chatAlone = use(7300, 7168, 'full');
    await r.fixture.componentInstance['store'].refreshContext();
    await settleAll(r);
    expect(text(r, 'assistant-context-full')).toContain('This chat fills what the model can read.');
    expect(text(r, 'assistant-context-new')).toBe('Start a new chat');
  });

  describe('in a short pane (review 2 P2-A, review 3 P2-2: the pane\'s own height, not the window\'s)', () => {
    afterEach(() => {
      vi.restoreAllMocks();
      vi.unstubAllGlobals();
    });

    /** Every observed element, so a test can say the pane was resized (jsdom has no ResizeObserver and no layout). */
    let observers: { cb: ResizeObserverCallback; els: Element[] }[] = [];
    const resized = (): void => observers.forEach((o) => o.cb([], o as unknown as ResizeObserver));

    /** A pane `height()` px tall, and a files tray whose chips need `rows()` rows. */
    const shortPane = (height: () => number, rows: () => number): void => {
      observers = [];
      vi.stubGlobal(
        'ResizeObserver',
        class {
          private readonly o: { cb: ResizeObserverCallback; els: Element[] };
          constructor(cb: ResizeObserverCallback) {
            this.o = { cb, els: [] };
            observers.push(this.o);
          }
          observe(el: Element): void {
            this.o.els.push(el);
            this.o.cb([], this as unknown as ResizeObserver);
          }
          disconnect(): void {
            observers = observers.filter((o) => o !== this.o);
          }
        },
      );
      vi.spyOn(HTMLElement.prototype, 'scrollHeight', 'get').mockImplementation(function (this: HTMLElement) {
        return this.classList.contains('files-tray') ? rows() * 28 : 0;
      });
      vi.spyOn(HTMLElement.prototype, 'clientHeight', 'get').mockImplementation(function (this: HTMLElement) {
        if (this.classList.contains('pd-assistant')) return height();
        return this.classList.contains('files-tray') ? Math.min(rows(), this.classList.contains('folded') ? 2 : 3) * 28 : 0;
      });
    };

    it('folds many files to two rows behind an "N files" chip that opens and closes the tray', async () => {
      let rows = 1;
      shortPane(() => 360, () => rows);
      const { r } = await open();
      await pick(r, [file('a.txt', 'a'), file('b.txt', 'b')]);
      expect(r.one('[data-testid=assistant-files]').classList.contains('folded')).toBe(true);
      expect(r.all('[data-testid=assistant-files-summary]')).toHaveLength(0);
      expect(r.one('[data-testid=assistant-files]').classList.contains('scrolls')).toBe(false);
      rows = 4;
      await pick(r, Array.from({ length: 10 }, (_, i) => file(`part-${i}.txt`, `${i}`)));
      const summary = r.one<HTMLButtonElement>('[data-testid=assistant-files-summary]');
      expect(summary.textContent?.trim()).toBe('12 files');
      expect([summary.getAttribute('aria-expanded'), summary.getAttribute('aria-controls')]).toEqual(['false', 'pd-assistant-files']);
      expect(summary.getAttribute('title')).toBe('Show all attached files');
      summary.click();
      await settleAll(r);
      expect(r.one('[data-testid=assistant-files]').classList.contains('folded')).toBe(false);
      // Opened, it still holds more than it shows (three rows of four): it says it scrolls with a fade at its foot (review 3 P2-5).
      expect(r.one('[data-testid=assistant-files]').classList.contains('scrolls')).toBe(true);
      expect(r.one('[data-testid=assistant-files-summary]').getAttribute('aria-expanded')).toBe('true');
      expect(r.one('[data-testid=assistant-files-summary]').getAttribute('title')).toBe('Show fewer attached files');
      r.one<HTMLButtonElement>('[data-testid=assistant-files-summary]').click();
      await settleAll(r);
      expect(r.one('[data-testid=assistant-files]').classList.contains('folded')).toBe(true);
    });

    it('never folds in a tall pane, and folds when the pane itself gets short (a docked pane in a tall window too)', async () => {
      let height = 600;
      shortPane(() => height, () => 4);
      const { r } = await open();
      await pick(r, Array.from({ length: 12 }, (_, i) => file(`part-${i}.txt`, `${i}`)));
      expect(r.one('[data-testid=assistant-files]').classList.contains('folded')).toBe(false);
      expect(r.all('[data-testid=assistant-files-summary]')).toHaveLength(0);
      expect(r.fixture.nativeElement.classList.contains('short')).toBe(false);
      height = 360;
      resized();
      await settleAll(r);
      expect(r.fixture.nativeElement.classList.contains('short')).toBe(true);
      expect(r.one('[data-testid=assistant-files]').classList.contains('folded')).toBe(true);
      expect(r.one<HTMLTextAreaElement>('[data-testid=assistant-input]').rows).toBe(2);
      height = 600;
      resized();
      await settleAll(r);
      expect(r.fixture.nativeElement.classList.contains('short')).toBe(false);
    });
  });

  it('counts a waiting file at about four bytes a token when the memory engine is given the port\'s sizes (review 2 P2-F)', async () => {
    const attachments = new MemoryAssistantAttachments();
    const { r } = await open({ attachments, attachmentTokens: (a) => attachments.tokensOf(a) });
    expect(text(r, 'assistant-context')).toBe('About 40 of 7.2k tokens');
    await pick(r, [file('big.log', 'x'.repeat(4000))]);
    expect(text(r, 'assistant-context')).toBe('About 1k of 7.2k tokens');
  });

  it('asks the engine again with the files waiting in the composer, so the chips move the meter (review P1-2)', async () => {
    const { r, backend } = await open();
    await pick(r, [file('big.log', 'x'.repeat(100))]);
    expect(backend.pending.at(-1)?.map((a) => (a.kind === 'file' ? a.name : ''))).toEqual(['big.log']);
    r.one<HTMLButtonElement>('[data-testid=assistant-file-remove]').click();
    await settleAll(r);
    expect(backend.pending.at(-1)).toEqual([]);
  });

  it('tells the host when a file is taken off the question (review P2-1)', async () => {
    const { r, attachments } = await open();
    await pick(r, [file('a.txt', 'a')]);
    r.one<HTMLButtonElement>('[data-testid=assistant-file-remove]').click();
    await settleAll(r);
    expect(attachments?.removed).toEqual(['a.txt']);
  });

  it('keeps Send off while recording or transcribing (review P2-7)', async () => {
    const { r } = await open();
    await type(r, 'Check the disks');
    const send = (): HTMLButtonElement => r.one<HTMLButtonElement>('[data-testid=assistant-send]');
    expect(send().disabled).toBe(false);
    r.one<HTMLButtonElement>('[data-testid=assistant-mic]').click();
    await settleAll(r);
    expect(send().disabled).toBe(true);
  });

  it('lays a refusal out as one banner: its glyph, its words and Dismiss (review P2-13)', async () => {
    const { r } = await open({ attachments: new MemoryAssistantAttachments({ maxFileBytes: 1 }) });
    await pick(r, [file('two.txt', 'xx')]);
    const banner = r.one('[data-testid=assistant-failure]');
    expect(banner.querySelector('pd-status-glyph')).not.toBeNull();
    expect(banner.querySelector('[data-testid=assistant-failure-text]')?.textContent).toContain('"two.txt" is larger than');
    banner.querySelector<HTMLButtonElement>('[data-testid=assistant-failure-dismiss]')?.click();
    await settleAll(r);
    expect(r.all('[data-testid=assistant-failure]')).toHaveLength(0);
  });

  it('shows no meter for a backend that does not measure', async () => {
    const { r, backend } = await open({ speech: null });
    backend.used = null;
    // The memory backend measures; one that answers null (the base class) shows none.
    (backend as { context: (id: string | null, a?: readonly Attachment[]) => Promise<ContextUse | null> }).context = () => Promise.resolve(null);
    r.one<HTMLButtonElement>('[data-testid=assistant-new]').click();
    await settleAll(r);
    expect(r.all('[data-testid=assistant-context]')).toHaveLength(0);
  });
});
