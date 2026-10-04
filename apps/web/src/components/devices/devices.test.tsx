import { render, screen, within } from '@testing-library/react';
import i18next from 'i18next';
import { describe, expect, it } from 'vitest';
import agentFixture from '../../../../../packages/api-contract/test/fixtures/desktop-capabilities.json' with { type: 'json' };
import { accessibilityViolations } from '../../test/axe';
import { isReduced, limitedFeatures, type DesktopCapabilities } from './capabilities';
import { DeviceCapabilityBadge } from './DeviceCapabilityBadge';
import { DeviceCapabilityDetails } from './DeviceCapabilityDetails';

/**
 * Reports as the desktop agent encodes them, with every status and reason code it knows:
 * written by apps/desktop/sync/tests/capabilities_fixture.rs and checked against the contract.
 */
const fixture = agentFixture as {
  statuses: string[];
  reasons: string[];
  reports: { name: string; body: DesktopCapabilities }[];
};

function report(name: string): DesktopCapabilities {
  const found = fixture.reports.find((sample) => sample.name === name);
  if (!found) {
    throw new Error(`no ${name} report in the fixture`);
  }
  return found.body;
}

const allGranted: DesktopCapabilities = {
  platform: 'linux_wayland',
  desktop: 'gnome',
  features: {
    screenshots: { status: 'granted', reasons: [] },
    activity: { status: 'granted', reasons: [] },
    idle: { status: 'granted', reasons: [] },
    apps: { status: 'granted', reasons: [] },
    urls: { status: 'granted', reasons: [] },
    session_lock: { status: 'granted', reasons: [] },
  },
};

describe('device capabilities', () => {
  it('marks a GNOME Wayland device as reduced, and nothing before a report or when all is measured', () => {
    const { rerender, container } = render(
      <DeviceCapabilityBadge capabilities={report('gnome_wayland_first_login')} />,
    );
    expect(screen.getByText('Wayland: reduced')).toBeVisible();

    rerender(<DeviceCapabilityBadge capabilities={report('ubuntu_x11')} />);
    expect(screen.getByText('Linux X11: reduced')).toBeVisible();

    rerender(<DeviceCapabilityBadge capabilities={allGranted} />);
    expect(container).toBeEmptyDOMElement();
    rerender(<DeviceCapabilityBadge capabilities={null} />);
    expect(container).toBeEmptyDOMElement();
  });

  it('explains each feature the device does not fully measure, in order, and only those', () => {
    render(<DeviceCapabilityDetails capabilities={report('gnome_wayland_first_login')} />);

    const section = screen.getByRole('region', { name: "What this device can't fully measure" });
    const items = within(section).getAllByRole('listitem');
    expect(items.map((item) => item.querySelector('span')?.textContent)).toEqual([
      'Screenshots',
      'Activity',
      'Apps',
      'Websites',
    ]);
    expect(items[0]).toHaveTextContent('Waiting for the member');
    expect(items[0]).toHaveTextContent(
      "The member's desktop asks them to share their screen at the first screenshot.",
    );
    expect(items[1]).toHaveTextContent("Keyboard and mouse aren't counted separately.");
    expect(items[2]).toHaveTextContent('Starts after the member next logs in to their desktop.');
  });

  it('says why a member declined or turned something off', () => {
    render(<DeviceCapabilityDetails capabilities={report('gnome_wayland_declined')} />);

    expect(screen.getByText(/The member declined screen sharing/)).toBeVisible();
    expect(screen.getByText('The member turned off the Worklume GNOME extension.')).toBeVisible();
  });

  it('has words for every status and reason the agent sends', () => {
    for (const status of fixture.statuses) {
      expect(i18next.exists(`devices.capabilities.status.${status}`), status).toBe(true);
    }
    for (const reason of fixture.reasons) {
      expect(i18next.exists(`devices.capabilities.reason.${reason}`), reason).toBe(true);
    }
    for (const { name, body } of fixture.reports) {
      const { container, unmount } = render(<DeviceCapabilityDetails capabilities={body} />);
      expect(container.textContent, name).not.toContain('devices.capabilities');
      expect(limitedFeatures(body).length > 0, name).toBe(isReduced(body));
      unmount();
    }
  });

  it('shows nothing when everything is measured', () => {
    const { container } = render(<DeviceCapabilityDetails capabilities={allGranted} />);
    expect(container).toBeEmptyDOMElement();
    expect(isReduced(allGranted)).toBe(false);
  });

  it('has no detectable accessibility violations', async () => {
    const { container } = render(
      <>
        <DeviceCapabilityBadge capabilities={report('kde_wayland')} />
        <DeviceCapabilityDetails capabilities={report('kde_wayland')} />
      </>,
    );

    expect(await accessibilityViolations(container)).toEqual([]);
  });
});
