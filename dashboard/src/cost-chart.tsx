import { createMemo, createSignal, For, Show } from "solid-js";
import type { CostPoint, CostView } from "./cost-schema";
import { colorClass, contributorLabel, money } from "./cost-schema";

export function CostChart(props: { view: CostView; select: (day: string, key: string) => void; compare?: boolean; compact?: boolean }) {
  const [focused, setFocused] = createSignal<CostPoint>();
  const points = () => props.view.series;
  const keys = createMemo(() => [...new Set(points().flatMap((point) => point.contributors.map((c) => c.key)))]);
  const extremes = createMemo(() => {
    let max = 0; let min = 0;
    for (const point of points()) {
      let positive = 0; let negative = 0;
      for (const row of point.contributors) { const value = Number(row.amount); if (value >= 0) positive += value; else negative += value; }
      max = Math.max(max, positive, props.compare && point.previous ? Number(point.previous) : 0);
      min = Math.min(min, negative, props.compare && point.previous ? Number(point.previous) : 0);
    }
    return { max: max || 1, min, span: (max || 1) - min };
  });
  const y = (amount: number) => 22 + (extremes().max - amount) / extremes().span * 184;
  const step = () => 704 / Math.max(1, points().length);
  const x = (index: number) => 76 + index * step();
  const bars = (point: CostPoint) => {
    let positive = 0; let negative = 0;
    return point.contributors.map((item) => {
      const value = Number(item.amount); const base = value >= 0 ? positive : negative;
      if (value >= 0) positive += value; else negative += value;
      return { ...item, top: y(Math.max(base, base + value)), height: Math.abs(y(base) - y(base + value)) };
    });
  };
  const previous = () => points().map((p, i) => p.previous === null ? "" : `${i === 0 || points()[i - 1]?.previous === null ? "M" : "L"} ${x(i) + step() / 2} ${y(Number(p.previous))}`).join(" ");
  return <div class="cost-chart">
    <svg viewBox="0 0 800 252" role="img" aria-label={`Billed cost by ${props.view.group}, ${props.view.currency}. Select a bar to filter spending. Values are available in the table below.`}>
      <For each={[0, 1, 2, 3, 4]}>{(tick) => <g><line class="chart-grid" x1="76" x2="786" y1={22 + tick * 46} y2={22 + tick * 46} /><text class="chart-axis" x="65" y={26 + tick * 46} text-anchor="end">{Math.round(extremes().max - tick / 4 * extremes().span).toLocaleString()}</text></g>}</For>
      <line class="chart-zero" x1="76" x2="786" y1={y(0)} y2={y(0)} />
      <For each={points()} keyed={(p) => p.date}>{(point, index) => <g>
        <For each={bars(point())} keyed={(bar) => bar.key}>{(bar) => <rect class={`chart-bar ${colorClass(bar().key)}`} x={x(index()) + 2} y={bar().top} width={Math.max(1, step() - 4)} height={Math.max(0.5, bar().height)} tabindex="0" role="button" aria-label={`${point().date}, ${contributorLabel(bar().key)}, ${money(bar().amount, props.view.currency)}`}
          onMouseEnter={() => setFocused(point())} onFocus={() => setFocused(point())}
          onClick={() => props.select(point().date, bar().key)}
          onKeyDown={(event) => { if (event.key === "Enter" || event.key === " ") { event.preventDefault(); props.select(point().date, bar().key); } }}><title>{point().date} · {contributorLabel(bar().key)} · {money(bar().amount, props.view.currency)}</title></rect>}</For>
        <Show when={index() % Math.max(1, Math.ceil(points().length / 6)) === 0}><text class="chart-axis" x={x(index()) + step() / 2} y="234" text-anchor="middle">{point().date.slice(5)}</text></Show>
      </g>}</For>
      <Show when={props.compare}><path class="chart-previous" d={previous()} /></Show>
    </svg>
    <div class="chart-legend"><For each={keys()}>{(key) => <button class="legend-button" onClick={() => props.select("", key)}><span class={`legend-swatch ${colorClass(key)}`} />{contributorLabel(key)}</button>}</For><Show when={props.compare}><span class="previous-legend">Previous total</span></Show></div>
    <div class="chart-readout" aria-live="polite"><Show when={focused()} fallback={<span>{props.view.complete ? "Imported costs" : "Provisional export; late charges and adjustments may change totals."}</span>}>{(point) => <><strong>{point().date} · {money(point().total, props.view.currency)}</strong><span>{point().contributors.map((c) => `${contributorLabel(c.key)} ${money(c.amount, props.view.currency)}`).join(" · ")}</span></>}</Show></div>
    <Show when={!props.compact}><details class="chart-data"><summary>View accessible daily cost table</summary><div class="table-scroll"><table><thead><tr><th>Date</th><th>Cost</th><th>Previous period</th></tr></thead><tbody><For each={points()} keyed={(p) => p.date}>{(p) => <tr><td><button class="text-button" onClick={() => props.select(p().date, "")}>{p().date}</button></td><td class="numeric">{money(p().total, props.view.currency)}</td><td class="numeric">{money(p().previous, props.view.currency)}</td></tr>}</For></tbody></table></div></details></Show>
  </div>;
}
