import { screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it } from 'vitest';
import { accessibilityViolations } from '../../test/axe';
import { renderApp } from '../../test/render-app';

describe('mobile navigation', () => {
  it('opens as a labelled dialog and closes with Escape, returning focus', async () => {
    const user = userEvent.setup();
    await renderApp();
    const trigger = screen.getByRole('button', { name: 'Open navigation' });

    await user.click(trigger);
    const dialog = await screen.findByRole('dialog', { name: 'Navigation' });
    expect(within(dialog).getByRole('link', { name: 'Home' })).toBeInTheDocument();
    expect(await accessibilityViolations()).toEqual([]);

    await user.keyboard('{Escape}');
    await waitFor(() => {
      expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
    });
    expect(trigger).toHaveFocus();
  });

  it('closes after choosing a destination', async () => {
    const user = userEvent.setup();
    await renderApp();

    await user.click(screen.getByRole('button', { name: 'Open navigation' }));
    const dialog = await screen.findByRole('dialog', { name: 'Navigation' });
    await user.click(within(dialog).getByRole('link', { name: 'Home' }));

    await waitFor(() => {
      expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
    });
  });
});
