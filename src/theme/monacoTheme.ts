export type MonacoThemeName = 'vs' | 'vs-dark';

export interface ThemeAttributeRoot {
  getAttribute(name: string): string | null;
}

export interface ThemeObserver {
  observe(root: ThemeAttributeRoot): void;
  disconnect(): void;
}

export type ThemeObserverFactory = (onChange: () => void) => ThemeObserver;

const defaultObserverFactory: ThemeObserverFactory = (onChange) => {
  const observer = new MutationObserver(onChange);
  return {
    observe(root) {
      observer.observe(root as unknown as Node, { attributes: true, attributeFilter: ['data-theme'] });
    },
    disconnect() {
      observer.disconnect();
    }
  };
};

export function resolveMonacoTheme(root: ThemeAttributeRoot): MonacoThemeName {
  return root.getAttribute('data-theme') === 'light' ? 'vs' : 'vs-dark';
}

export function watchMonacoTheme(
  apply: (theme: MonacoThemeName) => void,
  root: ThemeAttributeRoot = document.documentElement,
  observerFactory: ThemeObserverFactory = defaultObserverFactory
): () => void {
  const sync = () => apply(resolveMonacoTheme(root));
  sync();
  const observer = observerFactory(sync);
  observer.observe(root);
  return () => observer.disconnect();
}
