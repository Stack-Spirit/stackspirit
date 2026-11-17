import { useEffect, useState } from "react";

/** "system" follows prefers-color-scheme, like DeepSeek's desktop. */
export type ThemePreference = "system" | "light" | "dark";

const KEY = "stackspirit.theme";

function read(): ThemePreference {
  try {
    const value = window.localStorage.getItem(KEY);
    return value === "light" || value === "dark" ? value : "system";
  } catch {
    return "system";
  }
}

/**
 * Marks <html> as the next shell (so its tokens apply, including to dialogs that
 * portal to <body>) and applies the saved theme. `?theme=dark|light` overrides it for
 * previews. Both attributes are removed on unmount so the classic shell is unaffected.
 */
export function useNextTheme(): [ThemePreference, (theme: ThemePreference) => void] {
  const [theme, setTheme] = useState<ThemePreference>(() => {
    const forced = new URLSearchParams(window.location.search).get("theme");
    return forced === "light" || forced === "dark" ? forced : read();
  });

  useEffect(() => {
    const root = document.documentElement;
    root.dataset.ui = "next";
    if (theme === "system") delete root.dataset.theme;
    else root.dataset.theme = theme;
    return () => {
      delete root.dataset.ui;
      delete root.dataset.theme;
    };
  }, [theme]);

  const update = (next: ThemePreference) => {
    setTheme(next);
    try {
      if (next === "system") window.localStorage.removeItem(KEY);
      else window.localStorage.setItem(KEY, next);
    } catch {
      // Storage can be unavailable; the choice still applies for this session.
    }
  };

  return [theme, update];
}
