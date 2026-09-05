import { createEffect, createSignal, onSettled } from "solid-js";
export type Theme = "system" | "light" | "dark";
export function readTheme(): Theme {
  try { const theme = localStorage.getItem("health-dashboard-theme"); return theme === "light" || theme === "dark" ? theme : "system"; }
  catch { return "system"; }
}
export function applyTheme(theme: Theme): void {
  if (typeof document === "undefined") return;
  const dark = theme === "dark" || (theme === "system" && typeof matchMedia === "function" && matchMedia("(prefers-color-scheme: dark)").matches);
  document.documentElement.dataset.theme = dark ? "dark" : "light";
}
applyTheme(readTheme());
export function ThemePicker() {
  const [theme, setTheme] = createSignal<Theme>(readTheme());
  createEffect(theme, applyTheme);
  onSettled(() => {
    const media = matchMedia("(prefers-color-scheme: dark)");
    const update = () => applyTheme(theme());
    media.addEventListener("change", update);
    return () => media.removeEventListener("change", update);
  });
  return <label class="theme-picker">Appearance<select aria-label="Appearance" value={theme()} onChange={(event) => {
    const value = event.currentTarget.value;
    if (value !== "light" && value !== "dark" && value !== "system") return;
    setTheme(value);
    try { localStorage.setItem("health-dashboard-theme", value); } catch { /* The selected mode still applies when storage is unavailable. */ }
  }}><option value="system">System</option><option value="light">Light</option><option value="dark">Dark</option></select></label>;
}
