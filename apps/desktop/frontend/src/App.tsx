import { useTranslation } from 'react-i18next';
import { About } from './About';
import { DeviceLimits } from './DeviceLimits';
import { useAgent } from './ipc/use-agent';
import { useDeviceCapabilities } from './ipc/use-device-capabilities';
import { TimerScreen } from './timer/TimerScreen';
import { Unavailable } from './Unavailable';

/**
 * The agent's window: the timer, or why there is none yet; what this desktop doesn't let it
 * measure fully; and the about section.
 */
export function App() {
  const { t } = useTranslation();
  const agent = useAgent();
  const capabilities = useDeviceCapabilities();
  const { state } = agent;

  return (
    <div className="app">
      <header>
        <h1>{t('app.name')}</h1>
      </header>
      <main>
        {(state === null || state.kind === 'starting') && (
          <p role="status">{t('agent.starting')}</p>
        )}
        {state?.kind === 'unavailable' && <Unavailable reason={state.reason} />}
        {state?.kind === 'ready' && (
          <TimerScreen
            timer={state.timer}
            catalog={agent.catalog}
            error={agent.error}
            run={agent.run}
          />
        )}
        <DeviceLimits capabilities={capabilities} />
      </main>
      <About />
    </div>
  );
}
