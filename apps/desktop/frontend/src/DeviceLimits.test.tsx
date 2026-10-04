import { render, screen, within } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
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
    const { container, rerender } = render(<DeviceLimits capabilities={null} />);
    expect(container).toBeEmptyDOMElement();

    rerender(
      <DeviceLimits
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
    render(<DeviceLimits capabilities={gnomeFirstLogin} />);

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

  it('has no detectable accessibility violations', async () => {
    const { container } = render(<DeviceLimits capabilities={gnomeFirstLogin} />);

    expect(await accessibilityViolations(container)).toEqual([]);
  });
});
