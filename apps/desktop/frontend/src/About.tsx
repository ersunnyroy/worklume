import { useTranslation } from 'react-i18next';
import { describePlatform } from './ipc/describe-platform';
import { useAgentStatus } from './ipc/use-agent-status';

/** The agent's version and platform, for support requests. */
export function About() {
  const { t } = useTranslation();
  const agent = useAgentStatus();

  return (
    <footer className="about" aria-labelledby="about-heading">
      <h2 id="about-heading">{t('about.heading')}</h2>
      <p>{t('app.trayNote')}</p>
      {agent.state === 'ready' && (
        <dl>
          <dt>{t('about.version')}</dt>
          <dd>{agent.status.version}</dd>
          <dt>{t('about.platform')}</dt>
          <dd>{describePlatform(agent.status.platform, t)}</dd>
        </dl>
      )}
      {agent.state === 'failed' && <p role="alert">{t('about.unavailable')}</p>}
    </footer>
  );
}
