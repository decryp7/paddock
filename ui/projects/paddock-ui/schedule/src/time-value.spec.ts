import { TestBed } from '@angular/core/testing';
import { DateAdapter, MAT_DATE_FORMATS, MAT_DATE_LOCALE } from '@angular/material/core';
import { afterEach, describe, expect, it } from 'vitest';
import { dateToTime, TIME_PROVIDERS, timeOptions, timeToDate } from './time-value';

// Node (the test runner) reads `TZ` again whenever it is assigned.
const env = (globalThis as unknown as { process: { env: Record<string, string | undefined> } }).process.env;
const originalZone = env['TZ'];
const setZone = (zone: string): void => {
  env['TZ'] = zone;
};
afterEach(() => {
  if (originalZone === undefined) delete env['TZ'];
  else env['TZ'] = originalZone;
});

/** Zones with clock changes at midnight-adjacent hours, a half-hour shift, the southern summer and none at all. */
const ZONES = ['America/New_York', 'Europe/Berlin', 'Australia/Lord_Howe', 'America/Sao_Paulo', 'Asia/Kolkata', 'UTC'];

describe('timeToDate and dateToTime', () => {
  it('round-trips every minute of the day in every zone, including 00:00 and 23:59', () => {
    for (const zone of ZONES) {
      setZone(zone);
      for (let m = 0; m < 24 * 60; m++) {
        const time = `${String(Math.floor(m / 60)).padStart(2, '0')}:${String(m % 60).padStart(2, '0')}`;
        expect(dateToTime(timeToDate(time)), `${zone} ${time}`).toBe(time);
      }
    }
  });

  it('keeps the wall-clock time whatever day a picked Date carries, also on the days the clocks change', () => {
    setZone('America/New_York');
    // The picker keeps our date part and sets the hours and minutes; here the day is the spring-forward and fall-back day.
    const springDay = new Date(2024, 2, 10, 3, 30);
    expect(dateToTime(springDay)).toBe('03:30');
    expect(dateToTime(new Date(2024, 2, 10, 1, 59))).toBe('01:59');
    const fallDay = new Date(2024, 10, 3, 1, 30);
    expect(dateToTime(fallDay)).toBe('01:30');
    // A time that does not exist that day (02:30 on the spring-forward day) is moved by the platform to 03:30: the documented limit.
    expect(dateToTime(new Date(2024, 2, 10, 2, 30))).toBe('03:30');
    // The reference day itself is unaffected.
    expect(timeToDate('02:30')!.getHours()).toBe(2);
  });

  it('reads the local hours and minutes, never UTC', () => {
    setZone('Asia/Kolkata');
    expect(dateToTime(new Date('2026-10-28T18:30:00Z'))).toBe('00:00');
  });

  it('refuses anything but a 24-hour HH:MM', () => {
    for (const bad of ['', '9:00', '24:00', '12:60', '12:5', '12:30:00', 'noon', null, undefined]) expect(timeToDate(bad)).toBeNull();
    expect(dateToTime(null)).toBeNull();
    expect(dateToTime(undefined)).toBeNull();
    expect(dateToTime(new Date(Number.NaN))).toBeNull();
  });
});

describe('timeOptions', () => {
  it('lists the whole reference day, 48 half hours by default, labelled in the locale', () => {
    setZone('America/New_York');
    TestBed.configureTestingModule({ providers: [TIME_PROVIDERS, { provide: MAT_DATE_LOCALE, useValue: 'en-US' }] });
    const adapter = TestBed.inject<DateAdapter<Date>>(DateAdapter);
    const formats = TestBed.inject(MAT_DATE_FORMATS);
    const options = timeOptions(adapter, formats);
    expect(options).toHaveLength(48);
    expect(options[0]!.label).toBe('00:00');
    expect(options[47]!.label).toBe('23:30');
    expect(options.map((o) => dateToTime(o.value))).toEqual(Array.from({ length: 48 }, (_, i) => `${String(Math.floor(i / 2)).padStart(2, '0')}:${i % 2 ? '30' : '00'}`));
    expect(timeOptions(adapter, formats, 15)).toHaveLength(96);
  });
});
