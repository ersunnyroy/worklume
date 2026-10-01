import i18next from 'i18next';
import { describe, expect, it } from 'vitest';
import { describePlatform } from './describe-platform';

describe('describePlatform', () => {
  it.each([
    [{ os: 'macos', linux_session: null }, 'macOS'],
    [{ os: 'windows', linux_session: null }, 'Windows'],
    [{ os: 'linux', linux_session: 'x11' }, 'Linux (X11)'],
    [{ os: 'linux', linux_session: 'unknown' }, 'Linux (unknown display server)'],
  ] as const)('describes %j as %s', (platform, expected) => {
    expect(describePlatform(platform, i18next.t.bind(i18next))).toBe(expected);
  });
});
