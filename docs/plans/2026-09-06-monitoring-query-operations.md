# Monitoring queries and Google sign-in

The MCP connector gives Codex CLI and Claude Code access to the continuous monitor at `https://health.soundpatrol.com`. It performs authenticated reads of the monitoring API. It does not run local checks, load cloud-provider credentials, connect to PostgreSQL, or change infrastructure. The same API can be used by other clients that support Google IAP credentials.

## Installation

Download the `healthcheck-connect-Linux-X64` or `healthcheck-connect-macOS-ARM64` artifact from a successful [Development checks run](https://github.com/snowcrash-labs/healthcheck-monitor/actions/workflows/checks.yml). Use the artifact matching the machine architecture; the artifact name records the actual runner architecture. The archive includes the executable and its SHA-256 checksum. Install the verified executable on `PATH` and make it executable. Development builds are distributed; no release build is required.

An administrator supplies the approved Google desktop client JSON. Import that configuration once, then sign in with the Soundpatrol account authorized for the dashboard:

```sh
healthcheck-connect configure --client-json /path/to/approved-desktop-client.json
healthcheck-connect login
healthcheck-connect status
codex mcp add health -- healthcheck-connect mcp
claude mcp add --transport stdio --scope user health -- healthcheck-connect mcp
```

The default profile is `$XDG_CONFIG_HOME/healthcheck-connect/config.toml`, or `~/.config/healthcheck-connect/config.toml` when XDG_CONFIG_HOME is unset. `--config /absolute/path/config.toml` selects another profile; include the same argument in the registered MCP command. Configuration import refuses to overwrite an existing profile.

Codex and Claude launch the connector automatically. Refresh tokens are stored in macOS Keychain or Linux Secret Service. For headless Linux without a credential store, explicitly set `credential_store = "file"` and an absolute `credential_file` path in the profile. The file must be owned by the current user with mode `0600`; insecure or symlinked files are rejected. No credentials belong in MCP configuration, Git, command arguments, or tool output.

`healthcheck-connect logout` revokes the connector's refresh token and removes its stored credentials. It does not modify gcloud or other application credential stores. Expired or revoked access returns a login-required result; tool calls never start interactive authentication. If Google sign-in succeeds but IAP returns forbidden, an administrator must check existing dashboard group membership.

For an SSH session without a browser, forward local port 48881 to the same loopback port on the remote machine, then run `healthcheck-connect login --no-browser --callback-port 48881` there. Open the printed authorization URL in the local browser. Only the temporary callback listener uses this port; credentials stay on the machine running the connector. The authorization URL contains no access or refresh token.

## Queries

The connector exposes `list_scopes`, `get_health_summary`, `search_findings`, `search_diagnostics`, `get_resource`, `get_checks`, and `assess_deployment`. Useful requests include “Show errors in api during the last hour,” “Which Kubernetes checks were incomplete yesterday?”, and “Assess api for five minutes after this deployment.”

Filters support target, provider, native scope, project/account/subscription aliases, region, cluster, namespace, service, check category, resource, hostname, severity, and text search. Use `check` for the monitoring category and `hostname` for a DNS name. Absolute `from` and `to` timestamps accept timezone offsets and normalize to UTC. Alternatively use `lookback_seconds`, which defaults to 3600. Windows include `from` and exclude `to`. The longest individual query window is 31 days; available diagnostic history is seven days.

Pages default to 50 records and allow 1 through 100. Repeat the same filters with `next_cursor` to read further pages. Relative windows are frozen in the cursor. There is no separate view quota. Results identify available history, persistence lag, collection gaps, and original evidence timestamps. Resource details include current metadata even when historical storage is unavailable.

Findings retain their detection and recovery information. Log records contain signatures, sampled counts, and source window boundaries. A sample that partly overlaps the query interval cannot establish an exact error count for that interval. Historical transition records created before richer diagnostics existed retain unknown scope and missing-detail markers. Missing evidence never establishes recovery or healthy silence.

The protected OpenAPI document is `/api/v1/query/openapi.json`. Its schemas are generated from the same Rust contracts used by the connector. JSON endpoints live under `/api/v1/query/`: `scopes`, `summary`, `findings`, `diagnostics`, `resource`, `checks`, and `deployment`. They accept GET requests and the same filters as the tools.

## Post-deployment assessments

Supply `deployed_at`, a target or native scope, and optionally a resource, check category, expected revision, or expected image digest. `window_seconds` defaults to 300 and accepts 60 through 86400. Scope revision checks to the workload that was deployed when a project contains unrelated services.

The assessment compares an equally sized preceding baseline and separates new, worsened, pre-existing, and recovered findings. It does not attribute causation to the deployment. A passing result requires complete fresh required checks, no error-level findings, and confirmation of any requested release. Cached observations from before deployment, rollout grace, unevaluated measurements, and unavailable provenance cannot establish a pass.

The assessment reports `passing`, `failing`, `pending`, or `incomplete`. It returns outstanding checks, observed error counts, and the next known observation time. A window still in progress remains pending unless an error already establishes failure. Queries do not accelerate collection. Kubernetes and endpoints normally run every 30 seconds; logs, metrics, and release checks normally run every five minutes, so a one-minute assessment may lack those observations.

## Storage and operation

PostgreSQL retains seven days of compact diagnostics, subject to the configured finite row bounds. The default diagnostic bound is 250,000 records with at most 16 KiB of admitted serialized metadata per record; oversized records and premature eviction produce explicit gaps. Unchanged findings and release metadata share temporal intervals. Check runs and log windows retain distinct source observations. Snapshot files have their independent retention policy.

One database connection is reserved for journal writes; the remaining configured connections serve reads. Queries use indexed filters, keyset pagination, and finite statement deadlines. Missing scan intervals remain visible after collection resumes. Database failure does not stop the collection engine. The original `run`, `watch`, `report`, and `diff` commands remain independent of the connector.
