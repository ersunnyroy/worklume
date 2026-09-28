import { createRootRouteWithContext, Outlet } from '@tanstack/react-router';
import type { RouterContext } from '../app/router';
import { AppShell } from '../components/layout/AppShell';

export const Route = createRootRouteWithContext<RouterContext>()({
  component: RootLayout,
});

function RootLayout() {
  return (
    <AppShell>
      <Outlet />
    </AppShell>
  );
}
