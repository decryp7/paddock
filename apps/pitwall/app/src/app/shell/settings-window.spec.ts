import { describe, expect, it } from 'vitest';
import { MockBackend } from '../core/mock-backend';
import { settingsPagesFor } from './settings-window';

describe('the Settings window pages', () => {
  it('lists Connected apps only in a build with the chat (Rust says so through chat_enabled)', async () => {
    const ids = async (backend: MockBackend) => (await settingsPagesFor(backend)).map((p) => p.id);
    expect(await ids(new MockBackend({ chat: true }))).toContain('pw.connected-apps');
    expect(await ids(new MockBackend({ chat: false }))).not.toContain('pw.connected-apps');
    // A build that cannot say has no chat; the other pages stay.
    const failing = await ids(new MockBackend({ failing: ['chatEnabled'] }));
    expect(failing).not.toContain('pw.connected-apps');
    expect(failing).toContain('pw.general');
  });
});
