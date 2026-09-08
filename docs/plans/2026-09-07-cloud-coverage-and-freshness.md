# Cloud coverage, freshness, and history

The next operational milestone is useful, fresh evidence from every configured supported scope, retained reliably enough for current health, recent errors, and short post-deployment assessments. This implements the operational portion of the [gap-closure plan](2026-09-07-monitoring-gap-closure.md). It does not add synthetic transactions or remediation.

## Measure the loss and delay paths

Instrument scheduled time, actual start, first observation, source completion, evaluator completion, current-view publication, and durable acknowledgment. Record target/check, bounded operation family, configuration revision, and fixed failure categories. Do not use resource IDs, request IDs, or log signatures as unbounded metric labels.

Expose separate counters for response truncation, pagination limits, scan budget rejection, cache rejection, retained-state truncation, record validation, recorder capacity, journal bytes, journal channel admission, database timeout, and retention eviction. Include oldest pending work and the affected coverage interval. Current generic dropped-event counters and busy responses do not identify the corrective action.

Profile the configured workload before increasing limits again. The current implementation serializes observations to estimate retained bytes, clones cached results and snapshots, and accounts for observations independently across check results. Large quota inventories can occupy state needed by later checks. These are concrete candidates, not a proven single cause of every delay. Compare sampled CPU and allocation profiles with per-stage timings.

Code entry points are [scheduling](../../crates/core/src/scheduler.rs), [state retention](../../crates/core/src/state.rs), [byte accounting](../../crates/core/src/bounds.rs), [inventory reuse](../../crates/providers/src/inventory_cache.rs), [endpoint collection](../../crates/providers/src/endpoint_scan.rs), [query publication](../../crates/server/src/query_publish.rs), and the [history journal](../../crates/history/src/journal.rs).

Acceptance: a one-hour capture can attribute every missing check or dropped record to a named stage and distinguish provider delay from local contention. Collection and API telemetry must remain available when the history database is unavailable.

## Fair collection and retained evidence

Keep fair admission across targets and provider scopes. Prefer due operational state, alerts, and metric refreshes over an exhaustive optional catalog refresh. Coalesce missed ticks and retain the no-overlap rule for the same check. A slow inventory must not hold unrelated ready work behind a shared client or cache lock.

Inventory deployed resources before expanding quota catalogs. Restrict detailed quota reads to relevant service/region combinations or explicitly configured capacity checks; retain a separate coverage outcome for unqueried catalogs. Do not let thousands of unused quota definitions displace real workload state. Quota evaluation needs a valid usage numerator and denominator; a quota limit alone is metadata.

Share immutable normalized observations across checks where identity, scope, source window, permissions, and freshness match. Retain one accounted allocation for shared evidence rather than charging and cloning the same large inventory for every consumer. Keep bounded `scc` caches, explicit eviction, and source timestamps. A cache hit cannot create a new observation time or clear a finding using stale evidence. Failed or incomplete reads cannot replace a valid complete inventory silently.

Replace repeated whole-snapshot byte estimation with accounting maintained as bounded objects enter and leave state, after profiling confirms the cost. Partition retention fairly across active scopes, with a common reserve for findings and critical operational observations. Eviction must preserve explicit missing coverage and cannot establish resource removal. If persistence format changes, version it and test restart from the existing snapshot without resetting age.

Acceptance:

- A synthetic idle account with thousands of quota definitions cannot starve a small target with real workloads.
- After warmup, supported required checks meet their configured intervals within measured jitter and operation deadlines; original timestamps satisfy the configured freshness rule.
- Inventory requests are coalesced, connections are reused, and metrics remain batched. Verify call counts against the fake transport and representative live scopes.
- Scope throttling, authentication expiry, and unavailable APIs do not delay independent providers.
- Memory, task, cache, and client counts plateau under churn; no ordinary operational evidence disappears merely because an optional catalog grew.

## Durable history and lifecycle

