import { describe, expect, it } from 'vitest';
import { MockBackend } from '../../core/mock-backend';
import { render, settle } from '../../../testing/render';
import { PlaygroundChat } from './playground-chat';
import { resetChatBackend } from './chat-backend';

const until = async (fixture: Parameters<typeof settle>[0], done: () => boolean) => {
  for (let i = 0; i < 100 && !done(); i++) await settle(fixture, 1);
};

describe('PlaygroundChat', () => {
  it('shows the assistant pane over the chat engine, in its empty state with the composer', async () => {
    resetChatBackend();
    const { fixture, el } = await render(PlaygroundChat);
    await settle(fixture);
    expect(el.querySelector('pd-assistant')).not.toBeNull();
    expect(el.querySelector('[data-testid=assistant-empty]')).not.toBeNull();
    expect(el.querySelector('[data-testid=assistant-input]')).not.toBeNull();
    // Pitwall is the engine itself: the family's "needs local AI" state never shows here.
    expect(el.querySelector('pd-needs-ai')).toBeNull();
    // Pitwall's own words, and no "Allow actions" tick: Dyno's allow and a question on every action call are the gates.
    expect(el.querySelector('[data-testid=assistant-empty]')?.textContent).toContain('Connected apps');
    expect(el.querySelector('[data-testid=assistant-allow-actions]')).toBeNull();
  });

  it('picks a tool-capable model, lists the others disabled with the reason, and says what is connected', async () => {
    const backend = new MockBackend();
    const { fixture, el } = await render(PlaygroundChat, backend);
    await until(fixture, () => !!el.querySelector('[data-testid=chat-apps]')?.textContent?.trim());
    expect(el.querySelector('[data-testid=chat-apps]')?.textContent).toContain('No app connected');
    const picker = await backend.chatModels();
    expect(picker.selected).toBe('qwen3.5-4b-q4_k_m');
    expect(picker.models.filter((m) => m.disabled).map((m) => m.name)).toContain('Gemma 4 12B');
    const usable = picker.models.filter((m) => !m.disabled).length;
    expect(picker.models.slice(usable).every((m) => m.disabled !== null)).toBe(true);
    (el.querySelector('[data-testid=chat-model] .mat-mdc-select-trigger') as HTMLElement).click();
    await settle(fixture);
    const options = Array.from(document.querySelectorAll<HTMLElement>('mat-option'));
    const gemma = options.find((o) => o.textContent?.includes('Gemma 4 12B'))!;
    expect(gemma.getAttribute('aria-disabled')).toBe('true');
    expect(gemma.textContent).toContain(" · Can't use tools");
    // The reason in full sits once inside the open list, as the group's label, never under the field.
    const notes = Array.from(document.querySelectorAll('[data-testid=chat-model-note]')).map((g) => g.textContent ?? '');
    expect(notes.some((n) => n.includes('chat template has no tool calls'))).toBe(true);
    // The starter catalog's tool models not downloaded yet: listed, disabled, with how to get one.
    const starter = options.find((o) => o.textContent?.includes('Qwen3.8 27B'))!;
    expect(starter.getAttribute('aria-disabled')).toBe('true');
    expect(starter.textContent).toContain(' · Not downloaded');
    expect(notes.some((n) => n.includes('download one in Models'))).toBe(true);
    expect(el.querySelector('mat-hint')).toBeNull();
    options.find((o) => o.textContent?.includes('Qwen3.5 9B'))!.click();
    await settle(fixture);
    expect((await backend.chatModels()).selected).toBe('qwen3.5-9b-q4_k_m');
    // Connected in another window (Settings): the chat hears it and follows, with no reload of its own.
    await backend.setAppKey('dyno', 'pmk_good');
    await until(fixture, () => !!el.querySelector('[data-testid=chat-apps]')?.textContent?.includes('Dyno'));
    expect(el.querySelector('[data-testid=chat-apps]')?.textContent).toContain('Tools from Dyno (6)');
    // The empty chat offers "Connected apps…" only while nothing is connected.
    expect(el.querySelector('[data-testid=assistant-empty-action]')).toBeNull();
    await backend.forgetApp('dyno');
    await until(fixture, () => !!el.querySelector('[data-testid=assistant-empty-action]'));
    expect(el.querySelector('[data-testid=assistant-empty-action]')?.textContent?.trim()).toBe('Connected apps…');
  });

  it('says when no model can use tools, and shows a failure with Try again', async () => {
    const none = new MockBackend({ empty: true });
    const r = await render(PlaygroundChat, none);
    await until(r.fixture, () => !!r.el.querySelector('[data-testid=chat-no-tool-model]'));
    expect(r.el.querySelector('[data-testid=chat-no-tool-model]')?.textContent).toContain('Download one in Models');
  });
});
