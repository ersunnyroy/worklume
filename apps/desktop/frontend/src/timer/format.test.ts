import i18next from 'i18next';
import { describe, expect, it } from 'vitest';
import { clock, isoDuration, spokenDuration, timeOfDay } from './format';

describe('format', () => {
  it.each([
    [0, '0:00:00', 'PT0H0M0S'],
    [59.9, '0:00:59', 'PT0H0M59S'],
    [3_723, '1:02:03', 'PT1H2M3S'],
    [36_000, '10:00:00', 'PT10H0M0S'],
    [-5, '0:00:00', 'PT0H0M0S'],
  ])('shows %s seconds as %s', (seconds, expected, iso) => {
    expect(clock(seconds)).toBe(expected);
    expect(isoDuration(seconds)).toBe(iso);
  });

  it('speaks durations in minutes, then hours and minutes', () => {
    const t = i18next.t.bind(i18next);
    expect(spokenDuration(59, t)).toBe('0 min');
    expect(spokenDuration(12 * 60 + 30, t)).toBe('12 min');
    expect(spokenDuration(2 * 3600 + 5 * 60, t)).toBe('2 h 5 min');
  });

  it('shows times of day in the member’s language', () => {
    // 2026-09-21 14:05 UTC; the test runs in UTC.
    expect(timeOfDay(Date.UTC(2026, 8, 21, 14, 5), 'en-GB')).toBe('14:05');
  });
});
