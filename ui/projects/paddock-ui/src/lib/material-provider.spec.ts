import { TestBed } from '@angular/core/testing';
import { MAT_DATE_FORMATS, MAT_RIPPLE_GLOBAL_OPTIONS } from '@angular/material/core';
import { MAT_SLIDE_TOGGLE_DEFAULT_OPTIONS } from '@angular/material/slide-toggle';
import { MAT_FORM_FIELD_DEFAULT_OPTIONS } from '@angular/material/form-field';
import { describe, expect, it } from 'vitest';
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
});
