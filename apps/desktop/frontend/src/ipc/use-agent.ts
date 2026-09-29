import { useCallback, useEffect, useState } from 'react';
import {
  commands,
  events,
  type AgentStateDto,
  type CatalogDto,
  type CommandError,
  type TimerViewDto,
} from './bindings.gen';

/** A timer command's typed result, as the generated bindings return it. */
export type CommandResult =
  { status: 'ok'; data: TimerViewDto } | { status: 'error'; error: CommandError };

export interface Agent {
  /** What the agent shows, or `null` until it first answers. */
  readonly state: AgentStateDto | null;
  readonly catalog: CatalogDto;
  /** Why the last command did nothing, until the next one. */
  readonly error: CommandError | null;
  /** Runs a timer command and shows the timer it returns. */
  readonly run: (command: () => Promise<CommandResult>) => Promise<void>;
}

const emptyCatalog: CatalogDto = { projects: [], break_policies: [] };

/**
 * The agent as the UI sees it: its state, pushed by the `agent-changed` event, and the catalog
 * to pick from. The UI keeps no tracking state of its own; every change comes from the agent.
 */
export function useAgent(): Agent {
  const [state, setState] = useState<AgentStateDto | null>(null);
  const [catalog, setCatalog] = useState<CatalogDto>(emptyCatalog);
  const [error, setError] = useState<CommandError | null>(null);
  const ready = state?.kind === 'ready';

  useEffect(() => {
    let active = true;
    const unlisten = events.agentChanged.listen((event) => {
      if (active) {
        setState(event.payload);
      }
    });
    commands.agentState().then(
      (current) => {
        if (active) {
          // An event may already have delivered something newer.
          setState((previous) => previous ?? current);
        }
      },
      () => undefined,
    );
    return () => {
      active = false;
      void unlisten.then((stop) => {
        stop();
      });
    };
  }, []);

  // The catalog is empty until the agent is ready, so it is read again then.
  useEffect(() => {
    if (!ready) {
      return undefined;
    }
    let active = true;
    commands.workCatalog().then(
      (current) => {
        if (active) {
          setCatalog(current);
        }
      },
      () => undefined,
    );
    return () => {
      active = false;
    };
  }, [ready]);

  const run = useCallback(async (command: () => Promise<CommandResult>) => {
    const result = await command();
    if (result.status === 'ok') {
      setError(null);
      setState({ kind: 'ready', timer: result.data });
    } else {
      setError(result.error);
    }
  }, []);

  return { state, catalog, error, run };
}
