import { For } from "solid-js";
import type { z } from "zod";
import { operation } from "./schema";
import { coverageText } from "./check-catalog";
import { age, rule } from "./format";
export function OperationsTable(props: { operations: z.infer<typeof operation>[]; now: number }) {
  return <div class="table-scroll"><table><thead><tr><th>Operation</th><th>Collection</th><th>Requirement</th><th>Records</th><th>Last observed</th></tr></thead><tbody><For each={props.operations} keyed={(row) => row.id}>{(row) => <tr data-row-key={row().id}><td><details><summary>{rule(row().id)}</summary><p>{coverageText[row().coverage].detail}</p><span class="cell-detail">{row().pages} pages · {row().attempts} attempts</span></details></td><td>{coverageText[row().coverage].title}</td><td>{row().required ? "Required" : "Optional"}</td><td class="numeric">{row().records.toLocaleString()}</td><td><time datetime={row().observed_at}>{age(row().observed_at, props.now)}</time></td></tr>}</For></tbody></table></div>;
}
