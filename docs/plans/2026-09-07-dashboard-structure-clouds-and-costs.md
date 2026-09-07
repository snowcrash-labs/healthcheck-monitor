# Dashboard structure, cloud coverage, and costs

Status: proposed implementation plan. Reviewed against application commit `1abfd83` on 2026-09-07. This pass changes documentation only. Existing uncommitted dependency changes are outside this work.

## Objective and decisions

Make it immediately clear what an administrator is looking at: which service or resource, in which environment and cloud, during which period, what is wrong, and what evidence supports that conclusion. Prioritize information structure, brevity, navigation, working investigation links, and stable updates. A visual redesign is secondary.

The overview should answer three questions: what needs attention, where it is happening, and what we are spending. Detailed collection mechanics belong under Checks. Add AWS and Azure through the existing provider adapters, with explicit deployment onboarding and service-by-service verification. Add costs as a separate ingestion and query domain within the same process, with a compact overview chart and a dedicated explorer.

Preserve Solid 2 RC, Solid Router, strict TypeScript and runtime response validation, centralized CSS, persistent navigation, light/dark/system modes, compile-time embedded compressed assets, Axum, PostgreSQL, Google IAP, and the one-off CLI. Keep the deployed application read-only. This plan does not authorize synthetic transactions, notifications, remediation, or new cloud grants by itself.

## Assessment of the current structure

The assessment uses source, existing configuration, and documented competitor workflows. No fresh authenticated browser walkthrough was performed in this pass. Reported broken links and flicker are treated as user-observed defects; their exact reproduction cases must be captured before implementation. Source findings below distinguish confirmed behavior from suspected causes.

| Current behavior and source | Why it is hard to use | Planned change |
| --- | --- | --- |
| `dashboard/src/overview.tsx` places error counts, incomplete checks, resource counts, and history storage in equal summary cards, followed by targets and all check cards. | Monitor internals compete with operational problems. Counts do not explain the affected service or failure. | Lead with affected services and a short attention list. Move healthy monitor/storage status into a compact status control; show a prominent notice only when it changes interpretation. |
| `check-card.tsx` repeats each check description, schedule, coverage wording, optional-gap wording, operation failures, timestamp, and navigation prompt for every target. | The overview becomes a repeated technical report. | Replace overview check cards with one coverage summary. Use compact check rows on Checks; explain check purpose once in its detail view. |
| `check-detail.tsx` repeats coverage explanations, requirement descriptions, page counts, attempts, timestamps, and run summaries. | Successful operations take as much reading effort as failures. | Default to failed or incomplete operations with concise reasons. Put successful operations and request mechanics in separate tabs or expandable sections. |
| `resources.tsx` leads with full resource IDs; each row includes console buttons, findings, facts, location, and freshness. | Variable row height and identifiers obscure the resource name and main problem. | Default to name/type, location, state, primary problem, and last confirmation. Move full identity, additional facts, and links to a preview or detail view. |
| `detail.tsx` and `resource-sections.tsx` stack current findings, location, contributing checks, all evidence, and history; `FindingDetail` repeats location and links inside each finding. | The same identity and navigation appear multiple times. | One identity header; Summary, Diagnostics, History, Configuration, and Cost tabs. Retain historical failure context where it differs from the current resource. |
| `layout.tsx` has a fixed “Operations / Infrastructure” breadcrumb and target-only global filtering. Routes have no shared time picker. | The header does not explain the current view; investigations lose scope and time context. | Route-derived breadcrumbs, explicit scope, a shared time control where supported, and persistent filter chips. |
| `history.tsx` uses a separate transition list and older history API; time-filtered query APIs already exist in `crates/query`. | Recent errors, current findings, and historical changes are disconnected. | One investigation flow over existing query contracts; distinguish current state from events during a selected period. |
| Global and per-panel coverage/freshness prose repeats similar caveats. | Necessary qualifications turn into background noise. | Show each limitation once at the narrowest relevant scope, with count and detail link. Never hide a limitation that changes the conclusion. |

Existing strengths to keep: clickable target cards, check drilldowns, retained triggering facts, provider-link allowlists, source timestamps, server-side search, bidirectional keyset pagination, three-page browser retention, request cancellation, and separate health/coverage/freshness semantics. These are implemented capabilities, not new deliverables.

