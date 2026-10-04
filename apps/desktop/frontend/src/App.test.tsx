import { act, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { App } from './App';
import {
  commands,
  events,
  type AgentStateDto,
  type CapabilitiesDto,
  type CatalogDto,
  type TimerViewDto,
} from './ipc/bindings.gen';
import { accessibilityViolations } from './test/axe';

vi.mock('./ipc/bindings.gen', () => ({
  commands: {
    agentStatus: vi.fn(),
    agentState: vi.fn(),
    workCatalog: vi.fn(),
    timerStart: vi.fn(),
    timerSwitch: vi.fn(),
    timerStop: vi.fn(),
    timerResume: vi.fn(),
    breakStart: vi.fn(),
    breakEnd: vi.fn(),
    idleAnswer: vi.fn(),
    deviceCapabilities: vi.fn(),
    screenshotsAskAgain: vi.fn(),
  },
  events: {
    agentChanged: { listen: vi.fn() },
    capabilitiesChanged: { listen: vi.fn() },
  },
}));

const mocked = vi.mocked(commands);
const listen = vi.mocked(events.agentChanged.listen);
const listenCapabilities = vi.mocked(events.capabilitiesChanged.listen);

const WEBSITE = '0192f000-0000-7000-8000-000000000001';
const SUPPORT = '0192f000-0000-7000-8000-000000000002';
const CHECKOUT = '0192f000-0000-7000-8000-000000000011';
const LUNCH = '0192f000-0000-7000-8000-000000000021';
const COFFEE = '0192f000-0000-7000-8000-000000000022';

const catalog: CatalogDto = {
  projects: [
    { id: WEBSITE, name: 'Website', tasks: [{ id: CHECKOUT, name: 'Checkout' }] },
    { id: SUPPORT, name: 'Support', tasks: [] },
  ],
  break_policies: [
    { id: LUNCH, name: 'Lunch' },
    { id: COFFEE, name: 'Coffee' },
  ],
};

const nothing = { start: false, switch: false, stop: false, start_break: false, end_break: false };

function timer(overrides: Partial<TimerViewDto> = {}): TimerViewDto {
  return {
    status: { kind: 'stopped' },
    work: null,
    today_seconds: 0,
    question: null,
    allowed: { ...nothing, start: true, start_break: true },
    ...overrides,
  };
}

const running = timer({
  status: { kind: 'running' },
  work: { project_id: WEBSITE, project_name: 'Website', task_id: null, task_name: null },
  today_seconds: 90,
  allowed: { ...nothing, switch: true, stop: true, start_break: true },
});

function ready(view: TimerViewDto): AgentStateDto {
  return { kind: 'ready', timer: view };
}

let emit: (state: AgentStateDto) => void = () => undefined;

/** Renders the app with the agent in `state`, and waits for the first answer. */
async function renderApp(state: AgentStateDto, withCatalog: CatalogDto = catalog) {
  mocked.agentState.mockResolvedValue(state);
  mocked.workCatalog.mockResolvedValue(withCatalog);
  const view = render(<App />);
  await screen.findByRole('heading', { level: 1, name: 'Worklume' });
  if (state.kind === 'ready') {
    await screen.findByText('Today');
  }
  // Let the catalog and the about section arrive.
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
  return view;
}

let pushCapabilities: (capabilities: CapabilitiesDto) => void = () => undefined;

describe('App', () => {
  beforeEach(() => {
    mocked.agentStatus.mockResolvedValue({
      version: '0.1.0',
      platform: { os: 'macos', linux_session: null },
    });
    listen.mockImplementation((handler) => {
      emit = (state) => {
        act(() => {
          handler({ event: 'agent-changed', id: 1, payload: state });
        });
      };
      return Promise.resolve(() => undefined);
    });
    mocked.deviceCapabilities.mockResolvedValue(null);
    listenCapabilities.mockImplementation((handler) => {
      pushCapabilities = (capabilities) => {
        act(() => {
          handler({ event: 'capabilities-changed', id: 2, payload: capabilities });
        });
      };
      return Promise.resolve(() => undefined);
    });
  });

  it('shows the agent starting, then why it cannot track', async () => {
    await renderApp({ kind: 'starting' });
    expect(await screen.findByRole('status')).toHaveTextContent('Starting…');

    emit({ kind: 'unavailable', reason: 'credential_store' });
    const alert = await screen.findByRole('alert');
    expect(alert).toHaveTextContent("Worklume can't track time on this device");
    expect(alert).toHaveTextContent('Your system keychain is locked or unavailable.');
    expect(await accessibilityViolations()).toEqual([]);
  });

  it('follows the agent: its events replace what is shown', async () => {
    await renderApp(ready(timer()));
    emit(ready({ ...running, today_seconds: 3_723 }));
    expect(screen.getByRole('status')).toHaveTextContent('Tracking');
    expect(screen.getByText('1:02:03')).toHaveAttribute('datetime', 'PT1H2M3S');
    expect(screen.getByText('Website', { selector: '.project' })).toBeInTheDocument();
  });

  it('starts the timer on the picked project and task, by keyboard alone', async () => {
    const user = userEvent.setup();
    mocked.timerStart.mockResolvedValue({ status: 'ok', data: running });
    await renderApp(ready(timer()));

    await user.tab();
    const project = screen.getByLabelText('Project');
    expect(project).toHaveFocus();
    await user.selectOptions(project, 'Website');
    await user.tab();
    const task = screen.getByLabelText('Task');
    expect(task).toHaveFocus();
    await user.selectOptions(task, 'Checkout');
    await user.tab();
    expect(screen.getByRole('button', { name: 'Start timer' })).toHaveFocus();
    await user.keyboard('{Enter}');

    expect(mocked.timerStart).toHaveBeenCalledWith(WEBSITE, CHECKOUT);
    expect(await screen.findByRole('status')).toHaveTextContent('Tracking');
    expect(screen.queryByRole('button', { name: 'Start timer' })).not.toBeInTheDocument();
  });

  it('a project without tasks starts with no task', async () => {
    const user = userEvent.setup();
    mocked.timerStart.mockResolvedValue({ status: 'ok', data: running });
    await renderApp(ready(timer()));

    await user.selectOptions(screen.getByLabelText('Project'), 'Support');
    expect(screen.queryByLabelText('Task')).not.toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'Start timer' }));
    expect(mocked.timerStart).toHaveBeenCalledWith(SUPPORT, null);
  });

  it('says why a command did nothing', async () => {
    const user = userEvent.setup();
    mocked.timerStart.mockResolvedValue({ status: 'error', error: 'unknown_project' });
    await renderApp(ready(timer()));

    await user.click(screen.getByRole('button', { name: 'Start timer' }));
    expect(await screen.findByRole('alert')).toHaveTextContent(
      "That project isn't available to you any more.",
    );
  });

  it('without projects, says where they come from and offers no start', async () => {
    await renderApp(ready(timer({ allowed: nothing })), { projects: [], break_policies: [] });

    expect(screen.getByText(/No projects yet/)).toBeInTheDocument();
    expect(screen.queryByRole('button')).not.toBeInTheDocument();
    expect(await accessibilityViolations()).toEqual([]);
  });

  it('switches only to different work, and stops', async () => {
    const user = userEvent.setup();
    mocked.timerSwitch.mockResolvedValue({ status: 'ok', data: running });
    mocked.timerStop.mockResolvedValue({ status: 'ok', data: timer() });
    await renderApp(ready(running));

    const switchButton = screen.getByRole('button', { name: 'Switch' });
    expect(switchButton).toBeDisabled();
    await user.selectOptions(screen.getByLabelText('Project'), 'Support');
    expect(switchButton).toBeEnabled();
    await user.click(switchButton);
    expect(mocked.timerSwitch).toHaveBeenCalledWith(SUPPORT, null);

    await user.click(screen.getByRole('button', { name: 'Stop timer' }));
    expect(mocked.timerStop).toHaveBeenCalled();
    expect(await screen.findByRole('status')).toHaveTextContent('Not tracking');
    expect(await accessibilityViolations()).toEqual([]);
  });

  it('takes and ends breaks under a chosen policy', async () => {
    const user = userEvent.setup();
    const onBreak = timer({
      status: { kind: 'on_break', policy_name: 'Coffee' },
      work: running.work,
      allowed: { ...nothing, end_break: true },
    });
    mocked.breakStart.mockResolvedValue({ status: 'ok', data: onBreak });
    mocked.breakEnd.mockResolvedValue({ status: 'ok', data: running });
    await renderApp(ready(running));

    await user.selectOptions(screen.getByLabelText('Break'), 'Coffee');
    await user.click(screen.getByRole('button', { name: 'Take a break' }));
    expect(mocked.breakStart).toHaveBeenCalledWith(COFFEE);
    expect(await screen.findByRole('status')).toHaveTextContent('On a break: Coffee');
    expect(screen.queryByRole('button', { name: 'Stop timer' })).not.toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: 'End break' }));
    expect(mocked.breakEnd).toHaveBeenCalled();
    expect(await screen.findByRole('status')).toHaveTextContent('Tracking');
  });

  describe('the idle question', () => {
    const idle = timer({
      status: { kind: 'idle' },
      work: running.work,
      today_seconds: 900,
      question: {
        kind: 'idle',
        since_ms: Date.UTC(2026, 8, 21, 14, 5),
        idle_seconds: 12 * 60,
        paused: false,
      },
      allowed: nothing,
    });

    it('takes focus, explains itself and waits for an answer', async () => {
      const user = userEvent.setup();
      await renderApp(ready(idle));

      const dialog = screen.getByRole('alertdialog', { name: 'You were away' });
      expect(dialog).toHaveAccessibleDescription(
        /No keyboard or mouse input since 2:05\sPM, 12 min ago\. What should happen to that time\?/,
      );
      const keep = within(dialog).getByRole('button', { name: 'Keep it' });
      expect(keep).toHaveFocus();

      // Escape doesn't dismiss it: only an answer does.
      await user.keyboard('{Escape}');
      expect(dialog).toHaveAttribute('open');
      await user.tab();
      expect(within(dialog).getByRole('button', { name: 'Discard it' })).toHaveFocus();
      await user.tab();
      expect(
        within(dialog).getByRole('button', { name: 'Discard it and stop the timer' }),
      ).toHaveFocus();
      await user.tab();
      expect(within(dialog).getByLabelText('Project to move it to')).toHaveFocus();
      expect(await accessibilityViolations()).toEqual([]);
    });

    it.each([
      ['Keep it', { kind: 'keep' }],
      ['Discard it', { kind: 'discard' }],
      ['Discard it and stop the timer', { kind: 'stop' }],
    ] as const)('answers "%s"', async (button, answer) => {
      const user = userEvent.setup();
      mocked.idleAnswer.mockResolvedValue({ status: 'ok', data: running });
      await renderApp(ready(idle));

      await user.click(screen.getByRole('button', { name: button }));
      expect(mocked.idleAnswer).toHaveBeenCalledWith(answer);
      expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument();
    });

    it('moves the time to another project', async () => {
      const user = userEvent.setup();
      mocked.idleAnswer.mockResolvedValue({ status: 'error', error: 'failed' });
      await renderApp(ready(idle));

      await user.selectOptions(screen.getByLabelText('Project to move it to'), 'Support');
      await user.click(screen.getByRole('button', { name: 'Move it' }));
      expect(mocked.idleAnswer).toHaveBeenCalledWith({ kind: 'reassign', project_id: SUPPORT });
      // A failure is shown inside the dialog, where the member is.
      const dialog = screen.getByRole('alertdialog');
      expect(await within(dialog).findByRole('alert')).toHaveTextContent(
        "Worklume couldn't record that.",
      );
    });

    it('after a sleep, says the answer also resumes the timer', async () => {
      await renderApp(
        ready({
          ...idle,
          status: { kind: 'paused', reason: 'sleep' },
          question: {
            kind: 'idle',
            since_ms: Date.UTC(2026, 8, 21, 14, 5),
            idle_seconds: null,
            paused: true,
          },
        }),
      );
      const dialog = screen.getByRole('alertdialog');
      expect(dialog).toHaveTextContent('because your computer slept or your screen was locked');
      // After a reboot the agent can't say how long ago that was.
      expect(dialog).toHaveTextContent(/No keyboard or mouse input since 2:05\sPM\./);
    });
  });

  it('after a long sleep, asks whether to resume', async () => {
    const user = userEvent.setup();
    const paused = timer({
      status: { kind: 'paused', reason: 'sleep' },
      work: running.work,
      question: { kind: 'resume' },
      allowed: { ...nothing, stop: true },
    });
    mocked.timerResume.mockResolvedValue({ status: 'ok', data: running });
    await renderApp(ready(paused));

    const dialog = screen.getByRole('alertdialog', { name: 'Welcome back' });
    expect(within(dialog).getByRole('button', { name: 'Resume timer' })).toHaveFocus();
    expect(await accessibilityViolations()).toEqual([]);
    await user.keyboard('{Enter}');
    expect(mocked.timerResume).toHaveBeenCalled();
    expect(await screen.findByRole('status')).toHaveTextContent('Tracking');
  });

  it('explains what this desktop does not let it measure, as the agent reports it', async () => {
    await renderApp(ready(timer({})));
    expect(screen.queryByRole('region', { name: 'On this desktop' })).not.toBeInTheDocument();

    pushCapabilities({
      features: [
        { feature: 'screenshots', status: 'denied', reasons: ['consent_declined'] },
        { feature: 'idle', status: 'granted', reasons: [] },
      ],
    });
    const limits = screen.getByRole('region', { name: 'On this desktop' });
    expect(limits).toHaveTextContent('Screenshots Off');
    expect(limits).toHaveTextContent('You declined screen sharing, so no screenshots are taken.');
    expect(await accessibilityViolations()).toEqual([]);

    // Asking again goes to the agent, whose next report shows the screenshots waiting.
    mocked.screenshotsAskAgain.mockResolvedValue(true);
    await userEvent.setup().click(within(limits).getByRole('button', { name: 'Ask me again' }));
    expect(mocked.screenshotsAskAgain).toHaveBeenCalledTimes(1);
    pushCapabilities({
      features: [{ feature: 'screenshots', status: 'pending', reasons: ['consent_not_asked'] }],
    });
    expect(screen.getByRole('region', { name: 'On this desktop' })).toHaveTextContent(
      'Screenshots Waiting',
    );
  });
});
