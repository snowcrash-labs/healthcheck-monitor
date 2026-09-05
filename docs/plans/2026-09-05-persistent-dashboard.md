# Persistent monitoring service and dashboard

Implemented for native local demonstration. The shared monitoring engine runs autonomously under an Axum service and exposes a read-only SolidJS 2.0 RC dashboard. Collection continues without browsers. Health, coverage, observation age, history gaps, and the monitor's own condition remain separate. Existing command-line run, watch, report and diff behavior remains supported. Setup and operating limits are in the [dashboard runbook](2026-09-05-dashboard-operations.md).

## Runtime and API

Extract foreground supervision into `monitor-runtime`, with bounded observer updates for API views and journal batches. Keep collection off the HTTP request path. Bound accepted connections, request concurrency, event streams, response sizes and pagination. Use server-sent events for revision announcements; a slow browser cannot hold the scheduler or grow a queue. Preserve SIGHUP validation and bounded shutdown. Prefer HTTP/2 through TLS ALPN; retain HTTP/1.1 for clients and local development that require it.

Use a loopback listener for local development. Shared access uses the existing SSO proxy boundary with verified proxy credentials and trusted peers; provider credentials remain server-side. File-managed listener, TLS, access and database settings require restart; monitoring settings retain atomic reload. No remediation, synthetic transactions, browser-triggered scans, notifications, deployment or external messages are part of this implementation.

## PostgreSQL history

Use PostgreSQL 18.6, the latest released version verified from the PostgreSQL version policy. All application primary keys use native `DEFAULT uuidv7()` and version-seven constraints. Diesel's `postgres_backend`, diesel-async and bb8 provide typed queries without libpq. Embed migrations and apply them through the async connection wrapper. Native PostgreSQL enums, prefixed columns, named constraints, foreign-key indexes and bounded field types apply on both sides of the database boundary.

Persist configuration revisions, completed check summaries, finding transitions with diagnostic metadata, and explicit delivery gaps. These records describe the monitor's own observations; do not replicate unrestricted provider objects. Generate idempotency hashes for retry deduplication while PostgreSQL generates primary keys. Use bounded journal admission, bulk inserts, connection deadlines, limited retries, row/age retention and observable database faults. A database outage must not stop current collection or produce a healthy history claim.

## Frontend

Uses `solid-js` and `@solidjs/web` 2.0.0-rc.6, compatible Solid router 2.0.0-next.21, official compiler plugin 3.0.0-next.39, Vite 8.2.2, strict TypeScript, Solid context, and centralized CSS. Overview, findings, resources, evidence details and queryable history have explicit waiting, stale, missing-coverage, disconnected and history-unavailable states. Large lists use pagination; obsolete requests and disposed subscriptions are cancelled. Desktop and mobile top bars remain visible; light, dark and system themes persist locally.

## Verification

Native macOS verification passed 258 regular Rust tests plus the explicitly enabled PostgreSQL contract test. The latter used the isolated PostgreSQL 18.6 test database and verified native UUIDv7 defaults, typed fields, keyset pagination, idempotent retries, row retention, gap recording and journal flushing. Server tests cover trusted proxy access, rejected writes and cross-site requests, response/body/stream bounds, coalesced SSE reconnects, newest-source selection, unknown health with complete collection, deep links, HTTP/2 and stalled TLS handshakes. A queue test accounts for 999 dropped admissions while holding one bounded batch; a shutdown regression keeps dequeued work outstanding until its write completes.

Five frontend tests, strict TypeScript checking and the Vite production build passed. Two Chromium browser scenarios passed through the actual TLS service, covering resource routing and reload, target preservation, light/dark persistence, desktop/mobile sticky headers, mobile overflow and offline/reconnect behavior. Light, dark and mobile screenshots were inspected. The initial frontend entry is 108.88 kB, 32.84 kB gzipped; centralized CSS is 13.67 kB, 3.77 kB gzipped. Other routes load separate chunks.

The focused native demo repeatedly collected approximately 2,800 distinct displayed resources without a browser driving checks. HTTPS negotiated HTTP/2 and returned 200 for routed documents. A database inspection found 101 persisted transitions, all with version-seven keys, 41 check summaries and zero recorded gaps at the sampled point. No journal admission loss or view truncation occurred. A process RSS sample after five minutes was 112.6 MiB; this is a development-build point measurement, not a production load guarantee. A five-minute duration run ended normally and retained partial evidence.

A separate database-outage exercise continued one-second preflight checks and served its API with history marked unavailable. SIGHUP rejected a version-two configuration without changing its revision, then accepted a valid replacement and exposed the new revision. The 45-second duration expired normally, disk evidence published without a persistence fault, and database cleanup stopped at its five-second deadline with an explicit pending-history-loss log. A healthy-database SIGINT run returned 130 and published final evidence without a persistence fault; signaling and observing exit took 87 ms. Local snapshots remain independent of the database journal.

A one-off development queue assessment returned 0 with 20 queue observations and 2,541 shared prerequisite observations. Offline report and diff succeeded. A full development assessment attempted all 17 selected checks and references, retained 6,418 observations and 11 findings, and returned 3 for existing credential and coverage gaps. Kubernetes, queues and GitHub completed independently of failed cloud reads; there were no disk faults or dropped transitions. Deep-profile validation resolved 17 checks. Evidence is retained locally under `evidence/dashboard-live`, `evidence/dashboard-one-off`, `evidence/dashboard-full-assessment` and `evidence/dashboard-reload-outage`.

Formatting and Clippy passed. The final `cargo upgrade --incompatible` sweep reported 89 direct packages current; RustSec audited 583 locked packages without advisories or warnings. npm reported zero vulnerabilities. Collection and database transports use rustls, with no native-tls, OpenSSL or libpq dependency. The unmaintained PEM helper was removed in favor of the maintained parser in rustls's PKI types. The user-built release binary and all 37 original source-bundle files retained their checksums; no Rust release build ran.

The GitHub workflow now includes frontend checks on Linux/macOS and an isolated PostgreSQL 18.6 contract job. The repository has no remote, so that workflow has not run. Current dashboard changes have only been executed on native macOS; live GCP/AWS/Azure credentials, production load and the external SSO proxy remain validation gaps. The [monitor verification record](2026-09-05-monitor-verification.md) describes the original provider and Linux validation separately.

## References

- [Dashboard assessment](2026-09-05-health-dashboard-assessment.md).
- [Dashboard operations and local demo](2026-09-05-dashboard-operations.md).
- [PostgreSQL versions](https://www.postgresql.org/support/versioning/) and [native UUID functions](https://www.postgresql.org/docs/current/functions-uuid.html).
- [Solid 2.0 migration guide](https://github.com/solidjs/solid/blob/next/documentation/solid-2.0/MIGRATION.md).
