import { createRoot } from 'react-dom/client';
import { App } from './app/App';
import { createQueryClient } from './app/query-client';
import { createAppRouter } from './app/router';
import { BootError } from './components/errors/BootError';
import { loadRuntimeConfig } from './config/runtime-config';
import { initI18n, negotiateLanguage } from './i18n';
import './styles/index.css';

async function boot(container: HTMLElement): Promise<void> {
  const root = createRoot(container);
  try {
    const config = await loadRuntimeConfig();
    await initI18n(negotiateLanguage(navigator.languages));
    const queryClient = createQueryClient();
    const router = createAppRouter(queryClient);
    root.render(<App config={config} queryClient={queryClient} router={router} />);
  } catch (error) {
    // Nothing else can report yet; error tracking arrives in E00-S05.
    console.error('Worklume failed to start', error);
    root.render(<BootError />);
  }
}

const container = document.getElementById('root');
if (container === null) {
  throw new Error('index.html is missing the #root element');
}
void boot(container);
