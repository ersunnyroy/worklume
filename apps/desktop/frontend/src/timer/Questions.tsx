import { useId, useState, type SyntheticEvent } from 'react';
import { useTranslation } from 'react-i18next';
import {
  commands,
  type CatalogDto,
  type CommandError,
  type QuestionDto,
} from '../ipc/bindings.gen';
import type { Agent } from '../ipc/use-agent';
import { spokenDuration, timeOfDay } from './format';
import { Modal } from './Modal';

interface QuestionProps {
  readonly question: QuestionDto;
  readonly catalog: CatalogDto;
  readonly error: CommandError | null;
  readonly run: Agent['run'];
}

/** The question the agent is asking, as a dialog that stays until it is answered. */
export function Question({ question, catalog, error, run }: QuestionProps) {
  return question.kind === 'idle' ? (
    <IdleQuestion question={question} catalog={catalog} error={error} run={run} />
  ) : (
    <ResumeQuestion error={error} run={run} />
  );
}

function ErrorLine({ error }: { readonly error: CommandError | null }) {
  const { t } = useTranslation();
  return error === null ? null : (
    <p role="alert" className="error">
      {t(`timer.errors.${error}`)}
    </p>
  );
}

function IdleQuestion({
  question,
  catalog,
  error,
  run,
}: QuestionProps & { readonly question: Extract<QuestionDto, { kind: 'idle' }> }) {
  const { t, i18n } = useTranslation();
  const id = useId();
  const [picked, setPicked] = useState<string | null>(null);
  const target = catalog.projects.find((project) => project.id === picked) ?? catalog.projects[0];
  const time = question.since_ms === null ? null : timeOfDay(question.since_ms, i18n.language);

  const reassign = (event: SyntheticEvent) => {
    event.preventDefault();
    if (target !== undefined) {
      void run(() => commands.idleAnswer({ kind: 'reassign', project_id: target.id }));
    }
  };

  return (
    <Modal labelledBy={`${id}-heading`} describedBy={`${id}-body`}>
      <h2 id={`${id}-heading`}>{t('idle.heading')}</h2>
      <div id={`${id}-body`}>
        {time !== null && (
          <p>
            {question.idle_seconds === null
              ? t('idle.sinceOnly', { time })
              : t('idle.since', { time, duration: spokenDuration(question.idle_seconds, t) })}
          </p>
        )}
        {question.paused && <p>{t('idle.paused')}</p>}
        <p>{t('idle.question')}</p>
      </div>
      <ErrorLine error={error} />
      <div className="answers">
        <button
          type="button"
          data-initial-focus
          onClick={() => void run(() => commands.idleAnswer({ kind: 'keep' }))}
        >
          {t('idle.keep')}
        </button>
        <button
          type="button"
          onClick={() => void run(() => commands.idleAnswer({ kind: 'discard' }))}
        >
          {t('idle.discard')}
        </button>
        <button type="button" onClick={() => void run(() => commands.idleAnswer({ kind: 'stop' }))}>
          {t('idle.stop')}
        </button>
      </div>
      {target !== undefined && (
        <form className="reassign" onSubmit={reassign}>
          <label htmlFor={`${id}-project`}>{t('idle.reassignProject')}</label>
          <select
            id={`${id}-project`}
            value={target.id}
            onChange={(event) => {
              setPicked(event.target.value);
            }}
          >
            {catalog.projects.map((project) => (
              <option key={project.id} value={project.id}>
                {project.name}
              </option>
            ))}
          </select>
          <button type="submit">{t('idle.reassign')}</button>
        </form>
      )}
    </Modal>
  );
}

function ResumeQuestion({ error, run }: Omit<QuestionProps, 'question' | 'catalog'>) {
  const { t } = useTranslation();
  const id = useId();

  return (
    <Modal labelledBy={`${id}-heading`} describedBy={`${id}-body`}>
      <h2 id={`${id}-heading`}>{t('resume.heading')}</h2>
      <p id={`${id}-body`}>{t('resume.body')}</p>
      <ErrorLine error={error} />
      <div className="answers">
        <button type="button" data-initial-focus onClick={() => void run(commands.timerResume)}>
          {t('resume.resume')}
        </button>
        <button type="button" onClick={() => void run(commands.timerStop)}>
          {t('resume.stop')}
        </button>
      </div>
    </Modal>
  );
}
