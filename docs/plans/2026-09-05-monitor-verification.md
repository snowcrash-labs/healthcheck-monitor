# Monitoring verification

The [implementation plan](2026-09-04-configurable-health-monitor.md) is implemented with 225 passing tests, formatting and Clippy on macOS and Linux ARM64. Synthetic verification and the limited live checks below do not establish fleet health or replace validation with real credentials. The source bundle at `../memos/system-health/` was inspected read-only and remains unchanged.

## Source behavior crosswalk

The 30 source tests are behavioral references. The new tests exercise the shared Rust engine and typed evidence rather than a compatibility layer.

| Source group | Count | Rust coverage |
| --- | --- | --- |
| Queue health | 4 | [health_semantics.rs](../../crates/core/tests/health_semantics.rs): demand with crashing consumers, transient scale/recovery, persistent unready warnings, intentional zero replicas; single samples cannot prove persistence. |
| KEDA projection | 1 | [projection.rs](../../crates/integrations/tests/projection.rs): activation/replica metadata excludes authentication and environment values. |
| Configuration | 3 | [configuration.rs](../../crates/core/tests/configuration.rs): typed settings, duplicate-name rejection, HTTPS and exact statuses. |
| Diagnostic grouping | 3 | [projection.rs](../../crates/integrations/tests/projection.rs): volatile/customer values disappear, signatures group deterministically, runtime warnings retain warning classification. |
| Build retries | 2 | [provenance.rs](../../crates/core/tests/provenance.rs): later success supersedes only matching pipeline/revision/target/scope; unrelated revisions cannot hide failure. |
| Subprocess execution | 2 | [process_tests.rs](../../crates/integrations/src/process_tests.rs): both streams are collected under bounds, missing executables fail explicitly. |
| Desired GitHub state | 2 | [projection.rs](../../crates/integrations/tests/projection.rs): build targets are structural, unique and sorted; empty/missing targets fail closed. |
| Kubernetes health | 8 | [health_semantics.rs](../../crates/core/tests/health_semantics.rs), [schedules.rs](../../crates/core/tests/schedules.rs): intentional zero replicas, unready/restarting pods, historical Jobs, startup grace, old restart counters, CronJob success, node grace and draining nodes. |
| Release images | 2 | [projection.rs](../../crates/integrations/tests/projection.rs): digest and source revision are separate; suspended CronJob templates retain desired images without a live pod. |
| NATS report parsing | 2 | [projection.rs](../../crates/integrations/tests/projection.rs): numeric aggregate parsing and invalid-header rejection. |
| Log report ordering | 1 | [reporting.rs](../../crates/core/tests/reporting.rs): equal counts sort deterministically and warning noise stays out of the error table. |

The two confirmed defects have direct tests in [policy_regressions.rs](../../crates/core/tests/policy_regressions.rs): all endpoint failures reach the common evaluator, and successful Job terminal conditions precede attempt counters. Partial completion and contradictory terminal conditions are separate cases.

## Additional acceptance evidence

| Contract | Verification |
| --- | --- |
| Shared one-off/watch engine | Full, focused, single-sample and deep runs in [watch_behavior.rs](../../crates/core/tests/watch_behavior.rs). |
| Precedence, selection and reload | [configuration.rs](../../crates/core/tests/configuration.rs), [prerequisite_sampling.rs](../../crates/core/tests/prerequisite_sampling.rs), and schedule replacement/removal tests. |
| Failure isolation and bounded scheduling | [scheduling.rs](../../crates/core/tests/scheduling.rs) and [watch_behavior.rs](../../crates/core/tests/watch_behavior.rs): simultaneous failures, duration, cancellation, no same-check overlap and closed reload channels. |
| Sustained churn | A simulated 24-hour watch with more than 5,000 collections checks active tasks, retained observations/findings, serialized state and disk bounds under repeated failure and resource churn. This is not a production RSS measurement. |
| Provider contracts | Pagination, malformed responses, denial, expired authentication, throttling, unavailable services, missing metrics, per-query failures and global/regional resources in [provider tests](../../crates/providers/tests). |
| Shared reads and batches | [cache.rs](../../crates/providers/tests/cache.rs), [batches.rs](../../crates/providers/tests/batches.rs), [azure_metrics.rs](../../crates/providers/tests/azure_metrics.rs), and native CloudWatch serialization tests. |
| Freshness, recovery and replacement | [evidence.rs](../../crates/core/tests/evidence.rs), [partial_evidence.rs](../../crates/core/tests/partial_evidence.rs), [capacity.rs](../../crates/core/tests/capacity.rs), and UID-scoped restart/owner tests. |
| Storage and disk failure | [storage_faults.rs](../../crates/core/tests/storage_faults.rs) and [publication_tests.rs](../../crates/core/src/publication_tests.rs): interrupted writes, owner-only files, lock/restart age, retention, simulated ENOSPC, bounded retries and non-overlapping background writes. |
| Logs and sensitive data | [read_boundary.rs](../../crates/integrations/tests/read_boundary.rs), [log_windows.rs](../../crates/integrations/tests/log_windows.rs), [log_contracts.rs](../../crates/providers/tests/log_contracts.rs) and [metadata.rs](../../crates/providers/tests/metadata.rs): forbidden calls, response caps, redaction, bounded overlap/deduplication and metadata-only secret version reads. |
| Change-aware monitoring and flow progress | [changes.rs](../../crates/integrations/tests/changes.rs) and [flows.rs](../../crates/core/tests/flows.rs): changed workload selection, unknown mappings, stalled ready workers, new completions, zero demand, missing signals and counter resets. |
| SLOs and provenance | Native SLO budget/compliance tests, configured metric goals, runtime/registry/build/commit joins, multi-platform image relationships and prerequisite expiry tests. |

