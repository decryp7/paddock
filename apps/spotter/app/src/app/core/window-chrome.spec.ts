import { applyWindowChrome } from './window-chrome';

describe('applyWindowChrome', () => {
  it('marks the platform and turns Mica on only for Windows', () => {
    const root = document.createElement('html');
    applyWindowChrome(root, 'windows');
    expect(root.dataset['platform']).toBe('windows');
    expect(root.dataset['effects']).toBe('mica');
    applyWindowChrome(root, 'macos');
    expect(root.dataset['platform']).toBe('macos');
    expect(root.dataset['effects']).toBeUndefined();
  });
});
