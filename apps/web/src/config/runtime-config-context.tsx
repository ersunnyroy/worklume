import { createContext, use, type ReactNode } from 'react';
import type { RuntimeConfig } from './runtime-config';

const RuntimeConfigContext = createContext<RuntimeConfig | null>(null);

export function RuntimeConfigProvider({
  config,
  children,
}: {
  readonly config: RuntimeConfig;
  readonly children: ReactNode;
}) {
  return <RuntimeConfigContext value={config}>{children}</RuntimeConfigContext>;
}

export function useRuntimeConfig(): RuntimeConfig {
  const config = use(RuntimeConfigContext);
  if (config === null) {
    throw new Error('useRuntimeConfig must be used inside RuntimeConfigProvider');
  }
  return config;
}
