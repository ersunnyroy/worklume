import { Link } from '@tanstack/react-router';
import type { ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { useRuntimeConfig } from '../../config/runtime-config-context';
import { EnvironmentBadge } from './EnvironmentBadge';
import { MobileNav } from './MobileNav';
import { NavLinks } from './NavLinks';

export function AppShell({ children }: { readonly children: ReactNode }) {
  const { t } = useTranslation();
  const { environment } = useRuntimeConfig();

  return (
    <div className="min-h-dvh">
      <a href="#main" className="skip-link">
        {t('shell.skipToContent')}
      </a>
      <header className="flex h-14 items-center gap-3 border-b border-line bg-surface px-4">
        <MobileNav />
        <Link to="/" className="text-lg font-semibold">
          {t('app.name')}
        </Link>
        <EnvironmentBadge environment={environment} />
      </header>
      <div className="flex">
        <nav
          aria-label={t('shell.primaryNavigation')}
          className="hidden min-h-[calc(100dvh-3.5rem)] w-60 shrink-0 border-r border-line bg-surface p-3 md:block"
        >
          <NavLinks />
        </nav>
        <main id="main" tabIndex={-1} className="min-w-0 flex-1 p-6 focus:outline-none">
          {children}
        </main>
      </div>
    </div>
  );
}
