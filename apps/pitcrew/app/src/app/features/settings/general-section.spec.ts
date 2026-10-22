import { TestBed } from '@angular/core/testing';
import { MockBackend } from '../../core/mock-backend';
import { PreferencesService } from '../../core/preferences.service';
import { appProviders, makeMock, settle, text } from '../../../testing/test-helpers';
import { GeneralSection } from './general-section';

describe('GeneralSection', () => {
  let mock: MockBackend;
  afterEach(() => mock.dispose());

  async function mount() {
    localStorage.clear();
    mock = makeMock();
    TestBed.configureTestingModule({ providers: appProviders(mock) });
    const f = TestBed.createComponent(GeneralSection);
    await settle(f, 6);
    return { f, el: f.nativeElement as HTMLElement };
  }

  it('groups the notifications under three headings in one column, with All and None per group', async () => {
    const { el } = await mount();
    expect(Array.from(el.querySelectorAll('.group h3')).map((h) => text(h))).toEqual(['Needs action', 'Updates', 'Housekeeping']);
    expect(el.querySelectorAll('.kinds li').length).toBe(16);
  });

  it('turns a whole group off and on', async () => {
    const { el } = await mount();
    const prefs = TestBed.inject(PreferencesService);
    const none = el.querySelector<HTMLButtonElement>('button[aria-label="Turn off all Updates"]')!;
    none.click();
    expect(prefs.isEnabled('updated')).toBe(false);
    expect(prefs.isEnabled('updateAvailable')).toBe(false);
    expect(prefs.isEnabled('stalled')).toBe(true);
    el.querySelector<HTMLButtonElement>('button[aria-label="Turn on all Updates"]')!.click();
    expect(prefs.isEnabled('updated')).toBe(true);
  });

  it('marks the chosen theme with a check as well as the border', async () => {
    const { el } = await mount();
    expect(el.querySelectorAll('.theme[aria-checked="true"] .check').length).toBe(1);
    expect(el.querySelectorAll('.theme[aria-checked="false"] .check').length).toBe(0);
  });
});
