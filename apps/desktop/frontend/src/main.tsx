import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { StatusScreen } from './StatusScreen';
import './styles.css';

const container = document.getElementById('root');
if (container === null) {
  throw new Error('index.html is missing the #root element');
}
createRoot(container).render(
  <StrictMode>
    <StatusScreen />
  </StrictMode>,
);
