import { Link } from '@tanstack/react-router';
import { useTranslation } from 'react-i18next';
import { primaryNavigation } from './navigation';

export function NavLinks({ onNavigate }: { readonly onNavigate?: () => void }) {
  const { t } = useTranslation();
  return (
    <ul className="flex flex-col gap-1">
      {primaryNavigation.map((item) => (
        <li key={item.to}>
          <Link
            to={item.to}
            onClick={onNavigate}
            activeOptions={{ exact: item.to === '/' }}
            className="nav-link"
          >
            {t(item.labelKey)}
          </Link>
        </li>
      ))}
    </ul>
  );
}
