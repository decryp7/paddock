import { Component, signal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { el, settle } from '../../testing/helpers';
import { AppLogo } from './app-logo';

@Component({
  imports: [AppLogo],
  template: `<debrief-app-logo id="a" [size]="size()" [label]="label()" /><debrief-app-logo id="b" />`,
})
class Host {
  readonly size = signal(48);
  readonly label = signal('');
}

describe('AppLogo', () => {
  async function render() {
    const fixture = TestBed.createComponent(Host);
    await settle(fixture);
    return fixture;
  }

  it('renders at the requested size and is decorative without a label', async () => {
    const fixture = await render();
    const svg = el<HTMLImageElement>(fixture, '#a img');
    expect(svg.getAttribute('width')).toBe('48');
    expect(svg.getAttribute('height')).toBe('48');
    expect(svg.getAttribute('aria-hidden')).toBe('true');
    expect(svg.getAttribute('role')).toBeNull();
    expect(el<HTMLImageElement>(fixture, '#b img').getAttribute('width')).toBe('20');
  });

  it('becomes an image with an accessible name when labelled', async () => {
    const fixture = await render();
    fixture.componentInstance.label.set('Debrief');
    await settle(fixture);
    const svg = el<HTMLImageElement>(fixture, '#a img');
    expect(svg.getAttribute('role')).toBe('img');
    expect(svg.getAttribute('aria-label')).toBe('Debrief');
    expect(svg.getAttribute('aria-hidden')).toBeNull();
  });

  it('shows the family icon from the installed master', async () => {
    const fixture = await render();
    expect(el<HTMLImageElement>(fixture, '#a img').getAttribute('src')).toBe('brand/debrief-macos.svg');
  });
});
