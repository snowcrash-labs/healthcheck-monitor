# Configurable health monitoring

Status: implementation in progress. The service performs read-only observations of explicitly configured Soundpatrol targets. A one-off run and continuous monitoring use the same collection, evaluation, and evidence pipeline. Deployment, remediation, synthetic transactions, message consumption, notifications, application database connections, and historical evidence import remain outside this implementation.

## Execution and configuration

Provide `run`, `watch`, `report`, `diff`, `config validate --show-effective`, `auth status`, and explicit `auth login` commands. Support profiles, target/check/resource selection, prerequisite inclusion, adjustable intervals, one-off queue sampling, strict warning status, watch duration, and cancellation with five seconds for cleanup. Collection never initiates interactive authentication.

Resolve settings in order: built-in defaults, global settings, profile, target, check, explicit CLI overrides. Ship editable quick, full, and deep presets. Quick includes preflight, endpoints, current Kubernetes state, and one queue sample. Full covers every configured domain. Deep expands the log window to 24 hours and 5,000 entries. Reject invalid replacement configuration on SIGHUP without changing the active configuration; valid reloads replace schedules without overlapping a check.

Default collection intervals are 30 seconds for endpoints, Kubernetes, and queues; five minutes for metrics, logs, alerts, and releases; 15 minutes for inventory and continuity metadata; one hour for organization discovery. Queue runs take five observations 30 seconds apart unless overridden. Bound global operations at 16, each provider scope at four, and subprocesses at two. Defaults for connection, attempt, and operation timeouts are 10, 30, and 90 seconds, with three total attempts.

Retained application state is bounded by 256 MiB, 50,000 assets, and 10,000 active findings. Keep snapshots every five minutes for at most 24 hours, 288 snapshots, or 1 GiB. Bound responses, pagination, metric series, logs, subprocess streams, intermediate results, and all shared caches before accumulation. Use fair scheduling, jitter, coalesced missed ticks, reusable clients, shared inventories, batched metrics, and independent failure handling.

## Required coverage

| Domain | Coverage |
| --- | --- |
| Identity and scope | Selected principals, configured scope, credential availability and renewal, capability failures, explicit authentication helpers. |
| GitHub and desired state | Repositories, open PRs, configured workflows and deployments, structural desired-state parsing, source revisions, build targets, affected workloads from deployment changes. |
| GCP | GKE, Cloud Run, SQL, Redis/Valkey, regional/global Cloud Build, Monitoring configuration and time series, DNS, buckets, secret-version metadata, Pub/Sub, Eventarc, Scheduler, KMS, enabled APIs, Compute, instance groups, backend health, network controls, quotas, provider health, and configured SLOs. |
| AWS | Accounts and regions, EC2/ASG/EBS, EKS, ECS/Fargate, Lambda, load balancers, Route 53, CloudFront/ACM, RDS/Aurora, ElastiCache, DynamoDB metadata, S3 configuration, SQS/SNS/EventBridge, ECR, builds/pipelines, backups, keys/secret metadata, quotas, AWS Health, CloudWatch metrics/alarms/logs. |
| Azure | Tenant/subscription/resource-group discovery, Resource Graph, VM/VMSS/disks, AKS, Container Apps, App Service/Functions, load balancing/Application Gateway/Front Door, DNS/network controls, SQL/PostgreSQL, Redis, Cosmos DB metadata, Storage, messaging/event services, ACR, Key Vault metadata, backups, quotas, Resource/Service Health, Monitor metrics/alerts/diagnostics/activity/logs. |
| Kubernetes | Nodes, controllers, pods, Jobs/CronJobs, HPA/KEDA, warning Events, advertised routes, certificates, ExternalSecret synchronization, owner-based worker correlation, workload images. |
| Endpoints | Public routing discovery and configured health paths, DNS, trusted TLS and expiry, exact configured statuses, below-500 root reachability, latency, no response bodies. |
| Dependencies and flows | Service availability, capacity/headroom, replicas, continuity/recovery configuration, maintenance, encryption/key metadata, telemetry freshness, queue demand and consumer availability, pipeline and scanning progress. |
| Provenance and diagnostics | Desired/observed image digests, registry/build/deployment/repository/commit correlation, retry grouping, bounded redacted diagnostic signatures and missing-window reporting. |

