import { render, screen } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { About } from './About';
import { commands } from './ipc/bindings.gen';
import { accessibilityViolations } from './test/axe';

vi.mock('./ipc/bindings.gen', () => ({ commands: { agentStatus: vi.fn() } }));
const agentStatus = vi.mocked(commands.agentStatus);

describe('About', () => {
  beforeEach(() => {
    agentStatus.mockResolvedValue({
      version: '0.1.0',
      platform: { os: 'linux', linux_session: 'wayland' },
    });
  });

  it('explains that the agent keeps running from the tray', () => {
    render(<About />);

    expect(screen.getByRole('heading', { level: 2, name: 'About' })).toBeVisible();
    expect(screen.getByText(/Closing this window keeps Worklume running/)).toBeInTheDocument();
  });

  it('shows the version and platform reported by the agent', async () => {
    render(<About />);

    expect(await screen.findByText('0.1.0')).toBeInTheDocument();
    expect(screen.getByText('Linux (Wayland)')).toBeInTheDocument();
    expect(agentStatus).toHaveBeenCalledTimes(1);
  });

  it('says so when the agent cannot answer', async () => {
    agentStatus.mockRejectedValue(new Error('IPC unavailable'));

    render(<About />);

    expect(await screen.findByRole('alert')).toHaveTextContent('Agent details are unavailable.');
  });

  it('has no detectable accessibility violations', async () => {
    const { container } = render(<About />);
    await screen.findByText('0.1.0');

    expect(await accessibilityViolations(container)).toEqual([]);
  });
});
