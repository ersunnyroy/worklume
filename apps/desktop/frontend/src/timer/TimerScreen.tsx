import { useTranslation } from 'react-i18next';
import {
  commands,
  type CatalogDto,
  type CommandError,
  type TimerViewDto,
} from '../ipc/bindings.gen';
import type { Agent } from '../ipc/use-agent';
import { BreakStarter } from './BreakStarter';
import { clock, isoDuration } from './format';
import { Question } from './Questions';
import { WorkPicker } from './WorkPicker';

interface TimerScreenProps {
  readonly timer: TimerViewDto;
  readonly catalog: CatalogDto;
  readonly error: CommandError | null;
  readonly run: Agent['run'];
}

/**
 * The timer: what it is doing and on what, today's total, and the actions the agent allows
 * now. Every rule is the agent's; this only shows its view.
 */
export function TimerScreen({ timer, catalog, error, run }: TimerScreenProps) {
  const { t } = useTranslation();
  const { allowed, work } = timer;

  return (
    <section className="timer" aria-labelledby="timer-heading">
      <h2 id="timer-heading" className="visually-hidden">
        {t('timer.heading')}
      </h2>
      <p role="status" className="state">
        {statusText(timer, t)}
      </p>
      {work !== null && (
        <p className="work">
          <span className="project">{work.project_name ?? t('timer.unknownProject')}</span>
          {work.task_id !== null && (
            <span className="task">{work.task_name ?? t('timer.unknownTask')}</span>
          )}
        </p>
      )}
      <p className="today">
        <span>{t('timer.today')}</span>{' '}
        <time dateTime={isoDuration(timer.today_seconds)}>{clock(timer.today_seconds)}</time>
      </p>
      {timer.question === null && error !== null && (
        <p role="alert" className="error">
          {t(`timer.errors.${error}`)}
        </p>
      )}
      {(allowed.start || allowed.switch) && (
        <WorkPicker
          catalog={catalog}
          current={work}
          mode={allowed.start ? 'start' : 'switch'}
          run={run}
        />
      )}
      {timer.status.kind === 'stopped' && catalog.projects.length === 0 && (
        <p className="hint">{t('timer.noProjects')}</p>
      )}
      <div className="actions">
        {allowed.stop && (
          <button type="button" onClick={() => void run(commands.timerStop)}>
            {t('timer.stop')}
          </button>
        )}
        {allowed.start_break && <BreakStarter policies={catalog.break_policies} run={run} />}
        {allowed.end_break && (
          <button type="button" onClick={() => void run(commands.breakEnd)}>
            {t('timer.endBreak')}
          </button>
        )}
      </div>
      {timer.question !== null && (
        <Question question={timer.question} catalog={catalog} error={error} run={run} />
      )}
    </section>
  );
}

function statusText(timer: TimerViewDto, t: ReturnType<typeof useTranslation>['t']): string {
  const { status } = timer;
  switch (status.kind) {
    case 'stopped':
      return t('timer.status.stopped');
    case 'running':
      return t('timer.status.running');
    case 'idle':
      return t('timer.status.idle');
    case 'paused':
      return status.reason === 'sleep'
        ? t('timer.status.pausedSleep')
        : t('timer.status.pausedLocked');
    case 'on_break':
      return status.policy_name === null
        ? t('timer.status.onBreak')
        : t('timer.status.onBreakNamed', { policy: status.policy_name });
  }
}
