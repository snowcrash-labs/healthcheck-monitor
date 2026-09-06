import { For, Show } from "solid-js";
import type { Finding, ResourceContext } from "./schema";
import { age, rule, utc } from "./format";
import { checkCatalog, checkPath } from "./check-catalog";

export function Timestamp(props: { at: string | null; now: number }) {
  return <Show when={props.at} fallback={<span class="muted">Not recorded</span>}>{(at) => <time datetime={at()} title={utc(at())}>{age(at(), props.now)}<span class="cell-detail">{utc(at())}</span></time>}</Show>;
}
export function Facts(props: { values: { label: string; value: string }[] }) {
  return <dl class="facts-grid"><For each={props.values}>{(fact) => <div><dt>{fact.label}</dt><dd>{fact.value}</dd></div>}</For></dl>;
}
export function ConsoleLinks(props: { links: { label: string; url: string }[] }) {
  return <div class="console-links"><For each={props.links}>{(link) => <a class="button" href={link.url} target="_blank" rel="noopener noreferrer">{link.label} ↗</a>}</For></div>;
}
export function Location(props: { context: ResourceContext | null | undefined }) {
  const fields = () => {
    const c = props.context;
    if (!c) return [];
    return [
      ["Provider", c.provider.toUpperCase()], [c.provider === "gcp" ? "Project" : c.provider === "aws" ? "Account" : c.provider === "azure" ? "Subscription" : "Scope", c.scope],
      ["Service", rule(c.service)], ["Native resource ID", c.native_id], ["Region", c.region], ["Zone", c.zone],
      ["Cluster / context", c.cluster], ["Namespace", c.namespace], ["Resource name", c.name], ["UID", c.uid], ["Container", c.container], ["Reason", c.reason], ["Last exit code", c.exit_code === null ? null : String(c.exit_code)],
    ].flatMap(([label, value]) => label && value ? [{ label, value }] : []);
  };
  return <Show when={props.context} fallback={<p class="muted">Location metadata was not recorded for this observation.</p>}><Facts values={fields()} /></Show>;
}
const titles: Record<string, string> = {
  "container-crash-loop": "Container cannot stay running",
  "container-restarting": "Container restart count increased",
  "pod-not-ready": "Pod is not ready after its startup grace period",
  "replicas-not-ready": "Fewer replicas are ready than expected",
  "node-not-ready": "Node is not ready",
  "job-failed": "Job reached a terminal failure",
  "endpoint-unreachable": "Endpoint failed its DNS, TLS, or HTTP check",
  "endpoint-latency": "Endpoint latency exceeded its threshold",
  "runtime-failure-sample": "Runtime failures found in sampled logs",
  "release-revision-mismatch": "Running revision does not match the desired release",
  "deployment-blocked": "A failed build is blocking deployment",
  "build-failed": "Build failed",
  "metric-threshold": "Observed metric exceeded its configured threshold",
  "service-unavailable": "Managed service is unavailable",
  "backup-disabled": "Backups are disabled",
  "backup-failed": "Backup failed",
  "recovery-point-too-old": "Latest recovery point is older than allowed",
  "certificate-expiring": "Certificate is approaching expiry",
  "certificate-invalid": "Certificate is invalid or expired",
  "encryption-disabled": "Encryption is disabled",
  "dead-letter-backlog": "Failed deliveries are waiting in the dead-letter queue",
  "queue-age": "Queued work is older than allowed",
  "queued-work-crash-looping-consumer": "Work is queued while a consumer is crash looping",
  "queued-work-scaler-not-ready": "Work is queued while its scaler is not ready",
  "persistent-queued-work-consumer-starting": "Work remains queued while consumers are starting",
  "flow-stalled": "Incoming work is not progressing",
  "schedule-missed": "A scheduled execution was missed",
  "latest-schedule-not-successful": "Latest scheduled execution has not completed successfully",
  "slo-burn-rate": "Error budget is being consumed too quickly",
  "slo-compliance": "Service objective is below its target",
};
export function findingTitle(value: string): string { return titles[value] ?? rule(value); }
export function FindingDetail(props: { finding: Finding; now: number }) {
  return <article class="finding-detail"><div><span class={`severity severity-${props.finding.severity}`}>{rule(props.finding.severity)}</span><h3>{findingTitle(props.finding.rule)}</h3></div>
    <p>Expected state: {rule(props.finding.expected)} · {rule(props.finding.confidence)} evidence{props.finding.stale ? " · Stale" : ""}</p>
    <Show when={props.finding.diagnostic} fallback={<p class="muted">Triggering metadata was not recorded. Last observation: <Timestamp at={props.finding.observed_at} now={props.now} /></p>}>{(diagnostic) => <>
      <div class="detail-meta"><span>First detected <Timestamp at={diagnostic().first_detected_at} now={props.now} /></span><span>Latest confirmed failure <Timestamp at={diagnostic().last_detected_at} now={props.now} /></span></div>
      <Facts values={diagnostic().facts} /><Location context={diagnostic().context} /><ConsoleLinks links={diagnostic().links} />
    </>}</Show>
    <Show when={props.finding.check}>{(check) => <a class="quiet-link" href={checkPath(props.finding.target, check())}>{checkCatalog[check()].title} →</a>}</Show>
    <details><summary>Evidence sources</summary><ul><For each={props.finding.evidence}>{(source) => <li>{source}</li>}</For></ul></details>
  </article>;
}
