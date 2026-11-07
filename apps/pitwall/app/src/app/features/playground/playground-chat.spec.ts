import { describe, expect, it } from 'vitest';
import { render, settle } from '../../../testing/render';
import { PlaygroundChat } from './playground-chat';
import { resetChatBackend } from './chat-backend';

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
  });
});
