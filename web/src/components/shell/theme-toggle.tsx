import { component$, useSignal, useVisibleTask$ } from "@builder.io/qwik";
import {
  applyTheme,
  nextTheme,
  readTheme,
  themeLabel,
  type ThemeChoice,
} from "~/lib/theme";

export const ThemeToggle = component$(() => {
  const theme = useSignal<ThemeChoice>("system");

  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(() => {
    theme.value = readTheme();
  });

  return (
    <button
      type="button"
      class="btn btn-small"
      aria-label={`Theme: ${themeLabel(theme.value)}. Switch to ${themeLabel(nextTheme(theme.value))}.`}
      onClick$={() => {
        const next = nextTheme(theme.value);
        theme.value = next;
        applyTheme(next);
      }}
    >
      <svg
        width="14"
        height="14"
        viewBox="0 0 16 16"
        aria-hidden="true"
        focusable="false"
      >
        <circle cx="8" cy="8" r="6.5" fill="none" stroke="currentColor" />
        <path d="M8 1.5a6.5 6.5 0 0 1 0 13z" fill="currentColor" />
      </svg>
      {themeLabel(theme.value)}
    </button>
  );
});
