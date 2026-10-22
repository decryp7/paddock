import { TestBed } from '@angular/core/testing';
import { AppAvatar, avatarHue } from './app-avatar';

describe('AppAvatar', () => {
  it('gives the same hue to the same key and different hues to different keys', () => {
    expect(avatarHue('debrief')).toBe(avatarHue('debrief'));
    expect(avatarHue('debrief')).not.toBe(avatarHue('pitcrew'));
    expect(avatarHue('')).toBeGreaterThanOrEqual(0);
    expect(avatarHue('x')).toBeLessThan(360);
  });

  it('shows the first letter, is hidden from screen readers and can be large', () => {
    const f = TestBed.createComponent(AppAvatar);
    f.componentRef.setInput('key', 'debrief');
    f.componentRef.setInput('name', ' debrief');
    f.componentRef.setInput('size', 'lg');
    f.detectChanges();
    const el = f.nativeElement as HTMLElement;
    expect(el.textContent).toBe('D');
    expect(el.getAttribute('aria-hidden')).toBe('true');
    expect(el.classList.contains('lg')).toBe(true);
    f.componentRef.setInput('name', '');
    f.detectChanges();
    expect(el.textContent).toBe('?');
  });
});
