import { render, screen } from '@testing-library/react';
import axe from 'axe-core';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { commands } from './ipc/bindings.gen';
import { StatusScreen } from './StatusScreen';

vi.mock('./ipc/bindings.gen', () => ({ commands: { agentStatus: vi.fn() } }));
const agentStatus = vi.mocked(commands.agentStatus);

describe('StatusScreen', () => {
  beforeEach(() => {
    agentStatus.mockResolvedValue({
      version: '0.1.0',
      platform: { os: 'linux', linux_session: 'wayland' },
    });
  });

  it('explains that the agent keeps running from the tray', () => {
    render(<StatusScreen />);

    expect(screen.getByRole('heading', { level: 1, name: 'Worklume' })).toBeVisible();
    expect(screen.getByText(/Closing this window keeps it running/)).toBeInTheDocument();
  });

  it('shows the version and platform reported by the agent', async () => {
    render(<StatusScreen />);

    expect(await screen.findByText('0.1.0')).toBeInTheDocument();
    expect(screen.getByText('Linux (Wayland)')).toBeInTheDocument();
    expect(agentStatus).toHaveBeenCalledTimes(1);
  });

  it('says so when the agent cannot answer', async () => {
    agentStatus.mockRejectedValue(new Error('IPC unavailable'));

    render(<StatusScreen />);

    expect(await screen.findByRole('alert')).toHaveTextContent('Agent details are unavailable.');
  });

  it('has no detectable accessibility violations', async () => {
    const { container } = render(<StatusScreen />);
    await screen.findByText('0.1.0');

    // Colour contrast needs real layout; jsdom cannot compute it.
    const results = await axe.run(container, { rules: { 'color-contrast': { enabled: false } } });
    expect(results.violations.map((violation) => violation.id)).toEqual([]);
  });
});
