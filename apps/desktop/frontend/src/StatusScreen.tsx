import { describePlatform } from './ipc/describe-platform';
import { useAgentStatus } from './ipc/use-agent-status';

/**
 * Placeholder window content until the agent UI (E09-S09) replaces it with the timer, project
 * picker and settings. Localisation arrives with that story.
 */
export function StatusScreen() {
  const agent = useAgentStatus();

  return (
    <main className="status">
      <h1>Worklume</h1>
      <p>Worklume runs from your menu bar or system tray. Closing this window keeps it running.</p>
      <p>Sign-in and time tracking arrive in upcoming releases.</p>
      {agent.state === 'ready' && (
        <dl className="about">
          <dt>Version</dt>
          <dd>{agent.status.version}</dd>
          <dt>Platform</dt>
          <dd>{describePlatform(agent.status.platform)}</dd>
        </dl>
      )}
      {agent.state === 'failed' && <p role="alert">Agent details are unavailable.</p>}
    </main>
  );
}
