import { TestBed } from '@angular/core/testing';
import { describe, expect, it } from 'vitest';
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
    await pick(r, [file('photo.png', 'png', 'image/png'), file('huge.log', 'x'.repeat(11)), file('ok.txt', 'fine')]);
    const failure = text(r, 'assistant-failure');
    expect(failure).toContain('"photo.png" is an image');
    expect(failure).toContain('"huge.log" is larger than 10 bytes');
    expect(attachments?.added).toEqual(['photo.png', 'ok.txt']);
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
