import { useParams } from "@solidjs/router";
import { Show } from "solid-js";
import { query, resourceId } from "./api";
import { ResourceInspector } from "./resource-inspector";
import { useDashboard } from "./context";
import { Empty } from "./components";

export default function DetailPage() {
  const params = useParams();
  const dashboard = useDashboard();
  const id = () => resourceId(params.id);
  return <><a class="back-link" href={query("/resources", { target: dashboard?.target() })}>← Resources</a><Show when={id()} fallback={<Empty title="Invalid resource link" />}>{(id) => <ResourceInspector id={id()} />}</Show></>;
}
