# Monitoring query access

The running service exposes a typed, read-only query API behind Google IAP. A small Rust MCP connector handles Google desktop login and credential refresh for Codex CLI and Claude Code; it never collects infrastructure evidence locally. The desktop OAuth client is allowlisted for programmatic IAP access while browser authentication and devops-group authorization remain unchanged.

Queries cover current and recovered findings, grouped redacted logs, resource metadata, check coverage, and post-deployment assessments. Filters include native cloud scope, target, monitoring category, hostname, resource location, severity, and explicit or relative time windows. Seven days of compact historical evidence are retained with explicit persistence and collection gaps. Pagination does not impose a separate result quota.

Post-deployment assessments accept deployment time, scope, a window of at least one minute (five minutes by default), and optional revision or digest expectations. They compare an equally sized baseline and require fresh post-deployment evidence. Existing sampling and rollout grace remain authoritative. Queries do not trigger scans or change schedules.

Implementation includes shared contracts, indexed PostgreSQL history, additive API routes and OpenAPI, the MCP connector and credential store, devops-owned OAuth settings, client installation instructions, Linux/macOS development checks, and live IAP verification. Historical payloads remain allowlisted metadata; customer data, secret values, raw logs, and provider objects are excluded.
