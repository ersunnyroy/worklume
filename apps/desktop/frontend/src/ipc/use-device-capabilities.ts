import { useEffect, useState } from 'react';
import { commands, events, type CapabilitiesDto } from './bindings.gen';

/**
 * What the device can measure, as the agent last checked: read once, then pushed by the
 * `capabilities-changed` event. `null` until the agent's first check.
 */
export function useDeviceCapabilities(): CapabilitiesDto | null {
  const [capabilities, setCapabilities] = useState<CapabilitiesDto | null>(null);

  useEffect(() => {
    let active = true;
    const unlisten = events.capabilitiesChanged.listen((event) => {
      if (active) {
        setCapabilities(event.payload);
      }
    });
    commands.deviceCapabilities().then(
      (current) => {
        if (active) {
          // An event may already have delivered something newer.
          setCapabilities((previous) => previous ?? current);
        }
      },
      () => undefined,
    );
    return () => {
      active = false;
      void unlisten.then((stop) => {
        stop();
      });
    };
  }, []);

  return capabilities;
}
