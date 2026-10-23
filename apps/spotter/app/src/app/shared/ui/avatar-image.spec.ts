import { Component, signal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { Avatar } from './avatar';

@Component({ imports: [Avatar], template: `<sp-avatar name="Acme Installer" [src]="src()" />` })
class Host {
  readonly src = signal<string | null>(null);
}

function make() {
  const f = TestBed.createComponent(Host);
  f.detectChanges();
  const el = f.nativeElement as HTMLElement;
  return { f, el };
}

describe('Avatar image', () => {
  it('shows the monogram while the engine has no image', () => {
    const { el } = make();
    expect(el.querySelector('img')).toBeNull();
    expect(el.textContent).toContain('AI');
  });

  it('shows the cached image when there is one, decorative and not draggable', () => {
    const { f, el } = make();
    f.componentInstance.src.set('data:image/png;base64,AAAA');
    f.detectChanges();
    const img = el.querySelector('img')!;
    expect(img.getAttribute('alt')).toBe('');
    expect(img.getAttribute('draggable')).toBe('false');
    expect(el.textContent).not.toContain('AI');
  });

  it('falls back to the monogram when the image fails, and tries a new image again', () => {
    const { f, el } = make();
    f.componentInstance.src.set('data:image/png;base64,AAAA');
    f.detectChanges();
    el.querySelector('img')!.dispatchEvent(new Event('error'));
    f.detectChanges();
    expect(el.querySelector('img')).toBeNull();
    expect(el.textContent).toContain('AI');
    f.componentInstance.src.set('data:image/png;base64,BBBB');
    f.detectChanges();
    expect(el.querySelector('img')).not.toBeNull();
  });
});

describe('Avatar lookup by watch', () => {
  it('shows the engine picture for a watch and keeps the monogram for one without', async () => {
    const { appProviders, makeMock, settle } = await import('../../../testing/test-helpers');
    @Component({ imports: [Avatar], template: `<sp-avatar name="Acme Installer" watch="w-1" /><sp-avatar name="Zed" watch="w-none" />` })
    class Lookup {}
    TestBed.configureTestingModule({ providers: appProviders(makeMock()) });
    const f = TestBed.createComponent(Lookup);
    await settle(f);
    const avatars = Array.from((f.nativeElement as HTMLElement).querySelectorAll('sp-avatar'));
    expect(avatars[0]!.querySelector('img')?.getAttribute('src')).toMatch(/^data:image\/svg\+xml/);
    expect(avatars[1]!.querySelector('img')).toBeNull();
    expect(avatars[1]!.textContent).toContain('Z');
  });
});
