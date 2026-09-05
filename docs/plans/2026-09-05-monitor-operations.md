# Monitoring operations

The monitor runs in the foreground on Linux and macOS. It writes local evidence and performs documented read-only provider queries. It does not deploy infrastructure, consume messages, read application databases or secret values, initiate synthetic transactions, or send notifications. The implementation and acceptance audit remain in progress; use the [requirements plan](2026-09-04-configurable-health-monitor.md) and [verification record](2026-09-05-monitor-verification.md) together with this runbook.

## Configuration and scope

Start from `monitor.toml`. Each target names one provider scope: a GCP project, AWS account, Azure subscription, Kubernetes context, GitHub organization, or shared integration. Organization discovery can inventory other accessible scopes, but it does not register them for detailed monitoring. `artifact_targets` can reference another explicitly configured target for registry/build evidence without adding its unrelated health checks.

Settings resolve as built-in defaults, global `[settings]`, selected profile settings, target settings, global check settings, target check settings, and CLI overrides. Shared process, memory, asset, finding, concurrency, and history ceilings use the tightest resolved limit among selected checks. Provider-scope concurrency uses the tightest limit for checks sharing that provider scope. This prevents one check from expanding another check's shared budget. Per-response limits must still fit the resulting budget; inconsistent combinations fail validation.

`config validate --show-effective` prints resolved jobs and credential references without credential contents. Named credential file paths resolve relative to the configuration file. Unknown fields, duplicate targets, invalid versions, non-HTTPS health URLs, empty accepted-status lists, invalid resource identifiers, and inconsistent limits are rejected before collection.

| Profile | Default behavior |
| --- | --- |
| `quick` | Preflight, endpoints, current Kubernetes state, and one queue observation. |
| `full` | All configured domains, including metrics, alerts, SLOs, continuity, releases, redacted logs, and configured progress signals. Default for both modes. |
| `deep` | Full checks with a 24-hour, 5,000-entry error-log window. |

Profiles are editable TOML presets. A check's `enabled = false` removes it from scheduling. Required prerequisites are selected automatically; unavailable or disabled prerequisites leave dependent coverage incomplete. `--check`, `--target`, and `--resource` narrow the assessment. Prerequisite facts remain visible in reports without adding unrelated health findings. Resource selectors match resource identity substrings; use stable, unambiguous names.

```toml
version = 1

[settings]
concurrency = 16
scope_concurrency = 4
subprocesses = 2
history_interval = "5m"
history_age = "24h"
history_count = 288
history_bytes = 1073741824

[checks.logs]
interval = "5m"
log_window = "1h"
log_entries = 500
runtime_window = "24h"
runtime_entries = 250

[[targets]]
name = "example"
provider = "gcp"
scope = "example-project"
regions = ["us-central1"]
expected = "active"

[targets.checks.metrics]
interval = "2m"
capacity_warning = 80
capacity_error = 90
capacity_sustain = "10m"

[[targets.endpoints]]
name = "health"
url = "https://example.com/health"
accepted = [200]
```

The complete settings and target fields are defined in [settings.rs](../../crates/core/src/config/settings.rs), [patch.rs](../../crates/core/src/config/patch.rs), and [types.rs](../../crates/core/src/config/types.rs). Durations use values such as `30s`, `5m`, and `24h`. Latency, queue-age, recovery-age, burn-rate, and application progress thresholds require operator values. Capacity rules apply only with a valid denominator. Expected inactive states and per-resource expectations should describe intentional dormancy, suspended schedules, and scale-to-zero.

## Running and stopping

```text
cargo run -p healthcheck-monitor -- config validate --show-effective
cargo run -p healthcheck-monitor -- run --profile quick --target dev
cargo run -p healthcheck-monitor -- run --target api --check queues --samples 1
cargo run -p healthcheck-monitor -- watch --profile full --interval 60s --duration 30m
cargo run -p healthcheck-monitor -- report evidence/monitor-latest.json
cargo run -p healthcheck-monitor -- diff older.json newer.json
```

`run` attempts all selected checks, including independent checks after failures. Default queue sampling is five observations 30 seconds apart. A single observation cannot establish sustained queue failure or recovery. `watch` uses each check's interval, or one invocation-wide `--interval` override, until stopped or its duration expires. Missed ticks coalesce and a check does not overlap itself. Throttled scopes do not occupy waiting worker slots for independent scopes.

Send SIGHUP to reload the file. The full replacement is parsed and resolved before installation. Invalid replacements retain the prior revision. Valid replacements update schedules and shared limits, cancel removed checks, and discard results from the previous revision. Existing reservations remain charged after a memory or subprocess limit shrinks; new admissions wait until they fit. Send SIGINT or SIGTERM for partial-evidence shutdown. Duration expiry is normal watch completion. Cleanup and the final write share a bounded deadline; a stalled filesystem cannot prevent foreground termination.

Use a foreground supervisor that preserves stdout, delivers SIGTERM, and allows at least five seconds for cleanup. On Linux, a systemd service can run the development binary directly with an explicit working directory and `Restart=on-failure`. On macOS, launchd can run the same foreground binary through `ProgramArguments` and an explicit `WorkingDirectory`. Keep the evidence directory writable only by the monitoring identity. Supervision setup and deployment are separate operational actions; no service definitions are installed by this implementation.

| One-off status | Meaning |
| --- | --- |
| 0 | No error finding or incomplete required coverage in the selected scope. |
| 1 | Error-level health findings; `--strict` includes warnings. |
| 2 | Fatal configuration or output failure. |
| 3 | Incomplete required coverage, including runs that also have health errors. |
| 130 | User cancellation after attempting partial publication. |

