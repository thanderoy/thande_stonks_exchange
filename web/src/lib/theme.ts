/**
 * Theme choice: follow the system, or force light or dark. The choice is
 * stored in localStorage and applied as `data-theme` on <html>; CSS does the
 * rest. `public/theme-init.js` applies it before first paint.
 */
export type ThemeChoice = "system" | "light" | "dark";

export const THEME_KEY = "tse-theme";

const ORDER: ThemeChoice[] = ["system", "light", "dark"];

export function nextTheme(current: ThemeChoice): ThemeChoice {
  return ORDER[(ORDER.indexOf(current) + 1) % ORDER.length];
}

export function themeLabel(choice: ThemeChoice): string {
  return choice === "system" ? "System" : choice === "light" ? "Light" : "Dark";
}

export function readTheme(): ThemeChoice {
  try {
    const v = localStorage.getItem(THEME_KEY);
    if (v === "light" || v === "dark") return v;
  } catch {
    // Storage unavailable: fall through to system.
  }
  return "system";
}

export function applyTheme(choice: ThemeChoice): void {
  const root = document.documentElement;
  if (choice === "system") root.removeAttribute("data-theme");
  else root.setAttribute("data-theme", choice);
  try {
    if (choice === "system") localStorage.removeItem(THEME_KEY);
    else localStorage.setItem(THEME_KEY, choice);
  } catch {
    // Storage unavailable: the choice lasts until reload.
  }
}
