import { TestBed } from '@angular/core/testing';
import { MatIconRegistry } from '@angular/material/icon';
import { DomSanitizer } from '@angular/platform-browser';
import type { PitwallBackend } from '../../core/backend';
import { BACKEND } from '../../core/backend';
import { MockBackend } from '../../core/mock-backend';
import { registerIcons } from '../../shared/icons';
import { dialogsAnswering } from '../../../testing/render';
import { ModelsStore } from './models.store';
import { PREVIEW_DELAY_MS } from './presets.store';
import { PresetsTab, statusDetail } from './presets-tab';

const tick = async (n = 6) => {
  for (let i = 0; i < n; i++) await new Promise((r) => setTimeout(r, 0));
};

async function render(backend: PitwallBackend = new MockBackend(), confirm = true) {
  TestBed.configureTestingModule({
    imports: [PresetsTab],
    providers: [ModelsStore, { provide: BACKEND, useValue: backend }, dialogsAnswering(confirm)],
  });
  registerIcons(TestBed.inject(MatIconRegistry), TestBed.inject(DomSanitizer));
  await TestBed.inject(ModelsStore).load();
  const fixture = TestBed.createComponent(PresetsTab);
  fixture.detectChanges();
  const settle = async () => {
    await tick();
    fixture.detectChanges();
    await tick();
    fixture.detectChanges();
  };
  await settle();
  const el = fixture.nativeElement as HTMLElement;
  const button = (text: string) =>
    Array.from(el.querySelectorAll('button')).find((b) => b.textContent?.trim().startsWith(text) || b.getAttribute('aria-label') === text) as HTMLButtonElement;
  const type = (selector: string, value: string) => {
    const input = el.querySelector(selector) as HTMLInputElement | HTMLTextAreaElement;
    input.value = value;
    input.dispatchEvent(new Event('input'));
  };
  /** Types, lets the debounce pass, and lets the engine answer. */
  const typeAndPreview = async (selector: string, value: string) => {
    type(selector, value);
    fixture.detectChanges();
    await new Promise((r) => setTimeout(r, PREVIEW_DELAY_MS + 20));
    await settle();
  };
  /** Runs an item of a preset's "..." menu. */
  const menu = async (alias: string, item: string) => {
    button(`More actions for ${alias}`).click();
    await settle();
    Array.from(document.querySelectorAll<HTMLButtonElement>('.mat-mdc-menu-item')).find((b) => b.textContent!.trim() === item)!.click();
    await settle();
  };
  return { fixture, el, settle, button, type, typeAndPreview, menu };
}

describe('statusDetail', () => {
  it('says what a status means in a sentence', () => {
    expect(statusDetail({ state: 'ready', modelId: 'm', chosenBy: 'explicit', reason: 'the preset a names this model', loaded: false })).toBe('Serves m. the preset a names this model.');
    expect(statusDetail({ state: 'stale', message: 'gone' })).toBe('gone.');
  });
});

