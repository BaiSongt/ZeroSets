export type Theme = "dark" | "light";

const THEME_STORAGE_KEY = "zeroset.theme";
const THEME_EVENT = "zeroset:themechange";

export function storedTheme(): Theme {
  try {
    const saved = localStorage.getItem(THEME_STORAGE_KEY);
    if (saved === "light" || saved === "dark") return saved;
  } catch {
    // localStorage can be unavailable in privacy-restricted contexts.
  }
  return "dark";
}

export function currentTheme(): Theme {
  return document.documentElement.dataset.theme === "light" ? "light" : "dark";
}

export function applyTheme(theme: Theme, persist = true): Theme {
  document.documentElement.dataset.theme = theme;
  document.documentElement.style.colorScheme = theme;
  if (persist) {
    try {
      localStorage.setItem(THEME_STORAGE_KEY, theme);
    } catch {
      // Persisting the preference is best-effort.
    }
  }
  window.dispatchEvent(new CustomEvent<Theme>(THEME_EVENT, { detail: theme }));
  return theme;
}

export function initTheme(): Theme {
  return applyTheme(storedTheme(), false);
}

export function toggleTheme(): Theme {
  return applyTheme(currentTheme() === "dark" ? "light" : "dark");
}

export function onThemeChange(listener: (theme: Theme) => void): () => void {
  const handler = (event: Event): void => {
    listener((event as CustomEvent<Theme>).detail);
  };
  window.addEventListener(THEME_EVENT, handler);
  return () => window.removeEventListener(THEME_EVENT, handler);
}
