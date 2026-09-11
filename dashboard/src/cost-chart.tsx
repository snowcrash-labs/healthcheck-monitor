import { createMemo, createSignal, createUniqueId, For, Show, onSettled } from "solid-js";
import type { CostPoint, CostView } from "./cost-schema";
import { contributorLabel, coverageNotice, hasCredits, money, net, units } from "./cost-schema";

export function CostChart(props: { view: CostView; select: (day: string, key: string) => void; compare?: boolean; compact?: boolean }) {
  const [focusedDate, setFocusedDate] = createSignal<string>();
  const [width, setWidth] = createSignal(800);
  // Pattern ids are document-global; two charts on one page must not share one.
  const dots = `credit-dots-${createUniqueId()}`;
  let host: HTMLDivElement | undefined;
  onSettled(() => {
    if (!host) return;
    let frame = 0; let measured = 0;
    const observer = new ResizeObserver(([entry]) => {
      if (!entry) return;
      const next = Math.max(160, Math.round(entry.contentRect.width));
      if (next === measured) return;
      measured = next;
      // Changing the SVG ratio changes its height; publish outside the resize delivery.
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => setWidth(next));
    });
    observer.observe(host);
    return () => { observer.disconnect(); cancelAnimationFrame(frame); };
  });
  const focused = () => points().find((point) => point.date === focusedDate());
  const points = () => props.view.series;
  const keys = createMemo(() => [...new Set(points().flatMap((point) => point.contributors.map((c) => c.key)))]);
  const credited = createMemo(() => points().some((point) => hasCredits(point.credits)));
  let previousColors = new Map<string, string>();
  const colors = createMemo(() => {
    const active = keys().filter((key) => key !== "__other__").slice(0, 7).sort();
    const next = new Map<string, string>(); const used = new Set<string>();
    for (const key of active) { const old = previousColors.get(key); if (old) { next.set(key, old); used.add(old); } }
    for (const key of active) if (!next.has(key)) {
      const chosen = Array.from({ length: 7 }, (_, index) => `chart-${index + 1}`).find((color) => !used.has(color));
      if (chosen) { next.set(key, chosen); used.add(chosen); }
    }
    previousColors = next; return next;
  });
  const color = (key: string) => colors().get(key) ?? "chart-other";
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
  const slot = (date: string) => { const at = new Date(date + "T00:00:00Z"); const from = new Date(props.view.period.from + "T00:00:00Z"); return props.view.granularity === "monthly" ? (at.getUTCFullYear() - from.getUTCFullYear()) * 12 + at.getUTCMonth() - from.getUTCMonth() : Math.round((at.getTime() - from.getTime()) / 86400000); };
  const slots = () => Math.max(1, slot(new Date(Date.parse(props.view.period.to + "T00:00:00Z") - 86400000).toISOString().slice(0, 10)) + 1);
  const step = () => Math.max(80, width() - 80) / slots();
  const x = (index: number) => 60 + slot(points()[index]?.date ?? props.view.period.from) * step();
  /** Bars stack list-price spend; the credited slice of each segment is the band between its net and gross edges. */
  const bars = (point: CostPoint) => {
    let positive = 0; let negative = 0;
    return point.contributors.map((item) => {
      const value = Number(item.amount); const base = value >= 0 ? positive : negative;
      if (value >= 0) positive += value; else negative += value;
      const credit = Math.min(Math.max(0, -Number(item.credits)), Math.abs(value));
      const gross = base + value;
      const netEdge = value >= 0 ? gross - credit : gross + credit;
      return { ...item, top: y(Math.max(base, gross)), height: Math.abs(y(base) - y(gross)), creditTop: y(Math.max(netEdge, gross)), creditHeight: credit > 0 ? Math.abs(y(netEdge) - y(gross)) : 0 };
    });
  };
  const previous = () => points().map((p, i) => p.previous === null ? "" : `${i === 0 || points()[i - 1]?.previous === null ? "M" : "L"} ${x(i) + step() / 2} ${y(Number(p.previous))}`).join(" ");
  const describe = (amount: string, credits: string) => hasCredits(credits) ? `${money(amount, props.view.currency)} spend, ${money(credits, props.view.currency)} credits, ${money(net(amount, credits), props.view.currency)} net` : money(amount, props.view.currency);
  return <div class="cost-chart" ref={(element) => { host = element; }}>
    <svg viewBox={"0 0 " + width() + " 252"} role="group" aria-label={`Spend by ${props.view.group}, ${props.view.currency}. Dotted bands show credits applied to that spend. Select a bar to filter spending. Values are available in the table below.`}>
      <defs><pattern id={dots} patternUnits="userSpaceOnUse" width="6" height="6"><circle class="chart-credit-dot" cx="3" cy="3" r="1.1" /></pattern></defs>
      <For each={[0, 1, 2, 3, 4]}>{(tick) => <g><line class="chart-grid" x1="60" x2={width() - 14} y1={22 + tick * 46} y2={22 + tick * 46} /><text class="chart-axis" x="52" y={26 + tick * 46} text-anchor="end">{(extremes().max - tick / 4 * extremes().span).toLocaleString(undefined, { notation: extremes().max >= 10000 ? "compact" : "standard", maximumFractionDigits: extremes().max < 10 ? 4 : 0 })}</text></g>}</For>
      <line class="chart-zero" x1="60" x2={width() - 14} y1={y(0)} y2={y(0)} />
      <For each={points()} keyed={(p) => p.date}>{(point, index) => <g>
        <For each={bars(point())} keyed={(bar) => bar.key}>{(bar) => <>
          <rect class={`chart-bar ${color(bar().key)}`} x={x(index()) + 2} y={bar().top} width={Math.max(1, step() - 4)} height={Math.max(0, bar().height)} tabindex="0" role="button" aria-label={`${point().date}, ${contributorLabel(bar().key)}, ${describe(bar().amount, bar().credits)}`}
            onMouseEnter={() => setFocusedDate(point().date)} onFocus={() => setFocusedDate(point().date)}
            onClick={() => props.select(point().date, bar().key)}
            onKeyDown={(event) => { if (event.key === "Enter" || event.key === " ") { event.preventDefault(); props.select(point().date, bar().key); } }}><title>{point().date} · {contributorLabel(bar().key)} · {describe(bar().amount, bar().credits)}</title></rect>
          <Show when={bar().creditHeight > 0}>
            <rect class="chart-credit-fade" x={x(index()) + 2} y={bar().creditTop} width={Math.max(1, step() - 4)} height={bar().creditHeight} />
            <rect class="chart-credit-dots" fill={`url(#${dots})`} x={x(index()) + 2} y={bar().creditTop} width={Math.max(1, step() - 4)} height={bar().creditHeight} />
            <rect class="chart-credit-edge" x={x(index()) + 2.5} y={bar().creditTop + 0.5} width={Math.max(0, step() - 5)} height={Math.max(0, bar().creditHeight - 1)} />
          </Show>
        </>}</For>
        <Show when={index() % Math.max(1, Math.ceil(points().length / (width() < 500 ? 3 : 6))) === 0}><text class="chart-axis" x={x(index()) + step() / 2} y="234" text-anchor="middle">{point().date.slice(5)}</text></Show>
      </g>}</For>
      <Show when={props.compare}><path class="chart-previous" d={previous()} /></Show>
    </svg>
    <div class="chart-legend"><For each={keys()}>{(key) => <button class="legend-button" onClick={() => props.select("", key)}><span class={`legend-swatch ${color(key)}`} />{contributorLabel(key)}</button>}</For><Show when={credited()}><span class="credit-legend"><span class="legend-swatch legend-credit" />Credits applied</span></Show><Show when={props.compare}><span class="previous-legend">Previous total</span></Show></div>
    <div class="chart-readout" aria-live="polite"><Show when={focused()} fallback={<span>{coverageNotice(props.view)}</span>}>{(point) => <><strong>{point().date} · {money(point().total, props.view.currency)}{hasCredits(point().credits) ? ` spend · ${money(point().credits, props.view.currency)} credits · ${money(net(point().total, point().credits), props.view.currency)} net` : ""}</strong><span>{point().contributors.map((c) => `${contributorLabel(c.key)} ${describe(c.amount, c.credits)}`).join(" · ")}</span></>}</Show></div>
    <Show when={!props.compact}><details class="chart-data"><summary>View accessible daily cost table</summary><div class="table-scroll"><table><thead><tr><th>Date</th><th class="numeric">Spend</th><th class="numeric">Credits</th><th class="numeric">Net</th><th class="numeric">Previous period</th></tr></thead><tbody><For each={points()} keyed={(p) => p.date}>{(p) => <tr><td><button class="text-button" onClick={() => props.select(p().date, "")}>{p().date}</button></td><td class="numeric">{money(p().total, props.view.currency)}</td><td class="numeric">{units(p().credits) === 0n ? "None" : money(p().credits, props.view.currency)}</td><td class="numeric">{money(net(p().total, p().credits), props.view.currency)}</td><td class="numeric">{money(p().previous, props.view.currency)}</td></tr>}</For></tbody></table></div></details></Show>
  </div>;
}