Watch duration expiry returns 0 regardless of current findings; inspect its final evidence for health and coverage. Authentication failures affect dependent checks while independent checks continue. Collection resumes when usable credentials become available.

## Credentials and read permissions

Collection never starts interactive login. Use `auth status` to inspect selected identities and scope. `auth login <profile>` explicitly starts the configured browser helper: `gcloud auth application-default login`, `aws sso login`, `az login`, or `gh auth login --web`. Kubernetes uses the configured context and its credential plugin. Azure CLI authentication may invoke `az` for tokens; service collection uses native HTTP. Native AWS profiles support workload credentials, IAM Identity Center, and configured role assumption. GCP supports native ADC and named credential files. GitHub supports a token environment reference or its authenticated helper. NATS supports a TLS connection with a token environment reference or credential file.

Grant only the capabilities selected for a target. Permission denial, disabled APIs, unavailable entitlements, missing metrics, and truncated results remain explicit coverage outcomes. The monitor does not grant its own access or enable APIs.

| Integration | Required read capability families |
| --- | --- |
| GCP | Project/organization discovery; list/get metadata for configured Compute, GKE, Run, SQL, Redis/Valkey, builds, registry, DNS, buckets, Pub/Sub, Eventarc, Scheduler, KMS and secret versions; Monitoring time series, alert/SLO configuration and events; bounded Logging entries. Secret payload access is excluded. |
| AWS | STS identity and configured role assumption; Organizations/account/region discovery; service list/describe calls; CloudWatch metric/alarm queries; bounded CloudWatch Logs filtering; registry manifest metadata; continuity/key/secret metadata and provider health. AWS Health may require an account entitlement. S3 object reads, secret retrieval, KMS decryption, and message consumption are excluded. |
| Azure | Subscription/resource-group/Resource Graph reads; ARM resource and operational reads; Monitor definitions, metrics, alerts, diagnostics and activity metadata; Resource/Service Health; Log Analytics query permission for configured workspaces; ACR metadata/token scopes; Key Vault list and version metadata permissions. Management Reader access does not imply data-plane access. |
| Kubernetes | Get/list on selected workloads, nodes, Jobs/CronJobs, autoscalers, Events, routes, certificates and ExternalSecrets. The optional NATS fallback additionally requires exec access to the named existing utility deployment for the fixed aggregate command. |
| GitHub | Repository metadata, pull requests and comparisons, selected workflow/deployment metadata, desired-state file contents and commit references. Raw diffs and commit messages are excluded from evidence. |
| NATS | JetStream aggregate account/stream/consumer information. The fallback only runs the fixed stream report; it cannot run arbitrary shell commands or create a deployment. |

The executable read boundary is in [read_policy.rs](../../crates/integrations/src/read_policy.rs). Documented query POSTs are permitted, including CloudWatch, Resource Graph, Log Analytics, GCP log listing, backend health queries, and native registry token exchanges. Provider-specific endpoint catalogs and follow-up adapters define the requested operations. SDK logs are disabled; operational logs use fixed structured fields and never include raw provider responses, tokens, environment values, or diagnostic payloads.

## Evidence and interpretation

The output directory contains `monitor-latest.json`, `monitor-report.md`, dated snapshot JSON, rotated transition NDJSON, and a single-writer lock. Publication uses owner-only temporary files, fsync, and atomic rename. The latest snapshot is the publication marker. Restart restores the latest valid snapshot without refreshing evidence timestamps. Only recognized service-owned history and partial files are eligible for cleanup; unrelated operator files remain untouched.

A single background writer coalesces publication requests. Disk faults retain pending transitions within their configured bound and retry with exponential delays capped at one minute. Collection continues. A later successful write acknowledges only the captured transition prefix. If transitions exceed memory retention, the snapshot records a cumulative dropped count and the report marks history incomplete. JSON retains the complete bounded evidence set; Markdown limits large finding, endpoint, and diagnostic tables.

Collection success does not imply health. Reports distinguish healthy, degraded, unhealthy, unknown and expected inactive resources, plus complete, missing, stale, denied, unauthenticated, unavailable, cancelled and truncated operations. Failed or stale reads cannot clear findings or prove removal. Recovery normally needs two fresh clear evaluations; resource removal needs two distinct successful complete inventories. Configured health paths require their exact accepted statuses. Discovered roots use broad below-500 reachability and do not establish application health.

Logs retain diagnostic classes and aggregate counts, not raw messages. Reports show requested windows, sampled counts, duplicates, gaps and caps. Capped samples cannot establish healthy silence or an error rate. Pipeline progress requires configured aggregate demand and completion signals; readiness alone cannot establish progress. The checked-in development/API targets explicitly require these mappings and report a gap until the actual metric names are configured. See the [July 10 checklist requirements](2026-09-04-configurable-health-monitor.md#additional-requirements-from-the-july-10-checklist).

CloudFront metric reads use `us-east-1`, with per-metric statistics; event totals, current gauges and sustained capacity samples are kept distinct. See the [CloudFront metric contract](https://docs.aws.amazon.com/AmazonCloudFront/latest/DeveloperGuide/programming-cloudwatch-metrics.html) and [SQS metric definitions](https://docs.aws.amazon.com/AWSSimpleQueueService/latest/SQSDeveloperGuide/sqs-available-cloudwatch-metrics.html). SLO compliance goals are fractions; Azure uses explicitly mapped compliance metrics. Provenance joins runtime digests with registry, build, repository and commit evidence. A tag alone is not build attestation, and a missing observed digest remains unknown.
