import { TestBed } from '@angular/core/testing';
import { MAT_DATE_FORMATS, MAT_RIPPLE_GLOBAL_OPTIONS } from '@angular/material/core';
import { MAT_SLIDE_TOGGLE_DEFAULT_OPTIONS } from '@angular/material/slide-toggle';
import { MAT_FORM_FIELD_DEFAULT_OPTIONS } from '@angular/material/form-field';
import { MatIconRegistry } from '@angular/material/icon';
import { firstValueFrom } from 'rxjs';
import { describe, expect, it } from 'vitest';
import { provideShellCore } from '../../shell/src/commands';
import { providePaddockMaterial } from '../../material/src/provide';

describe('providePaddockMaterial', () => {
  it('makes form fields outlined with an always-floated label', () => {
    TestBed.configureTestingModule({ providers: [providePaddockMaterial()] });
    const options = TestBed.inject(MAT_FORM_FIELD_DEFAULT_OPTIONS);
    expect(options.appearance).toBe('outline');
    expect(options.floatLabel).toBe('always');
  });

  it('gives the calendar a short month label, so its first row is not upper-cased', () => {
    TestBed.configureTestingModule({ providers: [providePaddockMaterial()] });
    expect(TestBed.inject(MAT_DATE_FORMATS).display.monthLabel).toEqual({ month: 'short' });
  });

  it('hides the icon in the slide toggle handle', () => {
    TestBed.configureTestingModule({ providers: [providePaddockMaterial()] });
    expect(TestBed.inject(MAT_SLIDE_TOGGLE_DEFAULT_OPTIONS).hideIcon).toBe(true);
  });

  it('turns ripples off globally (the state layer is the feedback)', () => {
    TestBed.configureTestingModule({ providers: [providePaddockMaterial()] });
    expect(TestBed.inject(MAT_RIPPLE_GLOBAL_OPTIONS)?.disabled).toBe(true);
  });

  it('registers the pd: icon set with the icon registry (literals, nothing fetched)', async () => {
    TestBed.configureTestingModule({ providers: [providePaddockMaterial()] });
    const svg = await firstValueFrom(TestBed.inject(MatIconRegistry).getNamedSvgIcon('refresh', 'pd'));
    expect(svg.getAttribute('viewBox')).toBe('0 0 24 24');
  });

  it('registers the pd: icon set in every shell window through the shell core (main, dialog and pop-out)', async () => {
    TestBed.configureTestingModule({ providers: [provideShellCore()] });
    const svg = await firstValueFrom(TestBed.inject(MatIconRegistry).getNamedSvgIcon('home', 'pd'));
    expect(svg.querySelector('path')).not.toBeNull();
  });
});
