import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { App } from './App';
import { initI18n, negotiateLanguage } from './i18n';
import './styles.css';

const container = document.getElementById('root');
if (container === null) {
  throw new Error('index.html is missing the #root element');
}
const root = createRoot(container);
void initI18n(negotiateLanguage(navigator.languages)).then(() => {
  root.render(
    <StrictMode>
      <App />
    </StrictMode>,
  );
});
