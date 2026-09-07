import { For } from "solid-js";
import type { CheckView } from "./schema";
import { age } from "./format";
import { checkCatalog, checkPath, checkState, coverageText, interval } from "./check-catalog";

const states = { complete: "Collected", incomplete: "Partial", awaiting: "Waiting", stale: "Stale" };
export function checkIssue(check: CheckView, now: number): string {
  const state = checkState(check, now);
  if (state === "awaiting") return "Waiting for the first run";
  if (state === "stale") return "A fresh observation is overdue";
  const failure = check.failures.find((item) => item.required);
  if (failure) return coverageText[failure.coverage].title;
  return check.optional_gaps ? `${check.optional_gaps} optional coverage gaps` : "No collection failures";
}
export function CheckTable(props: { checks: CheckView[]; now: number }) {
  return <div class="table-scroll"><table class="checks-table"><thead><tr><th>Check</th><th>Target</th><th>Collection</th><th>Main issue</th><th>Last run</th><th>Schedule</th></tr></thead><tbody>
    <For each={props.checks} keyed={(check) => check.key}>{(check) => <tr data-row-key={check().key}>
      <td><a class="primary-link" href={checkPath(check().target, check().check)}>{checkCatalog[check().check].title}</a></td><td>{check().target}</td>
      <td><span class={`status ${checkState(check(), props.now) === "complete" ? "status-healthy" : "status-unknown"}`}>{states[checkState(check(), props.now)]}</span></td>
      <td>{checkIssue(check(), props.now)}</td><td><time datetime={check().finished_at ?? undefined}>{age(check().finished_at, props.now)}</time></td><td>Every {interval(check().interval_seconds)}</td>
    </tr>}</For>
  </tbody></table></div>;
}
