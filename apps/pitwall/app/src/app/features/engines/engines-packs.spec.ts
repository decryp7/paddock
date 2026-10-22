import { TestBed } from '@angular/core/testing';
import { MatIconRegistry } from '@angular/material/icon';
import { DomSanitizer } from '@angular/platform-browser';
import { BACKEND } from '../../core/backend';
import { LiveStore } from '../../core/live.store';
import { MockBackend } from '../../core/mock-backend';
import { registerIcons } from '../../shared/icons';
import { Confirm } from '../../shared/ui/confirm';
import { EnginesPage } from './engines-page';
import { EnginesStore } from './engines.store';

const tick = async (n = 6) => {
  for (let i = 0; i < n; i++) await new Promise((r) => setTimeout(r, 0));
};

describe('Engines and packs', () => {
  function store(backend = new MockBackend()) {
    TestBed.configureTestingModule({ providers: [EnginesStore, { provide: BACKEND, useValue: backend }] });
    return TestBed.inject(EnginesStore);
  }

  it('keeps the pack notes next to the engines, and survives a failing packs call', async () => {
    const s = store();
    await s.load();
    expect(s.packProblems().map((n) => n.file)).toEqual(['packs/old-vllm.toml']);
    expect(s.packWarnings().length).toBe(1);
    TestBed.resetTestingModule();
    const broken = store(new MockBackend({ failing: ['packs'] }));
    await broken.load();
    expect(broken.loaded()).toBe(true);
    expect(broken.packProblems()).toEqual([]);
  });

  it('lists a pack engine with the others and installs it by its pack id', async () => {
    const backend = new MockBackend();
    const s = store(backend);
    await s.load();
    const pack = s.available().find((e) => e.kind === 'pack')!;
    expect(pack.catalog[0]!.id).toBe('ik-llama-cpu');
    expect(await s.install(pack.catalog[0]!)).toBe(true);
    for (let i = 0; i < 400; i++) backend.tick();
    await s.load();
    const done = s.engines().find((e) => e.kind === 'pack')!;
    expect(done.builds.map((b) => b.id)).toEqual(['ik-llama-cpu']);
    expect(done.availability.state).toBe('available');
  });

  it('shows skipped packs as an inline notice on the page', async () => {
    TestBed.configureTestingModule({ imports: [EnginesPage], providers: [{ provide: BACKEND, useValue: new MockBackend() }, { provide: Confirm, useValue: { ask: () => Promise.resolve(true) } }] });
    registerIcons(TestBed.inject(MatIconRegistry), TestBed.inject(DomSanitizer));
    await TestBed.inject(LiveStore).load();
    const fixture = TestBed.createComponent(EnginesPage);
    fixture.detectChanges();
    await tick();
    fixture.detectChanges();
    const notice = (fixture.nativeElement as HTMLElement).querySelector('.pack-notice')!;
    expect(notice.textContent).toContain('1 pack file was skipped');
    expect(notice.textContent).toContain('packs/old-vllm.toml');
    expect(notice.textContent).toContain('unknown key `notes` ignored');
  });
});
