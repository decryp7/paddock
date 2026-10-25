import { Component, provideZonelessChangeDetection } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { one, text } from '@decryp7/paddock-ui/testing';
import { afterEach, describe, expect, it } from 'vitest';
import { PdNeedsAiCard } from './needs-ai-card';
import { assertStatusGlyphs } from '@decryp7/paddock-ui/testing';

@Component({
  imports: [PdNeedsAiCard],
  template: `<pd-needs-ai-card [title]="title" [message]="message" [icon]="icon" [code]="code" [codeLabel]="'Pairing code'" [approve]="approve" [hint]="hint" [error]="error" kind="needsPairing"><button type="button" class="pd-btn">Do it</button></pd-needs-ai-card>`,
})
class Host {
  title = 'Ask needs local AI';
  message = 'Pitwall is not running.';
  icon: 'info' | 'warning' = 'info';
  code = '';
  approve = '';
  hint = '';
  error = '';
}

describe('<pd-needs-ai-card>', () => {
  afterEach(() => TestBed.resetTestingModule());
  async function render(over: Partial<Host> = {}) {
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection()] });
    const f = TestBed.createComponent(Host);
    Object.assign(f.componentInstance, over);
    await f.whenStable();
    return f.nativeElement as HTMLElement;
  }

  it('is a labelled region with the title, the message and the projected actions', async () => {
    const el = await render();
    const card = one(el, '[data-testid=needs-ai]');
    expect(card.getAttribute('aria-labelledby')).toBe(one(el, '.pd-needs-ai__title').id);
    expect(card.getAttribute('data-kind')).toBe('needsPairing');
    expect(text(one(el, '[data-testid=needs-ai-message]'))).toBe('Pitwall is not running.');
    expect(text(one(el, '.pd-needs-ai__actions'))).toBe('Do it');
    expect(el.querySelector('[data-testid=needs-ai-code]')).toBeNull();
    expect(el.querySelector('[data-testid=needs-ai-error]')).toBeNull();
    expect(el.querySelector('.pd-needs-ai__icon--blocked')).toBeNull();
  });

  it('shows the code with its hint, a quiet hint under the actions, a blocking icon and an alert', async () => {
    const el = await render({ code: '482 913', approve: 'Approve it in Pitwall.', hint: 'On the built-in engine.', error: 'Failed.', icon: 'warning' });
    expect(text(one(el, '[data-testid=needs-ai-code]'))).toBe('482 913');
    expect(one(el, '[data-testid=needs-ai-code]').getAttribute('aria-label')).toBe('Pairing code');
    expect(text(one(el, '[data-testid=needs-ai-approve]'))).toBe('Approve it in Pitwall.');
    expect(text(one(el, '[data-testid=needs-ai-on-builtin]'))).toBe('On the built-in engine.');
    expect(one(el, '[data-testid=needs-ai-error]').getAttribute('role')).toBe('alert');
    expect(one(el, '.pd-needs-ai__icon--blocked')).toBeTruthy();
    assertStatusGlyphs(el);
  });

  it('names a card that has no title by its name', async () => {
    const el = await render({ title: '' });
    expect(one(el, '[data-testid=needs-ai]').getAttribute('aria-labelledby')).toBeNull();
    expect(el.querySelector('.pd-needs-ai__title')).toBeNull();
  });
});
