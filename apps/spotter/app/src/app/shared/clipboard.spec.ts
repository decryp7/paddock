import { TestBed } from '@angular/core/testing';
import { provideShellTesting, shellPorts } from '@decryp7/paddock-ui/shell/testing';
import { Clipboard } from './clipboard';

describe('Clipboard', () => {
  beforeEach(() => TestBed.configureTestingModule({ providers: [...provideShellTesting()] }));
  afterEach(() => vi.restoreAllMocks());

  it('copies and confirms with a message', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true });
    await TestBed.inject(Clipboard).copy('hello', 'The path');
    expect(writeText).toHaveBeenCalledWith('hello');
    expect(shellPorts().notices.at(-1)).toMatchObject({ level: 'success', message: 'The path copied' });
  });

  it('explains when the clipboard is not available', async () => {
    Object.defineProperty(navigator, 'clipboard', { value: { writeText: vi.fn().mockRejectedValue(new Error('no')) }, configurable: true });
    await TestBed.inject(Clipboard).copy('x', 'It');
    const n = shellPorts().notices.at(-1)!;
    expect(n.level).toBe('error');
    expect(n.message).toMatch(/^Could not copy\. /);
    expect(n.message).toContain('keyboard');
  });
});
