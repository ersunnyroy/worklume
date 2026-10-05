import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { DeviceLimits } from './DeviceLimits';
import type { CapabilitiesDto, FeatureCapabilityDto } from './ipc/bindings.gen';
import { accessibilityViolations } from './test/axe';

const granted = (feature: FeatureCapabilityDto['feature']): FeatureCapabilityDto => ({
  feature,
  status: 'granted',
  reasons: [],
});

/** GNOME on Wayland at the first login after installing. */
const gnomeFirstLogin: CapabilitiesDto = {
  features: [
    { feature: 'screenshots', status: 'pending', reasons: ['consent_not_asked'] },
    {
      feature: 'activity',
      status: 'reduced',
      reasons: ['estimated_from_idle', 'no_keyboard_mouse_split'],
    },
    granted('idle'),
    { feature: 'apps', status: 'pending', reasons: ['gnome_extension_next_login'] },
    { feature: 'urls', status: 'unsupported', reasons: ['not_implemented'] },
    granted('session_lock'),
  ],
};

describe('DeviceLimits', () => {
  it('shows nothing before the first report or when everything is measured', () => {
    const { container, rerender } = render(
      <DeviceLimits capabilities={null} onAskAgain={vi.fn()} />,
    );
    expect(container).toBeEmptyDOMElement();

    rerender(
      <DeviceLimits
        onAskAgain={vi.fn()}
        capabilities={{
          features: (
            ['screenshots', 'activity', 'idle', 'apps', 'urls', 'session_lock'] as const
          ).map(granted),
        }}
      />,
    );
    expect(container).toBeEmptyDOMElement();
  });

  it('explains each feature that is not fully measured, and only those', () => {
    render(<DeviceLimits capabilities={gnomeFirstLogin} onAskAgain={vi.fn()} />);

    const section = screen.getByRole('region', { name: 'On this desktop' });
    const items = within(section).getAllByRole('listitem');
    expect(items).toHaveLength(4);
    expect(items[0]).toHaveTextContent('Screenshots Waiting');
    expect(items[0]).toHaveTextContent(
      'Your desktop will ask you once to share your screen, when the first screenshot is due.',
    );
    expect(items[1]).toHaveTextContent('Activity Reduced');
    expect(items[1]).toHaveTextContent("Keyboard and mouse aren't counted separately.");
    expect(items[2]).toHaveTextContent('Apps used Waiting');
    expect(items[2]).toHaveTextContent('Starts after you next log in to your desktop.');
    expect(items[3]).toHaveTextContent('Websites visited Not available');
    expect(within(section).queryByText('Idle time')).not.toBeInTheDocument();
    expect(within(section).queryByText('Screen lock')).not.toBeInTheDocument();
  });

  it('offers to ask again only after the member declined screen sharing', async () => {
    const user = userEvent.setup();
    const askAgain = vi.fn();
    const { rerender } = render(
      <DeviceLimits capabilities={gnomeFirstLogin} onAskAgain={askAgain} />,
    );
    expect(screen.queryByRole('button', { name: 'Ask me again' })).not.toBeInTheDocument();

    const declined: CapabilitiesDto = {
      features: [
        { feature: 'screenshots', status: 'denied', reasons: ['consent_declined'] },
        { feature: 'apps', status: 'denied', reasons: ['gnome_extension_disabled'] },
      ],
    };
    rerender(<DeviceLimits capabilities={declined} onAskAgain={askAgain} />);
    // One button, under screenshots: never for the extension the member turned off.
    const button = screen.getByRole('button', { name: 'Ask me again' });
    expect(button.closest('li')).toHaveTextContent('Screenshots Off');
    await user.click(button);
    expect(askAgain).toHaveBeenCalledTimes(1);
    expect(await accessibilityViolations()).toEqual([]);
  });

  it('has no detectable accessibility violations', async () => {
    const { container } = render(
      <DeviceLimits capabilities={gnomeFirstLogin} onAskAgain={vi.fn()} />,
    );

    expect(await accessibilityViolations(container)).toEqual([]);
  });
});
