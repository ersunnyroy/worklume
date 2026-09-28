/** A union switch that forgets a member must be reported. */
type TimerState = 'running' | 'paused' | 'stopped';

export function describeState(state: TimerState): string {
  switch (state) {
    case 'running':
      return 'Tracking time';
    case 'paused':
      return 'Paused';
  }
}
