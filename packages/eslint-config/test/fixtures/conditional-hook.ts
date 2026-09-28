/** Hooks called conditionally break React's rules and must be reported. */
function useElapsedSeconds(): number {
  return 0;
}

export function TimerBadge(visible: boolean): number {
  if (visible) {
    return useElapsedSeconds();
  }
  return 0;
}
