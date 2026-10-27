import { TestBed } from '@angular/core/testing';
import { MatIconRegistry } from '@angular/material/icon';
import { firstValueFrom } from 'rxjs';
import { ICONS, provideIcons } from './icons';
import { kindIcon } from './labels';
import { SOURCE_KINDS } from '../core/mock/mock-data';

describe('icons', () => {
  it('draws every icon with currentColor so it follows the theme, and fetches nothing', () => {
    for (const [name, svg] of Object.entries(ICONS)) {
      expect(svg, name).toMatch(/^<svg [^>]*viewBox="0 0 24 24"/);
      expect(svg, name).toContain('currentColor');
      expect(svg.replace('http://www.w3.org/2000/svg', ''), name).not.toMatch(/https?:|<script|on\w+=/);
    }
  });

  it('has an icon for every source kind', () => {
    for (const k of SOURCE_KINDS) expect(ICONS[kindIcon(k.id)], k.id).toBeDefined();
  });

  it('registers each icon in the sp namespace at start-up', async () => {
    TestBed.configureTestingModule({ providers: [provideIcons()] });
    const registry = TestBed.inject(MatIconRegistry);
    const svg = await firstValueFrom(registry.getNamedSvgIcon('refresh', 'sp'));
    expect(svg.getAttribute('viewBox')).toBe('0 0 24 24');
    await expect(firstValueFrom(registry.getNamedSvgIcon('nope', 'sp'))).rejects.toBeDefined();
  });
});
