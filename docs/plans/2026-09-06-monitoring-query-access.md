# Monitoring query access

The running service exposes a typed, read-only query API behind Google IAP. A small Rust MCP connector handles Google desktop login and credential refresh for Codex CLI and Claude Code; it never collects infrastructure evidence locally. The desktop OAuth client is allowlisted for programmatic IAP access. The dedicated `healthcheck-access@soundpatrol.com` reader group authorizes coworkers without infrastructure-admin privileges; the existing infrastructure-admin group retains access.

Queries cover current and recovered findings, grouped redacted logs, resource metadata, check coverage, and post-deployment assessments. Filters include native cloud scope, target, monitoring category, hostname, resource location, severity, and explicit or relative time windows. Seven days of compact historical evidence are retained with explicit persistence and collection gaps. Pagination does not impose a separate result quota.

Post-deployment assessments accept deployment time, scope, a window of at least one minute (five minutes by default), and optional revision or digest expectations. They compare an equally sized baseline and require fresh post-deployment evidence. Existing sampling and rollout grace remain authoritative. Queries do not trigger scans or change schedules.

Implementation includes shared contracts, indexed PostgreSQL history, additive API routes and OpenAPI, the MCP connector and credential store, devops-owned OAuth settings, client installation instructions, Linux/macOS development checks, and live IAP verification. Historical payloads remain allowlisted metadata; customer data, secret values, raw logs, and provider objects are excluded.

## Verification

Live validation on 2026-09-07 confirmed individual Google sign-in, credential refresh from a fresh process over SSH, and all seven MCP tools against the deployed monitor. Queries covered dev/api summaries, check history, resource details, redacted diagnostics, cursor pagination with a frozen time window, and a scoped one-minute deployment assessment using its default page size. The assessment reported observed errors and missing evidence. Anonymous API requests still redirected to Google. The subsequent authorized coworker access change adds healthcheck-access alongside infrastructure-admin; see the [security review](2026-09-07-monitor-security-review.md) for scope and verification limits.

The SSH installation uses explicitly selected credential-file storage: mode `0600` files in a mode `0700` directory outside Git. macOS Keychain accepted login from a local Terminal but denied reads from SSH. The connector now distinguishes unavailable storage from missing Google authorization. [Linux/macOS development checks and PostgreSQL contract tests passed](https://github.com/snowcrash-labs/healthcheck-monitor/actions/runs/34106090813), including the credential-error regression. Setup and troubleshooting are in the [query access runbook](2026-09-06-monitoring-query-operations.md).

## Remaining work

The [dashboard, cloud coverage, and cost plan](2026-09-07-dashboard-structure-clouds-and-costs.md) reuses these contracts. It distinguishes current state from selected-period evidence and adds separate cost date/revision semantics; it must preserve existing query and connector compatibility. A frozen query time window is not a durable snapshot of records that can arrive late or be corrected.

Explicit deployment-assessment page sizes are rejected by query parsing; the current finding preview is fixed at 50 records. Correct numeric parsing for the flattened request and apply the requested preview size, with an encoded HTTP request regression test and live verification. The default assessment and ordinary finding/diagnostic pagination were verified.

Live results expose collection gaps and dropped history records. Successful authentication and query execution do not establish complete monitoring coverage. Investigate those operational gaps separately; queries must continue to disclose them until supported by complete evidence. Scheduled delivery from the devops main branch still awaits the deployment pull request's independent review.
