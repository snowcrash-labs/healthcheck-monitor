import { createContext, createEffect, createSignal, onSettled, useContext } from "solid-js";
import type { Accessor, ParentProps } from "solid-js";
import { useLocation, useNavigate, useSearchParams } from "@solidjs/router";
import { get, query } from "./api";
import { overview as overviewSchema } from "./schema";
import type { Overview } from "./schema";

interface Dashboard {
  overview: Accessor<Overview | undefined>;
  targets: Accessor<Overview["targets"]>;
  error: Accessor<string>;
  connected: Accessor<boolean>;
  paused: Accessor<boolean>;
  authorized: Accessor<boolean>;
  togglePause: () => void;
  now: Accessor<number>;
  refreshId: Accessor<number>;
  costRefreshId: Accessor<number>;
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
  const [targets, setTargets] = createSignal<Overview["targets"]>([]);
  const [error, setError] = createSignal("");
  const [connected, setConnected] = createSignal(false);
  const [paused, setPaused] = createSignal(false);
  const [authorized, setAuthorized] = createSignal(true);
  const [now, setNow] = createSignal(Date.now());
  const [refreshId, setRefreshId] = createSignal(0);
  const [costRefreshId, setCostRefreshId] = createSignal(0);
  const target = () => routeTarget() ?? (typeof params.target === "string" ? params.target : "");
  const refresh = () => { if (!paused() && authorized()) setRefreshId((value) => value + 1); };
  createEffect(() => [target(), refreshId(), paused()] as const, ([target, , paused]) => {
    if (paused) return;
    const controller = new AbortController();
    void get(query("/api/v1/overview", { target }), overviewSchema, controller.signal).then((result) => {
      if (controller.signal.aborted) return;
      if (result.ok) { setOverview(result.value); setTargets(result.value.targets); setError(""); }
      else if (!result.cancelled) { if (result.unauthorized) setOverview(undefined); setError(result.message); }
    });
    return () => controller.abort();
  });
  onSettled(() => {
    let lastMessage = Date.now();
    let lastGeneration = "";
    let pending: ReturnType<typeof setTimeout> | undefined;
    let source: EventSource | undefined;
    const revoke = () => { setAuthorized(false); setOverview(undefined); setTargets([]); disconnect(); };
    const disconnect = () => { source?.close(); source = undefined; setConnected(false); };
    const connect = () => {
      disconnect();
      if (!navigator.onLine || !authorized()) return;
      source = new EventSource("/api/v1/events");
      source.onopen = () => { lastMessage = Date.now(); setConnected(true); };
      source.onerror = () => setConnected(false);
      source.addEventListener("revision", (event: MessageEvent<string>) => {
        lastMessage = Date.now(); setConnected(true);
        try {
          const value: unknown = JSON.parse(event.data);
          if (typeof value !== "object" || value === null || !("generation" in value) || typeof value.generation !== "number") return;
          const revision = `${"epoch" in value && typeof value.epoch === "string" ? value.epoch : ""}:${value.generation}`;
          if (revision !== lastGeneration) {
            lastGeneration = revision;
            if (pending === undefined) pending = setTimeout(() => { pending = undefined; refresh(); }, 1000);
          }
        } catch { setConnected(false); }
      });
      refresh();
    };
    connect();
    window.addEventListener("offline", disconnect);
    window.addEventListener("online", connect);
    window.addEventListener("monitor:unauthorized", revoke);
    const clock = setInterval(() => { if (!paused()) setNow(Date.now()); if (Date.now() - lastMessage > 15_000) setConnected(false); }, 1000);
    const poll = setInterval(() => { if (!connected()) refresh(); }, 15_000);
    const costs = setInterval(() => { if (!paused() && authorized()) setCostRefreshId((value) => value + 1); }, 60_000);
    return () => { disconnect(); window.removeEventListener("offline", disconnect); window.removeEventListener("online", connect); window.removeEventListener("monitor:unauthorized", revoke); clearInterval(clock); clearInterval(poll); clearInterval(costs); if (pending !== undefined) clearTimeout(pending); };
  });
  const value: Dashboard = { overview, targets, error, connected, paused, authorized, togglePause: () => { setPaused((value) => !value); if (!paused()) { setNow(Date.now()); setCostRefreshId((value) => value + 1); refresh(); } }, now, refreshId, costRefreshId, target, setTarget: (target) => {
    setPaused(false);
    setOverview(undefined);
    if (routeTarget()) { void navigate(target ? `/targets/${encodeURIComponent(target)}?target=${encodeURIComponent(target)}` : "/"); }
    else setParams({ target: target || undefined, cursor: undefined, selected: undefined, tab: undefined, day: undefined, contributor: undefined, resource: undefined });
  }, refresh };
  return <Context value={value}>{props.children}</Context>;
}
export function useDashboard(): Dashboard | undefined { return useContext(Context); }
