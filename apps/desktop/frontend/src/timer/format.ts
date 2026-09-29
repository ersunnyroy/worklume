import type { TFunction } from 'i18next';

/** Seconds as a clock reading, `1:02:03`, the way timers show time in every language. */
export function clock(seconds: number): string {
  const whole = Math.max(0, Math.floor(seconds));
  const hours = Math.floor(whole / 3600);
  const minutes = Math.floor((whole % 3600) / 60);
  const rest = whole % 60;
  return `${String(hours)}:${String(minutes).padStart(2, '0')}:${String(rest).padStart(2, '0')}`;
}

/** Seconds as an ISO 8601 duration, `PT1H2M3S`, for `<time dateTime>`. */
export function isoDuration(seconds: number): string {
  const whole = Math.max(0, Math.floor(seconds));
  return `PT${String(Math.floor(whole / 3600))}H${String(Math.floor((whole % 3600) / 60))}M${String(whole % 60)}S`;
}

/** Seconds as words, "5 min" or "2 h 5 min", rounded down to the minute. */
export function spokenDuration(seconds: number, t: TFunction): string {
  const minutes = Math.floor(Math.max(0, seconds) / 60);
  if (minutes < 60) {
    return t('duration.minutes', { minutes });
  }
  return t('duration.hoursMinutes', { hours: Math.floor(minutes / 60), minutes: minutes % 60 });
}

/** A moment as the member's local time of day, "2:05 PM" or "14:05". */
export function timeOfDay(ms: number, language: string): string {
  return new Intl.DateTimeFormat(language, { hour: 'numeric', minute: '2-digit' }).format(ms);
}
