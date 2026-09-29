import '@testing-library/jest-dom/vitest';
import { cleanup } from '@testing-library/react';
import { afterEach, beforeAll } from 'vitest';
import { initI18n } from '../i18n';

beforeAll(async () => {
  await initI18n('en');
});

afterEach(() => {
  cleanup();
});

// index.html gives the window its title; jsdom starts without one.
document.title = 'Worklume';

// jsdom has no modal dialogs. These open and close them as a browser does, including Escape
// firing a cancelable `cancel` event, but without the top layer or the inert background, which
// the webview provides.
if (typeof HTMLDialogElement.prototype.showModal !== 'function') {
  const escapeListeners = new WeakMap<HTMLDialogElement, (event: KeyboardEvent) => void>();
  HTMLDialogElement.prototype.showModal = function showModal(this: HTMLDialogElement) {
    this.setAttribute('open', '');
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape' && this.dispatchEvent(new Event('cancel', { cancelable: true }))) {
        this.close();
      }
    };
    escapeListeners.set(this, onKeyDown);
    this.addEventListener('keydown', onKeyDown);
  };
  HTMLDialogElement.prototype.close = function close(this: HTMLDialogElement) {
    const onKeyDown = escapeListeners.get(this);
    if (onKeyDown !== undefined) {
      this.removeEventListener('keydown', onKeyDown);
    }
    this.removeAttribute('open');
    this.dispatchEvent(new Event('close'));
  };
}
