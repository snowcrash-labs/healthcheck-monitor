# Monitoring gap closure

Make the dashboard dependable for two questions: whether the configured services are operating normally, and what their reported costs are. Connecting cloud identities and rendering a chart are prerequisites; neither establishes complete operational coverage or reconciled billing.

This plan follows the deployed investigation UI and cloud onboarding. Detailed work is split into [collection, history, and service health](2026-09-07-cloud-coverage-and-freshness.md) and [billing completeness and contract compatibility](2026-09-07-billing-completeness-and-contracts.md). It supplements the [dashboard implementation plan](2026-09-07-dashboard-structure-clouds-and-costs.md), rather than reopening completed navigation and chart work.

## Observed baseline

Four AWS accounts and two Azure subscriptions have dedicated workload identities and configured operational checks. A snapshot captured at 05:29 UTC on 2026-09-08 had 99 registrations, 86 stored check results, and 84,817 retained observations. Some AWS metric results were nearly an hour old; the Azure primary inventory result contained no observations. The process used approximately 808 MiB with an 898 MiB peak, against a 1 GiB hard limit. Native isolated probes had returned inventory and metrics. Raising state limits did not establish timely continuous coverage; the empty results need attribution to collection, retention, or publication before another capacity change.

All five billing sources have published. At the latest completed import audit, AWS and Azure research had 90 published days, current GCP had 84, and the oldest six GCP days were waiting for the daily scan allowance. Azure primary resumed after support for smaller exact charges was added. Imported days and invoice reconciliation are separate measures.

The subsequent billing incident exposed a contract gap: PostgreSQL and Rust accepted 38 fractional decimal places while the frontend still allowed 18. A valid billing response therefore rejected the entire page. The correction aligns frontend validation and integer arithmetic; regression coverage now includes a protected production response rendered in Firefox, Chromium, and WebKit. Future contract and deployment gates belong in the billing plan.

On 2026-09-08 at approximately 05:33 UTC, a live scope query returned busy/unavailable, and a summary query reported unavailable diagnostic history. Earlier successful queries reported dropped persistence records. These are monitoring reliability failures; their finding counts cannot establish complete fleet health. Historical loss, capped log progress, stale removed-resource findings, and missing flow definitions already have Linear work items.

## Priority and sequence

| Priority | Deliverable | Completion evidence | Existing work |
| --- | --- | --- | --- |
| Immediate | Repair the billing frontend contract and verify the embedded production bundle | The protected response renders across all billing surfaces; anonymous requests remain denied | Current regression repair |
| High | Reliable current-state publication and history delivery | No ordinary supported records dropped in a representative sustained run; current and historical check counts agree | [History reliability](https://linear.app/soundpatrol/issue/SOU-1480) |
| High | Fair collection, useful retained evidence, and truthful freshness | Every supported required check produces evidence on its configured cadence after warmup; busy scopes do not starve others | Collection plan; coordinate with history reliability |
| High | Correct removal and resumable log windows | Two complete inventories retire absent resources; busy logs advance without losing quieter workloads | [Resource removal](https://linear.app/soundpatrol/issue/SOU-1481), [log progress](https://linear.app/soundpatrol/issue/SOU-1483) |
| High | Passive evidence of actual service progress | Demand, processing, and completion distinguish a working service from a stalled pipeline | [Flow integration](https://linear.app/soundpatrol/issue/SOU-1485), under [core service indicators](https://linear.app/soundpatrol/issue/SOU-1401) |
| Medium | Complete the provider capability matrix and native console verification | Every discovered supported family has explicit collected state, evaluation, freshness, and working destinations | Collection plan |
| Medium | Complete billing history, attribution, and reconciliation | Coverage by date and source is explicit; source totals reconcile without double counting | Billing plan |
| Medium | Make routine delivery and recovery verifiable | Reviewed CI/CD configuration, application/browser contract smoke, credential refresh, and rollback rehearsal | Existing [deployment PR](https://github.com/snowcrash-labs/devops/pull/256) |

The four linked monitor defects are High-priority Todo issues assigned to Peter Younghyun Chi at planning time. Reuse them. Flow definitions depend on the existing indicator and instrumentation work; do not create a second telemetry program. New collection-capacity and billing-export tasks should reference this plan and be checked against existing work before tickets are created.

Complete reliability instrumentation first, then fix the measured rejection and starvation paths. Removal, log cursor work, and billing contract checks can progress independently. Expand provider evaluations after basic evidence survives collection and publication. Invoice attribution can proceed alongside that work, provided it has independent import admission and cannot consume health-query capacity.

## Delivery boundaries

Keep one Rust service with the Solid/Vite bundle embedded at build time. Retain IAP, existing reader membership, HTTP/2, native workload federation, PostgreSQL Unix sockets, and the current VM while measuring capacity. Application code and public design documents stay here; identifiers, permissions, export configuration, service settings, and deployment machinery stay in the private devops repository.

The monitor remains read-only: no remediation, queue consumption, customer database reads, secret values, synthetic transactions, or automatic notifications. Collection does not initiate login. Billing export reads require their own narrowly scoped permissions and projections. Provider console access remains independent of IAP access.

Memory, remote operations, intermediate decoding, history, and disk stay bounded. Pagination makes retained records reachable; it cannot restore discarded records. Do not treat a larger memory allowance, a registered provider, an HTTP 200, or a completed inventory as proof of complete health assessment. Preserve original observation times and explicit gaps across restarts.

Use stable released Rust and prebuilt standard libraries, with no release builds during this work. Resolve current compatible dependencies when implementation changes them, run the requested dependency upgrade workflow, and preserve the rustls transport boundary. No additional always-on service is needed for the planned work.

## Release gates and effort

Each change needs a failing regression or a measured baseline, focused implementation, Linux/macOS development checks, and verification against the production-shaped dataset. Frontend/API changes require browser checks against the same wire contract. Publish the deployment marker only after the installed binary, protected API, embedded assets, and affected user flow pass; infrastructure readiness alone is insufficient.

Before claiming steady-state readiness, run the configured workload for at least one hour and follow it with a 24-hour supervised observation. Record check scheduling delay, source freshness, rejected records by cause, API latency, memory, CPU, task/client counts, database queue depth, and disk growth. A compact runbook must show what remains unassessed and why.

Initial engineering estimates are 4–7 days for collection/publication reliability, 3–5 for lifecycle and log progress, 2–4 for provider capability verification, 2–5 for flow integration after telemetry is available, and 5–9 for billing imports/reconciliation and contract gates. These overlap and exclude external access, support-plan, instrumentation, and review delays. Re-estimate after the first measured full-workload run; they are not delivery-date commitments.

## References

- [Deployed billing behavior and validation](2026-09-07-dashboard-billing-operations.md).
- [Query access, freshness, and deployment assessments](2026-09-06-monitoring-query-operations.md).
- [Read-only boundary and security review](2026-09-07-monitor-security-review.md).
- [Existing history-loss work](https://linear.app/soundpatrol/issue/SOU-1480), [removal work](https://linear.app/soundpatrol/issue/SOU-1481), [log-window work](https://linear.app/soundpatrol/issue/SOU-1483), and [flow-integration work](https://linear.app/soundpatrol/issue/SOU-1485).
