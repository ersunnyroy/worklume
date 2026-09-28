import { render, screen } from '@testing-library/react';
import axe from 'axe-core';
import { describe, expect, it } from 'vitest';
import { StatusScreen } from './StatusScreen';

describe('StatusScreen', () => {
  it('explains that the agent keeps running from the tray', () => {
    render(<StatusScreen />);

    expect(screen.getByRole('heading', { level: 1, name: 'Worklume' })).toBeVisible();
    expect(screen.getByText(/Closing this window keeps it running/)).toBeInTheDocument();
  });

  it('has no detectable accessibility violations', async () => {
    const { container } = render(<StatusScreen />);

    // Colour contrast needs real layout; jsdom cannot compute it.
    const results = await axe.run(container, { rules: { 'color-contrast': { enabled: false } } });
    expect(results.violations.map((violation) => violation.id)).toEqual([]);
  });
});