Organization discovery is inventory only. Detailed checks remain restricted to explicit targets. Unsupported resources and inventory-only coverage are visible; inventory metadata alone does not fulfill operational health coverage. Missing credentials are live-validation gaps, not permission to omit provider implementations.

## Additional requirements from the July 10 checklist

The [source checklist](https://soundpatrol.slack.com/archives/D0B7J7697GW/p1783716940793489) describes post-cutover validation of Redis/Valkey consumers. Its operational requirements apply beyond that particular migration.

| Requirement | Relationship to the original plan | Implementation and acceptance |
| --- | --- | --- |
| Inspect recent deployment changes and identify Helm workloads whose dependency configuration changed. | Additional specificity for desired-state and release correlation. | Add an optional repository comparison/change window that returns affected workload identities. Parse supported configuration structurally; do not retain environment values, credentials, or unrestricted diffs. Test that unchanged workloads are excluded and ambiguous mappings remain explicit. |
| Check one hour of errors for workloads with active replicas while respecting KEDA scale-to-zero. | Already covered by sampling, logs, expected states, and grace periods. | Correlate diagnostic windows with selected active workloads. Absence of logs or zero replicas cannot prove workload removal. |
| Confirm pipeline processor completions and new task creation. | New explicit progress requirement beyond readiness and backlog. | Add configured aggregate progress signals for task creation and pipeline completions. Report stalled demand only after the configured observation window; require fresh producer and consumer evidence. Missing or capped signals produce unknown progress. |
| Verify query producer, scanning services, and audio-download StatefulSets are progressing. | Additional cross-service correlation requirement. | Represent the scanning path as configured stages and dependencies, including StatefulSets. Test a stalled middle stage despite ready pods, consumer recovery, empty-input inactivity, and stale upstream evidence. |
| Check Redis/Valkey health and free memory. | Already covered by managed-service capacity and provider metrics. | Prefer provider free/used memory and valid capacity denominators. Distinguish a healthy control plane from usable headroom and progressing consumers. |
| Query database task records and proxy directly to the cache. | Outside the existing V1 collection boundary. | Use aggregate provider/application telemetry that demonstrates task creation and cache headroom. Record unknown when that evidence is unavailable. Do not connect to application databases or consume queue messages. |
| Decommission the old Redis cluster after validation. | Outside implementation and explicitly separate in the source checklist. | Evidence can inform a later operational decision; this service performs no decommissioning or other infrastructure writes. |

## Health and evidence contracts

Collection success, coverage completeness, health, and flow progress are separate. Health states are healthy, degraded, unhealthy, unknown, and expected inactive. Findings use stable resource/rule identities, severity, expected state, original observation timestamps, confidence, and evidence references. A focused report cannot claim fleet health.

Evaluate Job terminal conditions before attempt counters. Complete with previous failed attempts is successful recovery history; Failed is terminal failure; partial success is incomplete; contradictory terminal conditions are invalid. DNS, TLS, connection, timeout, and unacceptable HTTP statuses must reach the shared policy evaluator. Compare restart counters only within the same pod UID and container identity.

Default node grace is five minutes; pod and rollout grace is ten minutes. Capacity thresholds are 80% warning and 90% error sustained for ten minutes with a valid denominator. Latency, queue age, flow-progress windows, recovery objectives, and SLO limits require configuration. Freshness defaults to twice the interval plus the operation deadline. Recovery requires fresh evidence and two clear evaluations; removal requires two complete successful inventories. Failed, stale, and truncated observations cannot establish recovery or removal.

Publish versioned JSON, compact Markdown, and rotated NDJSON transitions with atomic publication, owner-only permissions, and a single-writer lock. Restore the latest valid snapshot without refreshing its age. Record new, worsened, recovered, stale, removed, and reappeared findings. Ignore incidental timestamps, ordering, request identifiers, and ordinary metric fluctuations. Persistence failures remain explicit while watch collection continues within bounds.

One-off exit codes are 0 for no error findings or incomplete required coverage, 1 for health errors or strict-mode warnings, 3 for incomplete required coverage including combined failures, 2 for fatal configuration/output errors, and 130 for cancellation after partial output. Watch duration expiry is normal completion; the final report preserves health and coverage.

## Implementation and verification

Use edition 2024 and the latest released stable Rust with the prebuilt standard library. Use current compatible dependencies, run `cargo upgrade --incompatible`, and commit the lockfile. Use rustls, Tokio, structured redacted tracing, native credential providers, persistent clients, and `scc` for shared concurrent registries. Native async implementations can use explicit Send bounds and generic dispatch; runtime trait objects require explicit future boxing.

Complete the engine, configuration, scheduling, reload, state accounting, persistence, and reporting contracts. Complete provider operational detail and metric coverage, shared dependencies, provenance, redacted diagnostics, change-aware selection, and configured flow-progress evaluation. Keep source files under 300 lines and the original source bundle unchanged.

Acceptance includes synthetic equivalents of all 30 source tests; the strict-endpoint and Job regressions; one-off/full/focused/single-sample/deep/watch execution; configuration precedence and reload; non-overlapping fair schedules; independent simultaneous failures; provider pagination, malformed data, denied/unavailable APIs, expired credentials, throttling, missing metrics, and regional/global resources; cancellation and subprocess cleanup; stale/recovered/replaced resources; interrupted writes, disk failure, retention, and restart age; sustained bounded watch operation under churn; shared inventories and connection reuse; batched metrics; forbidden-operation and sensitive-payload checks; formatting, Clippy, dependency/TLS audits; Linux/macOS development verification; and read-only validation of explicitly configured live targets.

## Current implementation

The workspace implements shared one-off/watch execution, strict configuration and profile resolution, selected-scope reporting, atomic reload, bounded scheduling/state/history, native credentials, cloud metadata and operational adapters, Kubernetes/endpoints/NATS/GitHub, incremental redacted diagnostics, aggregate flow progress, SLOs, and runtime/registry/build/repository provenance. Native async traits use explicit Send bounds. The released stable toolchain and prebuilt standard library remain in use; development incremental compilation is disabled for reproducible Rust 1.98.1 trait-obligation cache failures.

Reload applies the tightest shared ceilings among selected checks and rejects obsolete credential generations. In-flight memory and subprocess reservations remain charged when limits shrink. A single background writer coalesces requests, retries disk faults with bounded exponential backoff, restores owner-only atomic evidence, and records dropped transitions when bounded history queues fill.

The provider audit has corrected global/regional builds, registry identity, Valkey versus Redis Cluster APIs, operational projections, pipeline execution state, AWS quota discovery, CloudFront metric routing/statistics, and service-specific pagination. Automatic AWS metrics now follow shared inventories and distinguish absent services from missing telemetry. GCP gauges retain current values; capacity readings retain sustained minima. Cloud Run latency, request/delivery failures, dead-letter counts and Redis Cluster telemetry have explicit presets.

Verification currently passes 196 tests and Clippy. All 46 direct dependencies are current after `cargo upgrade --incompatible`. The audit of 482 locked dependencies found no vulnerabilities or advisory warnings. The normal/build dependency tree uses rustls and excludes native-tls and OpenSSL. No release builds were performed.

Read-only development checks confirmed Kubernetes, KEDA, the fixed NATS report fallback, and the configured trusted-TLS HTTP 200 health route. A focused queue run returned exit 0 with 2,416 prerequisite observations and 20 queue/stream observations using one sample; it establishes neither persistence nor fleet health. A simulated 24-hour watch exceeded 5,000 collections while checking task, retained-state and disk bounds; it is not a production RSS measurement.

The [verification record](2026-09-05-monitor-verification.md) maps all 30 source tests and additional acceptance contracts to Rust tests. The [operations runbook](2026-09-05-monitor-operations.md) documents profiles, configuration, credentials/read permissions, evidence and Linux/macOS foreground supervision.

Remaining acceptance work includes the final provider/telemetry audit, configuration-to-collector validation, live checks of the finished runtime, and final documentation review. GCP ADC, applicable AWS/Azure credentials, direct NATS TLS access, Linux execution and deployment-specific aggregate flow mappings remain validation gaps. The repository has no remote; the prepared Linux/macOS workflow has not run. These gaps do not reduce required implementation coverage.

## References

- [July 10 operational monitoring checklist](https://soundpatrol.slack.com/archives/D0B7J7697GW/p1783716940793489).
- [August 27 health-dashboard discussion](https://soundpatrol.slack.com/archives/D0B7J7697GW/p1787880989263019).
- Source behavior baseline: `../memos/system-health/`, inspected read-only.
- [Stable async functions in traits](https://blog.rust-lang.org/2023/12/28/Rust-1.75.0/).
- [Rust trait-object compatibility](https://doc.rust-lang.org/reference/items/traits.html#dyn-compatibility).
