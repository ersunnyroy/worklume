import type { LinkProps } from '@tanstack/react-router';

export interface NavItem {
  readonly to: NonNullable<LinkProps['to']>;
  readonly labelKey: 'nav.home';
}

/** Primary navigation. Each domain's screens add their entry here as they ship. */
export const primaryNavigation: readonly NavItem[] = [{ to: '/', labelKey: 'nav.home' }];