Secondary accessibility defect: CSS token `--muted: #687b8c` has approximately 4.37:1 contrast on white panels and 4.04:1 on the page background; some explanatory text is only 10–11 px. Fix this with the structural work, but do not treat larger fonts as the solution to verbosity. Test against [WCAG 2.2](https://www.w3.org/TR/WCAG22/).

## Competitor comparison and what to adopt

| Reference | Relevant pattern | Application here |
| --- | --- | --- |
| [Datadog Resource Catalog](https://docs.datadoghq.com/infrastructure/resource_catalog/) | Searchable resource inventory, contextual details, related telemetry, and cloud-console navigation. | A concise resource list with a preview that preserves the list and filters; full detail remains directly addressable. |
| [Datadog Cost Explorer](https://docs.datadoghq.com/cloud_cost_management/reporting/explorer/) | Cost breakdowns connect chart exploration with a table and change detail. | Daily spend chart, ranked contributors, previous-period comparison, and drilldown using the same selected dimensions. |
| [Grafana Metrics Drilldown](https://grafana.com/docs/grafana/latest/visualizations/simplified-exploration/metrics/drill-down-metrics/) | Filter first, then investigate a selected metric and related evidence; time range and exploration state can be shared. | Preserve time/scope across findings, diagnostics, and resources. Put relevant evidence next to the failure rather than requiring unrelated navigation. |
| [New Relic infrastructure inventory](https://docs.newrelic.com/docs/infrastructure/infrastructure-data/infrastructure-ui-pages/infra-inventory-ui-page/) | Search and combined filters narrow an inventory around an investigation. | Provider, environment, service, location, and state filters with clear active chips and predictable reset behavior. |

These are workflow references, not a mandate to reproduce their navigation breadth, dashboard builders, raw telemetry storage, or visual style. The proposed product remains an opinionated health and cost console. It cannot claim end-to-end client usability from infrastructure health alone; missing processing-progress telemetry remains visible.

## Information architecture

Primary navigation: Overview, Problems, Resources, Costs. Secondary navigation under Monitoring: Checks and Monitor status. Preserve existing `/findings`, `/history`, `/targets/:target`, and check/resource URLs as compatible entry points. History becomes a Problems tab and resource detail tab, rather than a competing top-level concept. Use “Problems” as the UI label while retaining finding identity and existing API terminology.

The persistent header shows the route title/breadcrumb, selected scope, time range, live/pause control where applicable, and theme control. Use “Environment” only for actual environment mappings; a monitoring target can also be an operations project or source repository. Show provider-native labels such as Project, Account, and Subscription. Do not label every provider scope “Project.”

Define a small service catalog in configuration: stable service key, human name, environment, ownership where known, and explicit resource selectors. Reuse existing observed service labels where verified. Keep unmapped resources visible as Unassigned; do not infer ownership or application dependencies solely from similar names. A billing service such as Compute Engine is a separate dimension from an application service such as processing workers.

| View | Default content | Detail on demand |
| --- | --- | --- |
| Overview | Short health summary, up to five highest-priority problem groups with total and “View all,” compact environment/service rows, daily cost chart, meaningful collection gap summary. | Full problem list, target/service detail, Checks, Costs. No full check catalog or repeated descriptions. |
| Problems | Active problems by default; columns for problem, affected service/resource, environment/location, severity, first detected, last confirmed. Separate Recent errors and Changes tabs. | Triggering values, expected values, redacted diagnostics, related changes, native links, and source evidence. |
| Resources | Searchable rows with human name/type, environment/cloud, health, primary problem, last confirmation. | Preview panel, full resource identity, detailed evidence and history, cloud-console actions. |
| Target/service detail | Identity, affected resources, current problems, coverage summary, and available service telemetry. | Checks, dependencies with verified relationships, history, and attributable cost. |
| Checks | Compact rows: name, target, collection status, main gap, last run; failures first. | What the check tests, schedule, prerequisites, complete operation list, retries, pagination, and run history. |
| Monitor status | Collection liveness, provider access, history gaps, persistence faults, last successful imports. | Technical diagnostics and configured limits. Browser connection is a separate status from collector freshness. |
| Costs | Daily/monthly spend, attribution, comparison, coverage, and source freshness. | Resource/service breakdown, allocation method, adjustments, source references. |

Example overview structure; values below are illustrative, not production observations:

```text
Overview                     All environments · Live
2 services need attention    1 monitoring gap

Problem                      Where                 Last confirmed
Worker restarted after OOM   Processing / dev      2 min ago
Queue is not progressing     Ingestion / api       1 min ago
View all 7 problems

Services                     State       Resources with problems
Processing / dev             Degraded    2
Ingestion / api              Degraded    1

Cost                         Last 30 days · USD · Updated yesterday
[daily stacked spend chart]  [total] [change vs previous period]
View costs                   Azure billing not connected
```

The cost card has an explicitly labeled independent billing period; a 15-minute incident selection must not imply minute-level billing data. Once on Costs, its time selector uses billing periods. Shared provider/service scope carries across pages; unsupported filters are disclosed, never silently ignored.

## Content rules and interaction behavior

1. A list row answers what, where, and when. Use one short problem title and one optional evidence line. Full sentences describing the monitor belong in help or check details, not every row.
2. Show human names first. Keep full IDs copyable in details. Distinguish resource type, application service, provider product, and environment using stable labels.
3. Use concise collection states: Collected, Partial, Access denied, Waiting, Stale. A Collected check is not a healthy service. Use separate health and evidence fields; avoid combined badges with ambiguous meaning.
4. Replace “8 of 12 checks have complete, fresh required evidence” with “Coverage: 8/12” plus a linked “4 need attention.” Explain the denominator once in Checks. Replace repeated “Optional; does not prevent complete required coverage” with “Optional” in the operation table.
5. Present known causes precisely: “Build results incomplete” rather than a generic “Incomplete evidence.” If the collector did not record the cutoff, say “Partial results; cutoff not recorded.” Do not manufacture a reason or remediation.
6. Group repeated instances of the same rule by verified service/workload for overview triage. Display both group and affected-resource counts. Preserve every underlying finding and distinguish suspected correlation from proven cause.
7. Default sorting is severity, then recent worsening/confirmation, with stable identity tie-breaking. Do not reorder the row under the cursor or keyboard focus on every update; queue reorder until refresh or show an update affordance.
8. Relative time in lists; exact UTC time available on focus/hover and in detail. Distinguish first detected, last confirmed failure, observation time, and ingestion time. Do not display the same absolute and relative timestamp twice in each row.
9. Empty states differentiate no matching problems, no configured resources, access denied, stale evidence, and unavailable history. Only claim no problems within the declared scope and evidence limits.
10. Preserve search, scope, sorting, selected tab, frozen time window, and scroll position in navigation/back behavior. Keep all retained rows reachable through search and pagination. Preview counts and DOM windows are presentation bounds, not hidden dataset cutoffs.

## Repair cloud investigation links

Confirmed source behavior: `crates/server/src/console_links.rs` maps a limited service-name set to console paths, falls back to GCP search using the native ID, and falls back to AWS console home for unsupported families. GKE pod routes use `region` as cluster location. `log_links.rs` generates GCP links only, uses available Kubernetes labels and absolute timestamps, and does not select resource-specific log fields for non-Kubernetes services. Current tests primarily establish origin/path/encoding; an AWS fixture even uses the GCP region `us-central1`. Those tests cannot prove destination correctness.

These are plausible explanations for the reported empty destinations, not proof of which link the user clicked. First capture representative failing resource, logs, build, and alert links using their allowlisted metadata. Compare the projected identity with the actual console destination in an authenticated work-account session. Check account/project selection, region versus zone, cluster name versus kubeconfig context, resource family aliases, log resource type, timestamps, encoding, resource deletion, and console permissions separately.

Implement a typed destination registry keyed by provider and native resource kind, not a loosely interpreted display service name. Define required identity fields for each destination. Add explicit cluster location, canonical cluster name, provider resource type, and log-source identity where the current context cannot express them. Reuse one generator for the dashboard, history, API, and MCP projections.

| Action | Destination contract |
| --- | --- |
| Open resource | Exact resource details with native project/account/subscription and region/zone. Validate GKE regional and zonal clusters, pods/controllers, Compute, Cloud Run, SQL, Redis, Pub/Sub, buckets, builds, and configured AWS/Azure families. |
| View logs | Correct log source/resource filters and an absolute incident window. Distinguish workload name from pod name. Link resource failures to relevant logs even when the observation is not itself a log sample. Add CloudWatch and Azure Monitor/Log Analytics destinations with required workspace/group metadata. |
| View alert or build | Exact provider alert/incident/build when a native ID exists. A local finding without a provider alert ID opens local evidence, not a fabricated cloud issue. |
| Fallback | Clearly labeled “Open service console” or “Search project” plus copyable identity. Never label a console home/search fallback as an exact resource link. Omit unavailable actions with a short reason in details. |

Keep fixed HTTPS origins, typed region/partition validation, encoded path/query/fragment components, and payload-free log queries. Historical resources may be gone; retained evidence and time-bounded logs remain useful. Do not treat a public console HTTP 200 or a login redirect as validation that the resource resolved. Native console access remains governed by the user's provider permissions, independently of dashboard IAP access.

Acceptance requires fixtures from actual collector projections, generated-to-parsed round trips, malicious identifier tests, correct absolute time windows, and authenticated click-through evidence for each configured resource family. Public console routes are not stable APIs; keep mappings isolated and record verification date so fixes stay small.

## Eliminate refresh flicker

Source candidates: `context.tsx` triggers refreshes from both revision events and a 15-second poll; every successful response replaces object arrays. `useQuery.ts` retains data on same-path refresh but clears it on path changes. `usePages.ts` retains three pages and restores a row anchor after publication. Reconciliation behavior under the selected Solid RC, loading labels, transient connection notices, list replacement, and anchor restoration need browser measurement before declaring a root cause.

Capture a short browser trace on Overview, a scrolled resource list, and an open resource detail while evidence changes and SSE reconnects. Record request cadence, DOM replacement, layout shifts, focus, expanded sections, and scroll position. Use Firefox as a required reproduction browser alongside Chromium; check WebKit for macOS support.

Separate initial loading, background updating, and failed refresh states. Keep valid existing content during background reads; patch changed entities by stable IDs after verifying the Solid RC's reconciliation semantics. Reconcile related overview/list results against a generation or snapshot token so counts and rows do not oscillate between revisions. Coalesce refresh triggers and use polling as a connection fallback where appropriate. A scope change must never relabel old data as belonging to the new scope; use a scope-keyed loading state.

Reserve space for status messages, keep refresh text from shifting columns, and avoid whole-page skeletons on background updates. Preserve tab selection, preview panel, expansion, input focus, text selection, and scroll anchor. A user-paused historical view stays frozen even while collection continues. Retain bounded page windows and cancel obsolete requests; do not solve flicker by retaining an unbounded copy of the fleet.

## AWS and Azure integration

Existing AWS code includes official SDK clients, service catalogs, metrics batching, operational details, alarms, bounded logs, discovery, and native credential handling. Azure has ARM/Resource Graph adapters, metrics, operational projections, logs, activity, registry access, and cached `azure_identity` credentials. The reviewed devops deployment config enables GCP dev, api, operations, and GitHub source targets; no AWS/Azure credential profiles or targets are configured there. Code presence does not establish live coverage or evaluator correctness.

Start with an account/subscription and service inventory, recording owner, region, environment, expected state, telemetry source, and billing scope. Reconcile discovered resources against explicitly enabled deep-monitoring targets. Show discovered-but-unmonitored and unsupported resources without marking them healthy. Do not guess account or tenant IDs. Record these inputs in private deployment configuration, not public repository billing fixtures.

| Domain | AWS acceptance surface | Azure acceptance surface |
| --- | --- | --- |
| Compute and containers | EC2/ASG/EBS, EKS workload access, ECS/Fargate deployment/task health, Lambda errors/throttles/concurrency. | VM/VMSS/disks, AKS workload access, Container Apps revisions, App Service/Functions availability and failures. |
| Edge and networking | Load-balancer target health, Route 53, CloudFront, ACM, applicable network metadata. | Load balancers, Application Gateway backends, Front Door, DNS, certificates and network controls. |
| Data and storage | RDS/Aurora state/replication/backups, ElastiCache capacity/evictions, DynamoDB throttles/capacity, S3 configuration and failure metrics. | SQL/PostgreSQL state/pressure/backups, Redis capacity, Cosmos DB availability/throttles, Storage failures and recovery configuration. |
| Messaging | SQS backlog/age/dead letters, SNS/EventBridge delivery failures, correlation with consumers. | Service Bus queues/dead letters, Event Hubs lag where available, Event Grid delivery, consumer correlation. |
| Release and recovery | ECR, CodeBuild/CodePipeline, backups, KMS/Secrets metadata. | ACR, deployment/activity metadata, Key Vault metadata, backups. |
| Telemetry and governance | CloudWatch metrics/alarms/logs, AWS Health entitlement, quotas, organization/account/region discovery. | Monitor metrics/alerts/logs/diagnostics, Resource/Service Health, quotas, tenant/subscription/resource-group discovery. |

For each configured family, track inventory, operational observations, evaluated rules, console links, permissions, contract tests, and live verification separately. Close missing implementations instead of declaring inventory sufficient. Empty telemetry, unavailable entitlements, unsupported metrics, denied APIs, and intentional inactivity are distinct outcomes. Database checks remain provider telemetry only; never connect to application databases or consume messages.

Use workload federation from the existing GCP VM identity rather than copying developer sessions. For AWS, implement and verify the Google OIDC to STS exchange and per-account role policy with tightly bound subject/audience claims; Google's `aud`/`azp` mapping needs explicit tests. For Azure, federate the GCP service account to an Entra application and exchange short-lived assertions through supported native credential paths. The existing adapter may need a credential provider addition; do not assume an Azure managed identity is directly available on a GCP VM. See [AWS's GCP workload identity guidance](https://aws.amazon.com/blogs/security/access-aws-using-a-google-cloud-platform-native-workload-identity/) and [Microsoft's GCP federation tutorial](https://learn.microsoft.com/en-us/entra/workload-id/workload-identity-federation-google-cloud).

Keep identities, trust policies, read grants, target configuration, and export setup in the devops repository. Separate monitoring permissions from billing permissions. Review exact API actions, including read-only POST queries, and exclude secret values, database records, queue reads, workload environment values, and mutations. Reuse SDK/HTTP clients, batch metrics, limit concurrency per scope, and verify a throttled provider does not delay GCP checks. Native SDK collection remains the default; local interactive helpers remain login-only exceptions.

## Cost overview and explorer

“Across all services” means every connected billing scope, including resources outside deep health-monitoring targets, shared infrastructure, network egress, load balancing, DNS, logging/metrics, storage, backups, support, commitments, and applicable credits/adjustments. Inventory counts and list prices cannot substitute for billing data. List missing billing accounts, projects/accounts/subscriptions, time windows, and external vendors explicitly. A cloud-only total must be labeled Cloud cost; do not label it total company spend while SaaS/API charges are absent.

Overview default: last 30 days of daily spend, selected currency, total, comparison with the preceding equal period, top contributors, and “Data through” timestamp. Use stacked daily bars by provider initially, with service/environment grouping available. Costs page adds monthly aggregation, provider product versus application-service dimensions, account/subscription/project, region, tags, resource, and charge category. Clicking a chart segment filters the ranked table; rows open attributable resources or billing details. Include a keyboard-accessible table equivalent. Negative adjustments must remain visible rather than being clipped from a positive-only stack.

Show a complete daily total even when only top contributors are drawn: aggregate the remainder into Other, which remains searchable and pageable. Do not aggregate currencies without a recorded conversion rate/date/source; default to separate currency totals. Mark incomplete days and source gaps as incomplete, not zero. Forecasts are a later optional feature; any projection is labeled an estimate and excludes incomplete periods or discloses the method.

### Source selection and accounting

| Source | Proposed ingestion | Prerequisites and limits |
| --- | --- | --- |
| GCP | Parameterized, partition-filtered aggregates from a dedicated BigQuery billing export/view using native authentication and bounded REST/SDK requests. | Reuse an existing standard/detailed or FOCUS export where available. Dedicated dataset/view read access plus query-job permission; enforce maximum bytes billed. Resource-level detail varies by service; initial/backfill availability depends on export setup. [Export documentation](https://docs.cloud.google.com/billing/docs/how-to/export-data-bigquery). |
| AWS | Native SDK reads of CUR 2.0 or supported FOCUS exports in an allowlisted S3 prefix, processing manifests and bounded batches. | Read-only export objects and required decrypt permission. Export versions may revise prior periods; do not append every refresh as new spend. Cost Explorer can bootstrap aggregate views if explicitly chosen, with its request cost/granularity limits documented. [Data Exports](https://docs.aws.amazon.com/cur/latest/userguide/dataexports-create.html), [report update semantics](https://docs.aws.amazon.com/cur/latest/userguide/what-is-cur.html). |
| Azure | Cost Management Actual/Amortized or FOCUS export manifests and blobs, read using native credentials; use a bounded query adapter where exports are unavailable. | Billing-scope eligibility and storage read rights differ from subscription monitoring rights. Preserve export versions and late corrections. [Cost Management exports](https://learn.microsoft.com/en-us/azure/cost-management-billing/costs/tutorial-improved-exports). |
| External services | Provider billing API/export adapters or explicit reviewed invoice summaries once the vendor inventory is known. | Account owner, currency, billing period, provenance, and import freshness required. No customer usage payloads or invoices containing personal data in public fixtures. Unconnected vendors remain listed as missing. |

Use FOCUS concepts where available, while versioning each adapter and preserving provider-specific distinctions. Store billed cost and amortized/effective cost as different measures. Never sum both or add a payer total to its child-account totals. Show support, tax, marketplace charges, credits/refunds, reservation/savings-plan charges, and shared costs in explicit categories. The default cross-cloud chart uses normalized billed cost in the selected currency; effective-cost comparison is enabled only when each source supports a compatible definition. Reconcile against provider totals for the same period, basis, and currency; report any difference rather than silently adjusting it.

Map charges to resources by canonical provider identity and time-valid scope. Map resources to application services through explicit selectors or verified tags. Historical mappings must not change merely because a current label changed. Keep Unallocated and Shared categories. Kubernetes namespace/workload cost requires an allocation method and utilization/request data; node charges and allocated pod shares are alternative views of the same spend, never additive totals. Do not claim precise per-tenant cost without actual attribution data.

### Ingestion, storage, APIs, and access

Create a cost domain independent of health observation expiry and check cadence. Poll export manifests at a configurable hourly interval, process changed partitions only, and re-read recent/open billing periods for corrections. Start with 90 days of backfill where available; retain daily aggregates for 13 months by default. These are configurable planning defaults, subject to measured storage and source availability.

Use one bounded import worker initially, streaming decompression/record batches and resumable checkpoints. Prefer source-side aggregation for large exports; if S3/Azure export volume exceeds the small VM's measured memory envelope, use a separately costed provider query path rather than loading whole files. Bound bytes, decompressed size, records, query scans, temporary disk, and execution time. A stopped import is resumable and incomplete; limits never silently remove services from totals. Billing work must not occupy health collection permits or readiness tasks.

Keep source exports as authority. PostgreSQL holds bounded derived daily aggregates, attribution mappings, source revision/checkpoint references, and import/reconciliation status, not unrestricted copied billing ledgers. Proposed entities: billing source, import revision, daily cost aggregate, and time-valid service allocation. Follow existing Diesel migrations/schema conventions, PostgreSQL native `uuidv7()` primary keys, prefixed columns, UTC `timestamptz`, constrained enums and ISO currency codes, and exact `numeric` amounts. Return decimal amounts as validated strings across JSON/TypeScript. Define uniqueness by source, period, dimensions, measure, currency, and allocation version; atomically replace a partition only after a complete successful import.

Add versioned cost summary, series, breakdown, and source-status read endpoints using the existing query/error/auth conventions. Responses include requested/available period, basis, currency, source freshness, completeness, grouping, and pagination. Aggregate charts server-side at a resolution appropriate to the range; paginate breakdown rows and expose Other without losing totals. Retained temporal health APIs supply investigation windows; current-state APIs must not pretend to support historical reconstruction they do not have.

Keep Google IAP on every route and asset. Treat billing data as an explicit access decision: initially expose source configuration/status only to administrators and enable cost amounts only after confirming the intended reader group. If costs require narrower readership than healthcheck-access, enforce a dedicated backend capability based on verified identity and reviewed membership mapping; hiding a tab is insufficient. Do not expose billing-account names, negotiated rates, or secrets in public fixtures, logs, or unauthenticated errors. Existing API/MCP clients must retain compatible behavior; cost query tools can be added over the same contracts after authorization is defined.

## Delivery order and acceptance

| Order | Deliverable | Completion evidence |
| --- | --- | --- |
| 1 | Capture current admin tasks, failing console links, and refresh traces; define concise page/row content and wireframes using representative redacted fixtures. | A new coworker can identify what a view represents and reach the main problem without interpreting collector terminology. Fixtures include healthy, degraded, stale, denied, partial, empty, and long-identifier cases. |
| 2 | Structural UI changes, progressive disclosure, route state, compact check tables, resource preview/detail tabs, and concise limitations. | From Overview, locate what/where/when and open evidence in at most two navigation steps. Checks remain fully inspectable. Back/forward restores filters and position. No repeated identity blocks or full check descriptions on Overview. |
| 3 | Typed console/log destination registry and stable refresh behavior. | Authenticated GCP click-through for resource/log/build/alert examples; realistic AWS/Azure contract fixtures. A 60-second update/reconnect test preserves focus, selection, expanded content, and visible row position without whole-panel disappearance. |
| 4 | AWS/Azure target inventory, federation, least-privilege IaC, adapter gap closure, and explicit coverage matrix. | Each configured family has operational evidence, rules, links, and read-only live verification. Expiry/denial/throttling affect only dependent work. No credentials copied from personal sessions. |
| 5 | GCP billing ingestion and cost domain with overview chart/explorer, then AWS and Azure exports and attribution. | Source totals reconcile by period/currency/basis; missing providers remain visible until connected. Corrections, retries, duplicate manifests, partial files, multiple currencies, negative charges, and resource deletion are tested. |
| 6 | Cross-cloud cost completeness, service mappings, documentation, and production validation. | Shared/unallocated/external-service coverage is explicit. Deployment and rollback documented in devops; operational runbook covers federation, export lag, broken links, source failure, and access removal. |

Orders 2 and 3 should ship before waiting for billing exports or cloud access. Scope and cost authorization decisions should be settled while those changes are underway. Build API/schema changes before dependent UI changes, and use additive migrations and compatible routes for rollback. Refresh frontend design conventions in `docs/design/fe/dashboard.md` and backend conventions in `docs/architecture/be/design.md` when the implementation is accepted.

Browser acceptance includes Firefox, Chromium, and WebKit; keyboard-only navigation; light/dark modes; 200% zoom; desktop and narrow widths; retained scroll across paging; deep links; and unavailable/reconnecting states. Measure interaction latency and layout shifts under a representative large fleet. Initial target: local input feedback under 100 ms, warm paged reads under 500 ms p95 on the deployed VM, and no growing browser/server memory over a one-hour churn/import test. These are acceptance targets to measure, not current performance claims.

Backend tests cover cursor/filter binding, scope isolation, stale evidence, incomplete history, exact decimal aggregation, complete-partition publication, cancellation/restart, bounded memory/disk, auth failures, and malicious metadata. Existing history-loss and capped-log-window defects must be repaired or explicitly gate historical completeness; a cleaner UI cannot repair absent evidence. Processing-progress telemetry remains a dependency for claims about actual service usability.

At implementation time, resolve current dependencies and run the requested `cargo upgrade --incompatible` sweep separately from behavior changes; update frontend locks and audit compatibility with the requested Solid RC. Use stable released Rust and prebuilt standard libraries. Run development formatting, Clippy, TypeScript, unit/contract/browser tests, dependency/TLS audits, and Linux/macOS checks. No release builds during this work. Keep one binary, the existing private VM/IAP layout, and bounded PostgreSQL storage; no additional always-on dashboard service is required.

## Inputs to resolve during implementation

- Exact AWS organization/accounts, Azure tenants/subscriptions, deployed services, and approved read identities; verify through existing devops configuration and authorized read-only discovery.
- Billing accounts/export locations, historical availability, source currencies, external vendor inventory, and who may view cost amounts. Do not infer this from health-monitoring access.
- Application-service/environment mappings and shared-cost allocation rules; leave unknown mappings explicit until established.
- Representative authenticated broken-link cases and recorded flicker traces. Do not declare either repaired from URL syntax tests or source inspection alone.
- Measured export volume and small-VM headroom, including the cost of BigQuery/other query processing. Determine whether source-side aggregation is needed before expanding storage or compute.

## References

- [Existing dashboard design](../design/fe/dashboard.md) and [backend conventions](../architecture/be/design.md).
- [Original monitoring scope](2026-09-04-configurable-health-monitor.md), [dashboard diagnostics](2026-09-05-dashboard-diagnostics.md), and [monitoring query design](2026-09-06-monitoring-query-access.md).
- [Query access and operations](2026-09-06-monitoring-query-operations.md) and [security review](2026-09-07-monitor-security-review.md).
- [SOU-1480: repair monitoring history loss](https://linear.app/soundpatrol/issue/SOU-1480), [SOU-1483: make capped log collection resumable](https://linear.app/soundpatrol/issue/SOU-1483), and [SOU-1485: add processing-progress telemetry](https://linear.app/soundpatrol/issue/SOU-1485). Reuse these existing work items rather than duplicating them.
