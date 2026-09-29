import { Dialog } from 'radix-ui';
import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { NavLinks } from './NavLinks';

/** Below the `md` breakpoint the primary navigation lives in a modal drawer. */
export function MobileNav() {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);

  return (
    <Dialog.Root open={open} onOpenChange={setOpen}>
      <Dialog.Trigger className="icon-button md:hidden" aria-label={t('shell.openNavigation')}>
        <svg aria-hidden="true" viewBox="0 0 24 24" className="size-5">
          <path
            d="M4 7h16M4 12h16M4 17h16"
            stroke="currentColor"
            strokeWidth="2"
            strokeLinecap="round"
          />
        </svg>
      </Dialog.Trigger>
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 bg-black/40" />
        <Dialog.Content
          aria-describedby={undefined}
          className="fixed inset-y-0 left-0 w-72 max-w-[85vw] bg-surface p-4 shadow-xl focus:outline-none"
        >
          <div className="mb-4 flex items-center justify-between">
            <Dialog.Title className="text-base font-semibold">
              {t('shell.navigationTitle')}
            </Dialog.Title>
            <Dialog.Close className="icon-button" aria-label={t('shell.closeNavigation')}>
              <svg aria-hidden="true" viewBox="0 0 24 24" className="size-5">
                <path
                  d="M6 6l12 12M18 6L6 18"
                  stroke="currentColor"
                  strokeWidth="2"
                  strokeLinecap="round"
                />
              </svg>
            </Dialog.Close>
          </div>
          <nav aria-label={t('shell.primaryNavigation')}>
            <NavLinks
              onNavigate={() => {
                setOpen(false);
              }}
            />
          </nav>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