Extend [the existing history-loss work](https://linear.app/soundpatrol/issue/SOU-1480). The recorder must not advance its durable comparison state before journal acknowledgment. Use bounded pending batches with stable operation identities, acknowledge after the database transaction commits, and reconcile unknown commit outcomes by that identity. Apply backpressure or a bounded service-owned disk spool where necessary; if neither admission path succeeds, retain an explicit gap and retry state rather than forgetting the transition.

Align current checks, legacy run summaries, and richer query records. A completed release scan appearing in one table but absent from MCP history must be detectable. Keep source observation time, ingestion time, and persistence watermark separate. Read queries need independent connection/admission capacity so a backfill or large write batch does not turn every request into a history outage.

For [resource removal](https://linear.app/soundpatrol/issue/SOU-1481), derive candidates from retained findings and authoritative inventory scope, not only membership in the immediately preceding result. Two later complete inventories must retire a missing resource even if the first post-disappearance inventory was interrupted, or the process restored a finding without its prior observation. Failed, stale, and truncated inventories never count as confirmations. Persist removal and close the historical interval; do not fabricate recovery.

Acceptance includes database interruption, queue saturation, record rejection, restart before and after commit acknowledgment, and SIGTERM with pending work. A bounded shutdown can leave explicit unfinished evidence, but cannot acknowledge uncommitted history. Verify persisted check counts and lifecycle intervals against source runs and the current API.

## Logs that make forward progress

Implement [the existing log-progress work](https://linear.app/soundpatrol/issue/SOU-1483) using independent progress for error and runtime-failure windows. Track provider, scope, source partition, absolute interval, continuation state, deduplication boundary, and completion. Use bounded time-window subdivision or provider pagination; do not repeatedly query only the newest project-wide sample.

Schedule partitions fairly so a noisy workload cannot hide quieter services. When cursors expire, replay a bounded overlapping interval using stable deduplication; do not advance a window merely because its sample cap was reached. Save cursor progress only after the corresponding redacted evidence is admitted durably. Unsupported or permanently unavailable historical windows remain explicit gaps.

Acceptance: sustained errors above the configured cap still allow quieter workloads and older selected intervals to progress. Restart midway through pagination, truncate one window while another completes, and test duplicate timestamps and late records. Sample counts remain sampled; no capped sample establishes an exact error rate or healthy silence. Raw messages and customer payloads remain outside evidence and operational logs.

## Provider capability matrix

Maintain a generated or verified matrix per configured resource family: discovery, live-state read, metrics, alerts/events, redacted logs, evaluation rules, console links, required permissions, and last successful verification. Inventory-only, unsupported, disabled, denied, missing telemetry, entitlement unavailable, and stale must remain distinct. Empty regions with a complete inventory are different from failed regions.

| Provider | Validate first on discovered assets | Complete the remaining supported families |
| --- | --- | --- |
| GCP | GKE, Cloud Run, Compute, SQL/cache, Pub/Sub, load-balancer health, builds, Monitoring, and logs | Storage, DNS/TLS, Eventarc/Scheduler, backup/recovery, KMS/secret metadata, quotas, service health, and configured SLOs |
| AWS | Actual EC2/EBS/Auto Scaling, S3, registry/build resources, CloudWatch metrics and alarms in discovered regions | EKS/ECS/Lambda, load balancing, Route 53/CloudFront/ACM, databases/cache, messaging, backups, KMS/secret metadata, quotas, and provider health |
| Azure | Actual VMs/disks/networking, Storage, Key Vault metadata, monitoring workspaces, and metric definitions | VMSS/AKS, Container Apps/App Service/Functions, load balancing/Front Door, databases/cache, messaging, ACR, backup/recovery, quotas, and Resource/Service Health |

For each implemented family, test a healthy case, a real unhealthy state, expected inactivity, missing metrics, malformed data, denied permissions, pagination, and stale evidence. Record account/region or subscription/location correctly. EKS/AKS discovery does not prove access to their Kubernetes APIs; configure workload credentials/RBAC only for discovered clusters requiring deep checks. Do not label an implemented family verified when its live account contains no examples.

AWS Health access may require a qualifying support plan; the API documents `SubscriptionRequiredException` for unsupported accounts. Detect that outcome once per capability refresh, avoid repeated failing calls, and show a provider-health coverage limitation. Do not purchase a support plan automatically or substitute the public status page for account-specific health. See [AWS Health API access](https://docs.aws.amazon.com/health/latest/APIReference/Welcome.html).

For Azure Resource Health, verify registration, identity, scope, and supported resource type, then compare subscription, resource-group, and individual-resource reads. Registration alone did not resolve the observed authentication error. Evaluate the documented Resource Graph `HealthResources` query as a read-only alternative, preserving its resource-type and freshness limits. Keep an explicit gap if the provider still cannot supply the signal. See [Resource Health support](https://learn.microsoft.com/en-us/azure/service-health/resource-health-faq) and [Resource Graph health queries](https://learn.microsoft.com/en-us/azure/service-health/resource-graph-health-samples).

Metrics require source-specific cadence and dimensions. Daily storage metrics cannot be judged missing from a one-hour window; an idle resource can legitimately lack a series. Validate units, aggregation, capacity denominators, expected activity, and provider publication delay. Keep configuration-dependent queue age, latency, recovery objectives, and SLO thresholds explicit.

Verify native console destinations while authenticated to the intended cloud scope. Exercise resource, incident-log, build, and alert links from actual projected observations. Display account/tenant context and accurately label search/service-console fallbacks. Keep URL mappings isolated and record verification dates; URL syntax tests and login redirects do not prove the destination exists.

## Evidence of normal client operation

Implement [flow integration](https://linear.app/soundpatrol/issue/SOU-1485) under the existing [core-indicator work](https://linear.app/soundpatrol/issue/SOU-1401). Map existing aggregate demand, stage progress, successful completion, errors, latency, and consumer readiness to actual dev/API flows. Use the established OpenTelemetry/provider path and current queue-lane definitions. Do not inspect customer rows or create traffic.

Each flow definition needs scope, signal selectors, units, cadence, expected idle behavior, startup/drain grace, stall threshold, and recovery confirmation. Service owners must provide the business thresholds; until they do, show unknown required coverage. Zero demand with scale-to-zero differs from demand with no consumers, and ready pods without completions cannot establish a healthy pipeline.

Short post-deployment checks require fresh post-deployment observations and an equal preceding baseline. Missing stages, incomplete logs, grace periods, or unavailable provenance produce pending/incomplete outcomes. Tests must cover healthy progress, a stalled stage, backlog without workers, expected inactivity, a replaced pod, and telemetry loss. The UI should state which client-facing behavior is supported by evidence and which is still unassessed.

## Completion gate

Run a one-hour full-workload test plus a 24-hour supervised production observation with concurrent UI/MCP reads. Establish per-target freshness, no ordinary history drops, bounded memory/disk/client counts, and stable query latency. Retain a provider capability table and unresolved external limitations in the runbook. Infrastructure state, collection completeness, and service health remain separate in every result.
