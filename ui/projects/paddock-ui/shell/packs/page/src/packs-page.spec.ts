import { provideZonelessChangeDetection } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { MemoryPacksBackend, memoryKind, provideShellPacks, type PacksBackend } from '@decryp7/paddock-ui/shell/packs';
import { PdPacksPage } from './packs-page';
import { assertStatusGlyphs } from '@decryp7/paddock-ui/testing';

async function settle(rounds = 12): Promise<void> {
  for (let i = 0; i < rounds; i++) await Promise.resolve();
}

const PACKS = [
  { name: 'builtin.toml', origin: 'builtin' as const },
  { name: 'mine.toml', origin: 'user' as const },
];
const BAD = [{ file: 'broken.toml', message: 'not valid TOML (line 2, column 3)' }];
const NOTE = [{ file: 'mine.toml', message: 'unknown key "extra" is ignored' }];

describe('the Knowledge packs page', () => {
  async function open(backend: PacksBackend) {
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection(), provideShellPacks({ backend: () => backend })] });
    const fixture = TestBed.createComponent(PdPacksPage);
    await settle();
    await fixture.whenStable();
    await settle();
    const el = fixture.nativeElement as HTMLElement;
    const q = (id: string): HTMLElement | null => el.querySelector(`[data-testid="${id}"]`);
    const click = async (id: string): Promise<void> => {
      (q(id) as HTMLButtonElement).click();
      await settle();
      await fixture.whenStable();
      await settle();
    };
    return { fixture, el, q, click };
  }

  it('lists the packs in use with their origin, and says all loaded', async () => {
    const { q, el } = await open(new MemoryPacksBackend());
    expect(q('packs-state-cleanup')?.textContent).toContain('All loaded');
    expect(el.textContent).toContain('Built in');
    expect(el.textContent).toContain('Yours');
    expect(el.querySelectorAll('.pd-packs__row').length).toBe(2);
    expect(q('packs-problems-cleanup')).toBeNull();
    expect(q('packs-reload')?.classList.contains('mat-mdc-unelevated-button')).toBe(false);
  });

  it('names a refused pack and why in an alert, and makes Reload the primary button', async () => {
    const { q, el } = await open(new MemoryPacksBackend({ kinds: [memoryKind('cleanup', 'Cleanup packs', PACKS, BAD, NOTE)] }));
    expect(q('packs-state-cleanup')?.textContent).toContain('One pack left out');
    const alert = el.querySelector('[data-testid="packs-problems-cleanup"] [role="alert"]');
    expect(alert?.textContent).toContain('broken.toml');
    expect(alert?.textContent).toContain('line 2, column 3');
    assertStatusGlyphs(el);
    expect(q('packs-warnings-cleanup')?.textContent).toContain('unknown key "extra"');
    expect(q('packs-reload')?.classList.contains('mat-mdc-unelevated-button')).toBe(true);
  });

  it('counts several refused packs', async () => {
    const { q } = await open(new MemoryPacksBackend({ kinds: [memoryKind('cleanup', 'Cleanup packs', PACKS, [...BAD, { file: 'b.toml', message: 'x' }])] }));
    expect(q('packs-state-cleanup')?.textContent).toContain('2 packs left out');
  });

  it('says "loaded, some keys ignored" for notes only', async () => {
    const { q } = await open(new MemoryPacksBackend({ kinds: [memoryKind('cleanup', 'Cleanup packs', PACKS, [], NOTE)] }));
    expect(q('packs-state-cleanup')?.textContent).toContain('some keys ignored');
  });

  it('Reload clears a fixed pack, and a change the engine finds shows without a click', async () => {
    const memory = new MemoryPacksBackend({ kinds: [memoryKind('cleanup', 'Cleanup packs', PACKS, BAD)], onReload: 'fix' });
    const { q, click, fixture } = await open(memory);
    await click('packs-reload');
    expect(q('packs-problems-cleanup')).toBeNull();
    expect(q('packs-state-cleanup')?.textContent).toContain('All loaded');
    memory.emit({ kinds: [memoryKind('cleanup', 'Cleanup packs', PACKS, BAD)] });
    await fixture.whenStable();
    expect(q('packs-problems-cleanup')).not.toBeNull();
  });

  it('shows "Still working" for as long as a reload runs, and never gives up on it', async () => {
    let release: () => void = () => undefined;
    class Slow extends MemoryPacksBackend {
      override async reload() {
        await new Promise<void>((r) => (release = r));
        return super.reload();
      }
    }
    const { q, fixture } = await open(new Slow());
    (q('packs-reload') as HTMLButtonElement).click();
    await settle();
    await fixture.whenStable();
    expect(q('packs-still-working')?.textContent).toContain('Still working');
    expect((q('packs-reload') as HTMLButtonElement).disabled).toBe(true);
    expect(fixture.nativeElement.querySelector('mat-progress-bar')?.getAttribute('aria-label')).toBe('Reading the packs…');
    release();
    await settle();
    await fixture.whenStable();
    expect(q('packs-reloading')).toBeNull();
  });

  it('a failed reload says why inline', async () => {
    class Failing extends MemoryPacksBackend {
      override async reload(): Promise<never> {
        throw new Error('the engine is busy');
      }
    }
    const { q, click } = await open(new Failing());
    await click('packs-reload');
    expect(q('packs-reload-error')?.getAttribute('role')).toBe('alert');
    expect(q('packs-reload-error')?.textContent).toContain('the engine is busy');
  });

  it('a failed ask shows an error with Retry, and the retry lists the packs', async () => {
    const { q, click } = await open(new MemoryPacksBackend({ statusFailures: 1 }));
    expect(q('packs-error')?.textContent).toContain('could not be listed');
    expect(q('packs-reload')).toBeNull();
    await click('packs-load-retry');
    expect(q('packs-state-cleanup')).not.toBeNull();
  });

  it('an app with no packs says so', async () => {
    const { q } = await open(new MemoryPacksBackend({ kinds: [] }));
    expect(q('packs-none')).not.toBeNull();
  });

  it('shows a loading word before the first answer', async () => {
    class Never extends MemoryPacksBackend {
      override status(): Promise<never> {
        return new Promise(() => undefined);
      }
    }
    const { q } = await open(new Never());
    expect(q('packs-loading')).not.toBeNull();
  });
});
