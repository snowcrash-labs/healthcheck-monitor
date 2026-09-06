import { createEffect } from "solid-js";

/** Keyboard-accessible paging also loads automatically near either edge of the window. */
export function ScrollBoundary(props: { previous?: boolean; enabled: boolean; loading: boolean; total?: number; load: () => void }) {
  let element: HTMLDivElement | undefined;
  createEffect(() => [props.enabled, props.loading] as const, ([enabled, loading]) => {
    if (!element || !enabled || loading) return;
    const observer = new IntersectionObserver((entries) => {
      if (entries.some((entry) => entry.isIntersecting)) props.load();
    }, { rootMargin: "160px" });
    observer.observe(element);
    return () => observer.disconnect();
  });
  return <div ref={element} class="scroll-boundary" aria-live="polite">
    <span>{props.total === undefined ? "" : `${props.total.toLocaleString()} results`}</span>
    <button class="button" disabled={!props.enabled || props.loading} onClick={props.load}>{props.loading ? "Loading…" : props.previous ? "Load previous results" : props.enabled ? "Load more results" : "All results reached"}</button>
  </div>;
}
