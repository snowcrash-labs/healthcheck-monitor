import { Show } from "solid-js";
import type { ParentProps } from "solid-js";
import type { Health } from "./schema";
import { healthText } from "./format";
export function Status(props: { health: Health }) { return <span class={`status status-${props.health}`}><span class="status-dot" />{healthText[props.health]}</span>; }
export function Empty(props: { title: string; detail?: string }) { return <div class="empty"><span class="empty-symbol" aria-hidden="true">○</span><h3>{props.title}</h3><Show when={props.detail}><p>{props.detail}</p></Show></div>; }
export function Notice(props: ParentProps<{ error?: boolean }>) { return <div role={props.error ? "alert" : "status"} class={`notice${props.error ? " notice-error" : ""}`}>{props.children}</div>; }
export function Pagination(props: { next: string | null; current: string; onChange: (cursor: string) => void; total?: number }) {
  return <div class="pagination"><span>{props.total === undefined ? "Recent history" : `${props.total.toLocaleString()} results`}</span><div><button class="button" disabled={!props.current} onClick={() => props.onChange("")}>First page</button><button class="button" disabled={!props.next} onClick={() => { if (props.next) props.onChange(props.next); }}>Next page <span aria-hidden="true">→</span></button></div></div>;
}
