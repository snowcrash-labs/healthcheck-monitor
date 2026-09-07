import { Show } from "solid-js";
import { useDashboard } from "./context";
import { Notice } from "./components";
import { Facts, Timestamp } from "./diagnostics";

export default function MonitorStatus() {
  const dashboard = useDashboard();
  if (!dashboard) return <Notice error>Dashboard state is unavailable.</Notice>;
  return <><div class="page-heading"><div><h1>Monitor status</h1><p>Collection and storage health; resource problems are under Problems.</p></div></div>
    <Show when={dashboard.overview()}>{(view) => <>
      <section class="panel"><div class="panel-heading"><h2>Collection</h2><span>{view().running ? "Running" : "Stopped"}</span></div>
        <Facts values={[{ label: "Configuration revision", value: view().configuration_revision }, { label: "Publication", value: view().persistence_fault ? "Failing" : "Available" }, { label: "View generation", value: String(view().generation) }]} />
        <p class="panel-note">Last collector heartbeat: <Timestamp at={view().heartbeat_at} now={dashboard.now()} /></p>
      </section>
      <section class="panel"><div class="panel-heading"><h2>History storage</h2><span>{view().history.available ? "Available" : "Unavailable"}</span></div>
        <Facts values={[{ label: "Queued batches", value: String(view().history.queued_batches) }, { label: "Recorded gaps", value: String(view().history.gaps) }, { label: "Dropped events / runs", value: `${view().history.dropped_events} / ${view().history.dropped_runs}` }]} />
        <p class="panel-note">Last persisted: <Timestamp at={view().history.last_persisted_at} now={dashboard.now()} /></p>
      </section>
    </>}</Show>
  </>;
}
