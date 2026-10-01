import { useId, useState, type SyntheticEvent } from 'react';
import { useTranslation } from 'react-i18next';
import { commands, type NamedDto } from '../ipc/bindings.gen';
import type { Agent } from '../ipc/use-agent';

interface BreakStarterProps {
  readonly policies: readonly NamedDto[];
  readonly run: Agent['run'];
}

/** Takes a break, under the one break policy or a chosen one. */
export function BreakStarter({ policies, run }: BreakStarterProps) {
  const { t } = useTranslation();
  const id = useId();
  const [picked, setPicked] = useState<string | null>(null);
  const policy = policies.find((candidate) => candidate.id === picked) ?? policies[0];
  if (policy === undefined) {
    return null;
  }

  const submit = (event: SyntheticEvent) => {
    event.preventDefault();
    void run(() => commands.breakStart(policy.id));
  };

  return (
    <form className="break" onSubmit={submit}>
      {policies.length > 1 && (
        <>
          <label htmlFor={`${id}-policy`}>{t('timer.breakPolicy')}</label>
          <select
            id={`${id}-policy`}
            value={policy.id}
            onChange={(event) => {
              setPicked(event.target.value);
            }}
          >
            {policies.map((candidate) => (
              <option key={candidate.id} value={candidate.id}>
                {candidate.name}
              </option>
            ))}
          </select>
        </>
      )}
      <button type="submit">{t('timer.startBreak')}</button>
    </form>
  );
}
