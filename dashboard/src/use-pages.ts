import { createEffect, createMemo, createSignal, onSettled } from "solid-js";
import type { Accessor } from "solid-js";
import type { z } from "zod";
import { get } from "./api";

export interface Page<T> { generation: number; items: T[]; next_cursor: string | null; previous_cursor: string | null; total: number }
interface Position { cursor?: string; direction?: "next" | "previous" }
interface Loaded<T> { position: Position; page: Page<T> }
const windowPages = 3;

/** Evicted pages remain reachable through the server's previous/next cursors. */
export function usePages<T extends { id: string }>(path: Accessor<string>, schema: z.ZodType<Page<T>>, refresh: Accessor<number>) {
  const [pages, setPages] = createSignal<Loaded<T>[]>([]);
  const [loading, setLoading] = createSignal(true);
  const [error, setError] = createSignal("");
  let activePath = "";
  let controller: AbortController | undefined;
  let sequence = 0;
  let frame = 0;
  const data = createMemo(() => {
    const loaded = pages();
    const first = loaded[0]; const last = loaded.at(-1);
    if (!first || !last) return undefined;
    const seen = new Set<string>();
    return { ...last.page, previous_cursor: first.page.previous_cursor, items: loaded.flatMap(({ page }) => page.items.filter((row) => {
      if (seen.has(row.id)) return false;
      seen.add(row.id); return true;
    })) };
  });
  function address(position: Position): string {
    const url = new URL(activePath, window.location.origin);
    url.searchParams.set("limit", "50");
    if (position.cursor) url.searchParams.set("cursor", position.cursor);
    if (position.direction) url.searchParams.set("direction", position.direction);
    return url.pathname + url.search;
  }
  function publish(loaded: Loaded<T>[]) {
    const anchor = [...document.querySelectorAll<HTMLElement>("[data-row-key]")].find((row) => row.getBoundingClientRect().bottom > 160);
    const key = anchor?.dataset.rowKey; const top = anchor?.getBoundingClientRect().top;
    setPages(loaded);
    cancelAnimationFrame(frame);
    frame = requestAnimationFrame(() => {
      if (!key || top === undefined) return;
      const row = document.querySelector<HTMLElement>(`[data-row-key="${CSS.escape(key)}"]`);
      if (row) window.scrollBy({ top: row.getBoundingClientRect().top - top, behavior: "instant" });
    });
  }
  async function collect(direction?: "next" | "previous") {
    const current = pages();
    const cursor = direction === "next" ? current.at(-1)?.page.next_cursor : current[0]?.page.previous_cursor;
    if (direction && (!cursor || loading())) return;
    controller?.abort();
    const request = new AbortController(); controller = request;
    const version = ++sequence;
    setLoading(true); setError("");
    const loaded: Loaded<T>[] = [];
    let position: Position = direction && cursor ? { cursor, direction } : current[0]?.position ?? {};
    const count = direction ? 1 : Math.max(1, current.length);
    for (let index = 0; index < count; index++) {
      const result = await get(address(position), schema, request.signal);
      if (version !== sequence || request.signal.aborted) return;
      if (!result.ok) { setLoading(false); if (!result.cancelled) setError(result.message); return; }
      loaded.push({ position, page: result.value });
      if (!result.value.next_cursor) break;
      position = { cursor: result.value.next_cursor, direction: "next" };
    }
    if (direction === "next") publish([...current, ...loaded].slice(-windowPages));
    else if (direction === "previous") publish([...loaded, ...current].slice(0, windowPages));
    else publish(loaded);
    setLoading(false);
  }
  createEffect(path, (nextPath) => {
    controller?.abort(); sequence++; activePath = nextPath;
    setPages([]); setLoading(true); setError("");
    const timer = setTimeout(() => void collect(), 150);
    return () => clearTimeout(timer);
  });
  createEffect(refresh, () => { if (activePath && !loading()) void collect(); });
  onSettled(() => () => { controller?.abort(); sequence++; cancelAnimationFrame(frame); });
  return { data, loading, error, next: () => void collect("next"), previous: () => void collect("previous"), retry: () => void collect() };
}