## Live and platform validation

Read-only development checks confirmed Kubernetes access through the configured context, KEDA metadata, the existing NATS utility's fixed aggregate report, and HTTP 200 with trusted DNS/TLS at the configured health path. A 65-second quick watch completed normally and published final evidence. Its incomplete cloud/edge coverage remains recorded; it does not establish full development health.

A focused development queue run with `--samples 1` returned exit 0 with complete selected coverage, 2,416 prerequisite observations and 20 queue/stream observations. It did not infer persistent queue failure or assess unrelated Kubernetes health. Missing aggregate progress mappings remain explicit in full development/API reports.

A subsequent 40-second development watch rejected a version-2 configuration on SIGHUP, then accepted a version-1 replacement that reduced memory to 128 MiB and changed queue cadence from five to ten seconds. It returned 0 on duration and published the replacement revision after eight Kubernetes attempts and five queue attempts. Its final evidence retained cancelled/stale operations with 2,416 Kubernetes and 20 queue observations, no persistence fault and no dropped transitions. A separate live SIGINT check returned 130 and published retained partial evidence within 0.1 seconds of the signal command completing. Evidence remains local under `evidence/runtime-reload-smoke` and `evidence/runtime-cancel-smoke`.

The dependency sweep used `cargo upgrade --incompatible` and found all 48 direct dependencies current. `cargo-audit 0.22.2`, installed with a development build, checked 482 locked dependencies against the advisory database updated September 2, 2026 and found no vulnerabilities or advisory warnings. The normal/build dependency tree uses rustls without native-tls or OpenSSL linkage; `openssl-probe` only locates certificate stores on Linux.

The final checkpoint passes 225 tests on both platforms. [removal_authority.rs](../../crates/core/tests/removal_authority.rs) verifies that optional API failures preserve findings/evidence, unrelated API failures do not block two successful inventories for a resource, and a late finding cap invalidates earlier recovery decisions. Its two successive 50,000-asset comparisons plus three lifecycle regressions completed in 0.70 seconds in a development build. The comparison indexes and retirement pruning avoid repeated full scans of prior evidence.

Linux development tests, formatting and Clippy ran in an isolated ARM64 Colima VM with 4 CPUs and 6 GiB RAM, using a container limited to 5 GiB and 512 processes. The workspace mount was read-only, with no SSH-agent forwarding or active Docker-context change. Rustup installed stable Rust 1.98.1 and prebuilt components. The base image digest was `sha256:620dbcd124499c59e2406d3741574b5c5838cf9eb9656f0c3a03948f79b02959`. Linux/macOS CI checks are also defined in [.github/workflows/checks.yml](../../.github/workflows/checks.yml); the repository has no remote, so that workflow has not run. No release builds or cloud infrastructure changes were performed.

The test container, named VM, Docker context and VM data disk were removed after verification. The prebuilt Colima/Lima tools and development-built cargo-audit remain installed; the local CLI inventory was updated accordingly.

The final full development run completed in 25 seconds after bounding GitHub history to the configured runtime window and separating identity checks from the inventory cache. All 17 selected checks and references contributed results. It published 4,067 Kubernetes observations, 20 queue observations, nine endpoint observations and 1,305 GitHub observations, with no persistence fault or dropped transitions. Exit 3 reflected missing GCP ADC and other incomplete required coverage. Eleven error findings were retained: six ExternalSecret readiness failures, two unavailable autoscalers and three unreachable endpoints. The report remains local at `evidence/final-full-dev-bounded/monitor-report.md`.

GCP ADC, applicable AWS/Azure credentials, direct NATS TLS access, production-scale RSS measurements and deployment-specific progress mappings remain live-validation gaps. Older source revisions outside the configured GitHub history window need an explicit repository reference or a longer window. These gaps do not reduce required implementation coverage; no database reads, synthetic work or infrastructure changes were used to bypass them.
