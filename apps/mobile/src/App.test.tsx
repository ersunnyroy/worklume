import { render, screen } from '@testing-library/react-native';
import * as ReactNative from 'react-native';
import { App } from './App';

describe('App', () => {
  it('announces the product name as a header', async () => {
    await render(<App />);

    expect(screen.getByRole('header', { name: 'Worklume' })).toBeOnTheScreen();
    expect(screen.getByText(/arrive in upcoming releases/)).toBeOnTheScreen();
  });

  it.each(['light', 'dark'] as const)('follows the system colour scheme (%s)', async (scheme) => {
    jest.spyOn(ReactNative, 'useColorScheme').mockReturnValue(scheme);

    await render(<App />);

    const background = scheme === 'dark' ? '#161b21' : '#ffffff';
    expect(screen.getByTestId('app-screen')).toHaveStyle({
      backgroundColor: background,
    });
  });
});
