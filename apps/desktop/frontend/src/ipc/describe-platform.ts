import type { Platform } from './bindings.gen';

const osNames = { macos: 'macOS', windows: 'Windows', linux: 'Linux' } as const;
const sessionNames = { x11: 'X11', wayland: 'Wayland', unknown: 'unknown display server' } as const;

/** "macOS", "Windows" or "Linux (Wayland)". Localisation arrives with E09-S09. */
export function describePlatform(platform: Platform): string {
  const os = osNames[platform.os];
  return platform.linux_session === null ? os : `${os} (${sessionNames[platform.linux_session]})`;
}
