import { createEffect, createSignal } from "solid-js";
import type { Accessor } from "solid-js";
import type { z } from "zod";
import { get } from "./api";
import { useDashboard } from "./context";

export function useQuery<T>(path: Accessor<string>, schema: z.ZodType<T>, refresh: Accessor<number>) {
  const dashboard = useDashboard();
  const [data, setData] = createSignal<T>();
  const [error, setError] = createSignal("");
  const [loading, setLoading] = createSignal(true);
  let previousPath = "";
  createEffect(() => [path(), refresh(), dashboard?.paused() ?? false] as const, ([path, , paused]) => {
    const changed = previousPath !== path;
    if (paused && !changed) { setLoading(false); return; }
    if (changed) setData(undefined);
    previousPath = path;
    setLoading(true); setError("");
    const controller = new AbortController();
    const timer = setTimeout(() => {
      void get(path, schema, controller.signal).then((result) => {
        if (controller.signal.aborted) return;
        setLoading(false);
        if (result.ok) setData(() => result.value);
        else if (!result.cancelled) { if (result.unauthorized) setData(undefined); setError(result.message); }
      });
    }, changed ? 120 : 0);
    return () => { clearTimeout(timer); controller.abort(); };
  });
  return { data, error, loading };
}
