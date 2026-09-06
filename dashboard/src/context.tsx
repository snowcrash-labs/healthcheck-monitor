import { createContext, createEffect, createSignal, onSettled, useContext } from "solid-js";
import type { Accessor, ParentProps } from "solid-js";
import { useLocation, useNavigate, useSearchParams } from "@solidjs/router";
import { get, query } from "./api";
import { overview as overviewSchema } from "./schema";
import type { Overview } from "./schema";

interface Dashboard {
  overview: Accessor<Overview | undefined>;
  error: Accessor<string>;
  connected: Accessor<boolean>;
  now: Accessor<number>;
  refreshId: Accessor<number>;
  target: Accessor<string>;
  setTarget: (target: string) => void;
  refresh: () => void;
}
const Context = createContext<Dashboard>();
export function DashboardProvider(props: ParentProps) {
  const [params, setParams] = useSearchParams();
  const location = useLocation();
  const navigate = useNavigate();
  const routeTarget = () => {
    const match = /^\/(?:targets|checks)\/([^/]+)/.exec(location.pathname);
    try { return match?.[1] ? decodeURIComponent(match[1]) : undefined; } catch { return undefined; }
  };
  const [overview, setOverview] = createSignal<Overview>();
  const [error, setError] = createSignal("");
  const [connected, setConnected] = createSignal(false);
  const [now, setNow] = createSignal(Date.now());
  const [refreshId, setRefreshId] = createSignal(0);
  const target = () => routeTarget() ?? (typeof params.target === "string" ? params.target : "");
  const refresh = () => setRefreshId((value) => value + 1);
  createEffect(() => [target(), refreshId()] as const, ([target]) => {
    const controller = new AbortController();
    void get(query("/api/v1/overview", { target }), overviewSchema, controller.signal).then((result) => {
      if (controller.signal.aborted) return;
      if (result.ok) { setOverview(result.value); setError(""); }
      else if (!result.cancelled) setError(result.message);
    });
    return () => controller.abort();
  });
  onSettled(() => {
    let lastMessage = Date.now();
    let lastGeneration = -1;
    let pending: ReturnType<typeof setTimeout> | undefined;
    let source: EventSource | undefined;
    const disconnect = () => { source?.close(); source = undefined; setConnected(false); };
    const connect = () => {
      disconnect();
      if (!navigator.onLine) return;
      source = new EventSource("/api/v1/events");
      source.onopen = () => { lastMessage = Date.now(); setConnected(true); };
      source.onerror = () => setConnected(false);
      source.addEventListener("revision", (event: MessageEvent<string>) => {
        lastMessage = Date.now(); setConnected(true);
        try {
          const value: unknown = JSON.parse(event.data);
          if (typeof value !== "object" || value === null || !("generation" in value) || typeof value.generation !== "number") return;
          if (value.generation !== lastGeneration) {
            lastGeneration = value.generation;
            if (pending === undefined) pending = setTimeout(() => { pending = undefined; refresh(); }, 250);
          }
        } catch { setConnected(false); }
      });
      refresh();
    };
    connect();
    window.addEventListener("offline", disconnect);
    window.addEventListener("online", connect);
    const clock = setInterval(() => { setNow(Date.now()); if (Date.now() - lastMessage > 15_000) setConnected(false); }, 1000);
    const poll = setInterval(refresh, 15_000);
    return () => { disconnect(); window.removeEventListener("offline", disconnect); window.removeEventListener("online", connect); clearInterval(clock); clearInterval(poll); if (pending !== undefined) clearTimeout(pending); };
  });
  const value: Dashboard = { overview, error, connected, now, refreshId, target, setTarget: (target) => {
    if (routeTarget()) { void navigate(target ? `/targets/${encodeURIComponent(target)}?target=${encodeURIComponent(target)}` : "/"); }
    else setParams({ target: target || undefined, cursor: undefined });
  }, refresh };
  return <Context value={value}>{props.children}</Context>;
}
export function useDashboard(): Dashboard | undefined { return useContext(Context); }
