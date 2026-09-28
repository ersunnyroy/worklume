import { useEffect, useState } from 'react';
import { commands, type AgentStatus } from './bindings.gen';

export type AgentStatusState =
  | { readonly state: 'loading' }
  | { readonly state: 'ready'; readonly status: AgentStatus }
  | { readonly state: 'failed' };

/** Reads the agent's version and platform once, through the typed `agent_status` command. */
export function useAgentStatus(): AgentStatusState {
  const [value, setValue] = useState<AgentStatusState>({ state: 'loading' });

  useEffect(() => {
    let active = true;
    commands.agentStatus().then(
      (status) => {
        if (active) {
          setValue({ state: 'ready', status });
        }
      },
      () => {
        if (active) {
          setValue({ state: 'failed' });
        }
      },
    );
    return () => {
      active = false;
    };
  }, []);

  return value;
}