describe('PresetsTab', () => {
  it('lists the presets with where each stands, in words', async () => {
    const { el } = await render();
    const rows = Array.from(el.querySelectorAll('tbody tr'));
    expect(rows.length).toBe(4);
    const text = el.textContent!;
    expect(text).toContain('debrief-explain');
    expect(text).toContain('Ready');
    expect(text).toContain('Model removed');
    expect(text).toContain('no longer installed');
    expect(text).toContain('1 cannot be served now');
    const triage = rows.find((r) => r.textContent!.includes('pitcrew-triage'))!;
    expect(triage.textContent).toContain('auto:chat,tools,fast');
    expect(triage.textContent).toContain('temperature 0.1');
    expect(rows.find((r) => r.textContent!.includes('debrief-explain'))!.textContent).toContain('system prompt');
  });

  it('shows an empty state with a way to create the first preset', async () => {
    const { el, button, settle } = await render(new MockBackend({ empty: true }));
    expect(el.textContent).toContain('No presets yet');
    button('New preset').click();
    await settle();
    expect(el.querySelector('form[aria-label="Preset editor"]')).not.toBeNull();
  });

  it('shows a load error with Retry', async () => {
    const backend = new MockBackend();
    const spy = vi.spyOn(backend, 'presets').mockRejectedValueOnce({ code: 'serverUnavailable', message: 'Server down' });
    const { el, button, settle } = await render(backend);
    expect(el.querySelector('[role="alert"]')?.textContent).toContain('Server down');
    button('Retry').click();
    await settle();
    expect(spy).toHaveBeenCalledTimes(2);
    expect(el.querySelectorAll('tbody tr').length).toBe(4);
  });

  it('creates a preset: the engine previews the draft live and the new preset appears', async () => {
    const { el, button, typeAndPreview, settle } = await render();
    button('New preset').click();
    await settle();
    expect((button('Create preset') as HTMLButtonElement).disabled).toBe(true);
    await typeAndPreview('#pw-preset-alias', 'dyno-notes');
    await typeAndPreview('#pw-preset-target', 'auto:chat,fast');
    const preview = el.querySelector('[aria-label="What this preset resolves to"]')!.textContent!;
    expect(preview).toContain('Ready');
    expect(preview).toContain('Would serve');
    expect(preview).toContain('qwen3.5-2b-q8_0');
    await typeAndPreview('#pw-preset-temperature', '0.4');
    await typeAndPreview('#pw-preset-system', 'Be short.');
    expect((button('Create preset') as HTMLButtonElement).disabled).toBe(false);
    button('Create preset').click();
    await settle();
    expect(el.querySelector('form')).toBeNull();
    const row = Array.from(el.querySelectorAll('tbody tr')).find((r) => r.textContent!.includes('dyno-notes'))!;
    expect(row.textContent).toMatch(/temperature 0.4/i);
    expect(row.textContent).toContain('system prompt');
  });

  it('blocks Save and says why when the engine finds a problem', async () => {
    const { el, button, typeAndPreview, settle } = await render();
    button('New preset').click();
    await settle();
    await typeAndPreview('#pw-preset-alias', 'debrief-explain');
    await typeAndPreview('#pw-preset-target', 'qwen3.5-9b-q4_k_m');
    const preview = el.querySelector('[aria-label="What this preset resolves to"]')!.textContent!;
    expect(preview).toContain('Cannot be saved');
    expect(preview).toContain('already exists');
    expect((button('Create preset') as HTMLButtonElement).disabled).toBe(true);
    await typeAndPreview('#pw-preset-alias', 'debrief-again');
    expect((button('Create preset') as HTMLButtonElement).disabled).toBe(false);
    await typeAndPreview('#pw-preset-target', 'removed-model');
    expect(el.querySelector('[aria-label="What this preset resolves to"]')!.textContent).toContain('Model removed');
    expect((button('Create preset') as HTMLButtonElement).disabled).toBe(true);
  });

  it('flags a number field that is not a number and sends nothing for it', async () => {
    const { el, button, type, settle } = await render();
    button('New preset').click();
    await settle();
    type('#pw-preset-alias', 'a-name');
    type('#pw-preset-target', 'qwen3.5-9b-q4_k_m');
    type('#pw-preset-temperature', 'warm');
    await settle();
    expect(el.querySelector('#pw-preset-temperature')!.getAttribute('aria-invalid')).toBe('true');
    expect(el.textContent).toContain('must be plain numbers');
    expect((button('Create preset') as HTMLButtonElement).disabled).toBe(true);
    type('#pw-preset-temperature', '0.5');
    await settle();
    expect(el.textContent).not.toContain('must be plain numbers');
  });

  it('edits a preset: the form starts from it and the preset is not its own duplicate', async () => {
    const { el, button, typeAndPreview, settle } = await render();
    button('Edit pitcrew-triage').click();
    await settle();
    expect((el.querySelector('#pw-preset-alias') as HTMLInputElement).value).toBe('pitcrew-triage');
    expect((el.querySelector('#pw-preset-context') as HTMLInputElement).value).toBe('16384');
    expect(el.textContent).toContain('Edit pitcrew-triage');
    await typeAndPreview('#pw-preset-temperature', '0.2');
    expect(el.querySelector('[aria-label="What this preset resolves to"]')!.textContent).not.toContain('Cannot be saved');
    button('Save preset').click();
    await settle();
    expect(el.querySelector('form')).toBeNull();
    expect(Array.from(el.querySelectorAll('tbody tr')).find((r) => r.textContent!.includes('pitcrew-triage'))!.textContent).toContain('temperature 0.2');
  });

  it('fixes a preset whose model was removed by pointing it at an installed one', async () => {
    const { el, button, typeAndPreview, settle } = await render();
    button('Edit legacy-explain').click();
    await settle();
    await typeAndPreview('#pw-preset-target', 'qwen3.5-9b-q4_k_m');
    button('Save preset').click();
    await settle();
    expect(el.textContent).not.toContain('Model removed');
    expect(el.textContent).not.toContain('cannot be served now');
  });

  it('shows the engine\'s message inline when saving is refused, and Cancel closes the editor', async () => {
    const backend = new MockBackend();
    const { el, button, typeAndPreview, settle } = await render(backend);
    button('New preset').click();
    await settle();
    await typeAndPreview('#pw-preset-alias', 'fine-name');
    await typeAndPreview('#pw-preset-target', 'qwen3.5-9b-q4_k_m');
    vi.spyOn(backend, 'createPreset').mockRejectedValueOnce({ code: 'internal', message: 'the presets could not be saved (disk full)' });
    button('Create preset').click();
    await settle();
    expect(el.querySelector('form [role="alert"]')?.textContent).toContain('disk full');
    button('Cancel').click();
    await settle();
    expect(el.querySelector('form')).toBeNull();
  });

  it('offers installed models and capability selections as choices', async () => {
    const { el, button, settle } = await render();
    button('New preset').click();
    await settle();
    const target = el.querySelector('#pw-preset-target') as HTMLInputElement;
    target.dispatchEvent(new Event('focusin'));
    await settle();
    const options = Array.from(document.querySelectorAll('.mat-mdc-option .mono')).map((o) => o.textContent?.trim());
    expect(options).toContain('qwen3.5-9b-q4_k_m');
    expect(options).toContain('auto:chat,fast');
    expect(el.querySelector('#pw-preset-engine')).toBeNull();
  });

  it('asks before deleting, and deletes only when confirmed', async () => {
    const no = await render(new MockBackend(), false);
    expect(no.el.querySelector('button[aria-label="Delete legacy-explain"]')).toBeNull();
    await no.menu('legacy-explain', 'Delete…');
    expect(no.el.querySelectorAll('tbody tr').length).toBe(4);
    TestBed.resetTestingModule();
    const yes = await render(new MockBackend(), true);
    await yes.menu('legacy-explain', 'Delete…');
    expect(yes.el.querySelectorAll('tbody tr').length).toBe(3);
    expect(yes.el.textContent).not.toContain('legacy-explain');
  });

describe('PresetsTab caching', () => {
  it('says in words which presets cache answers or keep the prompt warm, under the name', async () => {
    const { el } = await render();
    const rows = Array.from(el.querySelectorAll('tbody tr'));
    const about = (alias: string) => rows.find((r) => r.textContent!.includes(alias))!.querySelector('.alias .sub')!.textContent!.trim();
    expect(about('debrief-explain')).toMatch(/^Caches answers · warm prompt · /);
    expect(about('pitcrew-triage')).toMatch(/^Warm prompt · /);
    expect(about('legacy-explain')).toBe('Request decides every setting');
    // No pills on a second line (UI review 2 rule 8).
    expect(el.querySelector('tbody .badge')).toBeNull();
  });

  it('edits both toggles and saves them with the preset', async () => {
    const backend = new MockBackend();
    const { el, settle, button, type } = await render(backend);
    button('New preset').click();
    await settle();
    type('#pw-preset-alias', 'cached-one');
    type('#pw-preset-target', 'qwen3.5-9b-q4_k_m');
    await settle();
    const toggle = (id: string) => el.querySelector<HTMLButtonElement>(`button[aria-labelledby="${id}"]`)!;
    expect(toggle('pw-preset-cache-l').getAttribute('aria-checked')).toBe('false');
    expect(el.textContent).toContain('cleared when the model, engine or preset changes');
    toggle('pw-preset-cache-l').click();
    toggle('pw-preset-warm-l').click();
    await settle();
    expect(toggle('pw-preset-cache-l').getAttribute('aria-checked')).toBe('true');
    await new Promise((r) => setTimeout(r, PREVIEW_DELAY_MS + 20));
    await settle();
    button('Create preset').click();
    await settle();
    const saved = (await backend.presets()).find((e) => e.preset.alias === 'cached-one')!.preset;
    expect(saved).toMatchObject({ cacheResponses: true, keepWarm: true });
  });
});
});
