import { useId, useState, type SyntheticEvent } from 'react';
import { useTranslation } from 'react-i18next';
import { commands, type CatalogDto, type WorkDto } from '../ipc/bindings.gen';
import type { Agent } from '../ipc/use-agent';

interface WorkPickerProps {
  readonly catalog: CatalogDto;
  /** What the timer runs on now, preselected. */
  readonly current: WorkDto | null;
  readonly mode: 'start' | 'switch';
  readonly run: Agent['run'];
}

/** The project and task to start the timer on, or switch it to. */
export function WorkPicker({ catalog, current, mode, run }: WorkPickerProps) {
  const { t } = useTranslation();
  const id = useId();
  const [picked, setPicked] = useState<{ project: string; task: string } | null>(null);

  const known = (projectId: string | undefined) =>
    catalog.projects.find((project) => project.id === projectId);
  // The member's pick, else the current work, else the first project: never one the catalog
  // no longer has.
  const fallback = known(current?.project_id) ?? catalog.projects[0];
  const project = known(picked?.project) ?? fallback;
  const taskId = picked === null ? (current?.task_id ?? '') : picked.task;
  const task = project?.tasks.find((candidate) => candidate.id === taskId);
  if (project === undefined) {
    return null;
  }
  const unchanged =
    mode === 'switch' &&
    project.id === current?.project_id &&
    (task?.id ?? null) === (current.task_id ?? null);

  const submit = (event: SyntheticEvent) => {
    event.preventDefault();
    const taskOrNull = task?.id ?? null;
    void run(() =>
      mode === 'start'
        ? commands.timerStart(project.id, taskOrNull)
        : commands.timerSwitch(project.id, taskOrNull),
    );
  };

  return (
    <form className="picker" onSubmit={submit}>
      <fieldset>
        <legend>{t('timer.pickerLegend')}</legend>
        <label htmlFor={`${id}-project`}>{t('timer.project')}</label>
        <select
          id={`${id}-project`}
          value={project.id}
          onChange={(event) => {
            setPicked({ project: event.target.value, task: '' });
          }}
        >
          {catalog.projects.map((candidate) => (
            <option key={candidate.id} value={candidate.id}>
              {candidate.name}
            </option>
          ))}
        </select>
        {project.tasks.length > 0 && (
          <>
            <label htmlFor={`${id}-task`}>{t('timer.task')}</label>
            <select
              id={`${id}-task`}
              value={task?.id ?? ''}
              onChange={(event) => {
                setPicked({ project: project.id, task: event.target.value });
              }}
            >
              <option value="">{t('timer.noTask')}</option>
              {project.tasks.map((candidate) => (
                <option key={candidate.id} value={candidate.id}>
                  {candidate.name}
                </option>
              ))}
            </select>
          </>
        )}
      </fieldset>
      <button type="submit" disabled={unchanged}>
        {mode === 'start' ? t('timer.start') : t('timer.switch')}
      </button>
    </form>
  );
}
