import { useTranslation } from 'react-i18next';
import { About } from './About';
import { useAgent } from './ipc/use-agent';
import { TimerScreen } from './timer/TimerScreen';
import { Unavailable } from './Unavailable';

/** The agent's window: the timer, or why there is none yet, and the about section. */
export function App() {
  const { t } = useTranslation();
  const agent = useAgent();
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
      </main>
      <About />
    </div>
  );
}
