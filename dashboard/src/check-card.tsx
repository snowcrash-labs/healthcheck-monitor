import { For, Show } from "solid-js";
import type { CheckView } from "./schema";
import { checkCatalog, checkPath, checkState, checkStateText, coverageText, interval } from "./check-catalog";
import { age, rule, utc } from "./format";
export function CheckCard(props: { check: CheckView; now: number }) {
  const state = () => checkState(props.check, props.now);
  return <a class="check-card linked-card" href={checkPath(props.check.target, props.check.check)}>
    <div><strong>{checkCatalog[props.check.check].title}</strong><span class={`check-result ${state() === "complete" ? "text-healthy" : "text-warning"}`}>{checkStateText[state()]}</span></div>
    <p>{props.check.target} · Every {interval(props.check.interval_seconds)}</p>
    <p>{checkCatalog[props.check.check].description}</p>
    <Show when={props.check.required_failures > 0}><p class="text-warning">{props.check.required_failures} required operation{props.check.required_failures === 1 ? "" : "s"} missing complete evidence.</p></Show>
    <Show when={props.check.optional_gaps > 0}><p>{props.check.optional_gaps} optional coverage gap{props.check.optional_gaps === 1 ? "" : "s"}.</p></Show>
    <ul><For each={props.check.failures}>{(failure) => <li><span>{rule(failure.operation)}: {coverageText[failure.coverage].title}</span><span class="requirement">{failure.required ? "Required for coverage" : "Optional"}</span></li>}</For></ul>
    <small title={props.check.finished_at ? utc(props.check.finished_at) : undefined}>{age(props.check.finished_at, props.now)}</small><span class="quiet-link">View all operations →</span>
  </a>;
}
