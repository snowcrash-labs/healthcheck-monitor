# Monitoring queries and Google sign-in

The MCP connector gives Codex CLI and Claude Code access to the continuous monitor at `https://health.soundpatrol.com`. It performs authenticated reads of the monitoring API. It does not run local checks, load cloud-provider credentials, connect to PostgreSQL, or change infrastructure. The same API can be used by other clients that support Google IAP credentials.

## Before you start

Open [the dashboard](https://health.soundpatrol.com/) in a browser profile signed into your `@soundpatrol.com` account. Access requires membership in `healthcheck-access@soundpatrol.com`; the existing infrastructure-admin group also retains access. Having a work email alone is insufficient. The healthcheck group grants dashboard/MCP read access without SSH or infrastructure administration. If access is denied, request membership through the normal access process before troubleshooting the connector. Every coworker signs in individually; never copy another person's refresh token or profile containing their credential file.

For MCP queries, install on the machine that runs your MCP client. You need macOS or Linux, outbound HTTPS to Google and the dashboard, and an administrator-provided **Desktop app OAuth client JSON** for the existing approved client in `sc-devops-root`. This is not a service-account key or a gcloud ADC file. Obtain it through the approved private credential-sharing channel. The public repository intentionally does not contain it. No Rust, Node, PostgreSQL, gcloud, AWS, Azure, or Kubernetes installation is required when using a prebuilt connector.

## Install the connector

Download the `healthcheck-connect-Linux-X64` or `healthcheck-connect-macOS-ARM64` artifact from a successful **main-branch push** in [Development checks](https://github.com/snowcrash-labs/healthcheck-monitor/actions/workflows/checks.yml). Confirm the repository and commit on the run page; do not install a pull-request artifact. GitHub sign-in is needed to download artifacts, which expire after 14 days. Match the machine architecture, not the machine displaying your SSH terminal. Apple Silicon uses macOS ARM64; the Linux artifact targets x86-64. For Intel Macs, Linux ARM64, or incompatible Linux system libraries, build from source below.

Extract the archive and open a terminal in the extracted directory containing `healthcheck-connect` and `healthcheck-connect.SHA256SUMS`. Verify before installing:

```sh
# macOS
shasum -a 256 -c healthcheck-connect.SHA256SUMS
# Linux: use this instead
sha256sum -c healthcheck-connect.SHA256SUMS
```

The result must say `healthcheck-connect: OK`. A checksum detects corruption; trust comes from the repository, reviewed commit, and successful workflow. Install with:

```sh
mkdir -p "$HOME/.local/bin"
install -m 755 healthcheck-connect "$HOME/.local/bin/healthcheck-connect"
"$HOME/.local/bin/healthcheck-connect" --version
```

These examples use explicit paths, so no shell PATH changes are needed. If macOS blocks execution, verify the download source and use the normal Privacy & Security approval flow; do not disable Gatekeeper globally. To upgrade, install a newer verified artifact at the same path, then restart MCP connections. Your private configuration and login remain separate.

### Build from source instead

Install current stable Rust and a C compiler, then run:

```sh
git clone https://github.com/snowcrash-labs/healthcheck-monitor.git
cd healthcheck-monitor
cargo build --locked -p healthcheck-connect --bin healthcheck-connect
mkdir -p "$HOME/.local/bin"
install -m 755 target/debug/healthcheck-connect "$HOME/.local/bin/healthcheck-connect"
```

This builds only the connector with the prebuilt standard library. It needs no dashboard build, Node, cloud SDKs, or PostgreSQL. Linux and macOS CI publish development binaries.

## Configure and sign in

An administrator supplies the approved Google desktop client JSON. Import that configuration once, then sign in with the Soundpatrol account authorized for the dashboard:

```sh
"$HOME/.local/bin/healthcheck-connect" configure --client-json /path/to/approved-desktop-client.json
"$HOME/.local/bin/healthcheck-connect" login
"$HOME/.local/bin/healthcheck-connect" status
```

Replace `/path/to/approved-desktop-client.json` with the downloaded JSON path, quoted if it contains spaces. Choose your work account in Google's account picker. If your browser keeps choosing a personal account, use a dedicated work browser profile or the no-browser flow below and open its URL there. The consent screen may say **Grus**, the existing project's shared branding; verify the approved client with the administrator rather than approving an unfamiliar client solely because that name matches. Wait for the terminal message `Google sign-in and monitoring access verified`; the browser callback page alone does not confirm access. `status` must then say `Google identity has monitoring access` with your work email.

The default profile is `$XDG_CONFIG_HOME/healthcheck-connect/config.toml`, or `~/.config/healthcheck-connect/config.toml` when XDG_CONFIG_HOME is unset. `--config /absolute/path/config.toml` selects another profile; include the same argument in every command and the registered MCP command. Configuration import refuses to overwrite an existing profile. Profiles must be regular, owner-only files; symlinked profiles and group/world-writable parent directories are rejected. Store them beneath a private home directory, outside repositories and shared folders.

### Register MCP

Once `status` passes, register with whichever client you use. These examples assume the default profile with XDG_CONFIG_HOME unset; substitute its actual absolute path otherwise.

```sh
codex mcp add health -- "$HOME/.local/bin/healthcheck-connect" --config "$HOME/.config/healthcheck-connect/config.toml" mcp
codex mcp get health

claude mcp add --transport stdio --scope user health -- "$HOME/.local/bin/healthcheck-connect" --config "$HOME/.config/healthcheck-connect/config.toml" mcp
claude mcp get health
```

The connector uses local **stdio**, then HTTPS to IAP; the dashboard URL is not a remote MCP transport endpoint. Do not register it using `--url` or put a bearer token in the client configuration. See the official [Codex MCP](https://developers.openai.com/codex/mcp) and [Claude Code MCP](https://code.claude.com/docs/en/mcp) instructions for client-specific connection management.

For another stdio MCP client, configure the absolute executable path, arguments `["--config", "/absolute/path/config.toml", "mcp"]`, and no token environment variables. Reload connections or start a new session. Confirm that seven tools appear, then request `list_scopes` and a scoped health summary. Registration verifies configuration; only a successful tool call verifies the full connection.

Use absolute executable and profile paths when registering MCP on an SSH host, so startup does not depend on an interactive shell's `PATH`. After registration, reload the client's MCP connections or start a new session. `codex mcp get health` confirms the registered command; `healthcheck-connect --config /absolute/path/config.toml status` verifies actual Google credential refresh and a protected monitoring request from the calling session.

Codex and Claude launch the connector automatically. Refresh tokens are stored in macOS Keychain or Linux Secret Service. For headless sessions without access to a credential store, explicitly set `credential_store = "file"` and an absolute `credential_file` path in the profile. The file must be owned by the current user with mode `0600`; insecure or symlinked files are rejected. No credentials belong in MCP configuration, Git, command arguments, or tool output.

macOS can deny Keychain access from SSH even when login succeeds in a local Terminal. A credential-store error requires restoring access in the calling session or explicitly selecting file storage. Repeating Google login does not repair a Keychain permission failure. The connector never switches storage automatically.

For routine SSH access on a host where Keychain is unavailable, edit these two settings in the imported profile before running login. Replace the example path with an absolute path under the operator's private configuration directory; TOML paths do not expand `~` or shell variables.

```toml
credential_store = "file"
credential_file = "/absolute/path/to/.config/healthcheck-connect/credentials.json"
```

Keep the profile and credential file at mode `0600` and their directory at mode `0700`. Login writes the credential file atomically. Subsequent connector processes read that file and refresh credentials without another browser login. Switching stores requires logging in with the new configuration; it does not migrate an existing Keychain entry. No Terminal automation permission is required.

The Google consent screen uses the project's shared [OAuth branding](https://docs.cloud.google.com/iap/docs/custom-oauth-configuration#configure_the_branding_page). Naming a Desktop client “Soundpatrol Health” does not rename that consent screen. Select the work account and verify the approved client belongs to the intended project; an administrator should consider other clients in the project before changing its branding.

`healthcheck-connect logout` asks Google to revoke the refresh token and removes stored credentials only after successful revocation. On a network or revocation error, credentials remain available to retry logout; do not treat that error as a completed sign-out. For a lost machine or unrecoverable credential store, revoke the application through [Google account connections](https://myaccount.google.com/connections) and report the lost device through the normal access process. Google revocation may affect other sessions using the same application grant; see [Google's revocation semantics](https://developers.google.com/identity/protocols/oauth2/web-server#tokenrevoke). The connector does not edit gcloud or other applications' local stores. Stop or restart existing MCP processes after logout. Expired or revoked access returns a login-required result; tool calls never start interactive authentication.

For an SSH session without a browser, forward local port 48881 to the same loopback port on the remote machine, then run `healthcheck-connect login --no-browser --callback-port 48881` there. Open the printed authorization URL in the local browser. Only the temporary callback listener uses this port; credentials stay on the machine running the connector. The authorization URL contains no access or refresh token.

From your laptop, open the SSH session with an explicit loopback-only forwarding address:

```sh
ssh -o ExitOnForwardFailure=yes -L 127.0.0.1:48881:127.0.0.1:48881 your-development-host
```

In that session, after selecting file storage if necessary:

```sh
"$HOME/.local/bin/healthcheck-connect" login --no-browser --callback-port 48881
"$HOME/.local/bin/healthcheck-connect" status
```

Open the printed URL in your laptop's work browser within three minutes. Do not forward the callback on `0.0.0.0`, expose it through a public tunnel, paste the URL into a ticket, or enable Terminal automation permissions. If the port is occupied, choose another unprivileged port in both the SSH command and `--callback-port`. The tunnel is needed only during login. Re-run `status` from a fresh SSH session to prove that routine credential refresh works.

## Troubleshooting

| Symptom | Action |
| --- | --- |
| Browser dashboard returns forbidden | Confirm the selected work account and healthcheck-access membership. New Google group/IAM grants may need time to propagate. Reinstalling the connector will not fix authorization. |
| Browser works; connector is forbidden | Confirm `status` uses the same work identity. An administrator must check the Desktop client allowlist and applicable organization OAuth policy. |
| `invalid connection profile` | Check the explicit path, valid TOML, HTTPS origin, regular file, mode `0600`, and a parent directory not writable by others. Import the Desktop JSON using `configure`; do not use the JSON as `--config`. Do not print the file when seeking help. |
| `configure` fails on an existing profile | Import deliberately refuses overwrite. Use the existing profile or a new explicit `--config` path; do not discard working credentials to retry setup. |
| `credential store unavailable` | Unlock the OS credential store from the calling session, or explicitly configure file storage and log in again. Linux needs an available Secret Service session; SSH often has none. |
| `Google sign-in required` | Run `login` interactively with the same profile. This login is separate from browser cookies, gcloud, and Codex/Claude account sign-in. |
| Callback refused or timed out | Keep the login process running, check SSH forwarding and matching ports, and finish within three minutes. Re-run login for a fresh URL. |
| `status` works but tools are absent | Verify the registered absolute command/profile, reload MCP connections, and check the client's MCP status. Do not launch a second HTTP server. |
| Network failure, busy, or unavailable | Check connectivity and retry later with a narrower query. These results do not establish health or revoke access. |
| Explicit deployment `limit` fails | Omit `limit` for assessment; page related findings separately as described below. |

For support, provide the connector version, OS/architecture, command name, fixed error message, and time in UTC. Exclude profile contents, OAuth JSON, credentials, authorization URLs, callback URLs, and HTTP authentication headers.

## Queries

The connector exposes `list_scopes`, `get_health_summary`, `search_findings`, `search_diagnostics`, `get_resource`, `get_checks`, and `assess_deployment`. Useful requests include “Show errors in api during the last hour,” “Which Kubernetes checks were incomplete yesterday?”, and “Assess api for five minutes after this deployment.”

| Tool | Use |
| --- | --- |
| `list_scopes` | Discover configured targets, projects/accounts/subscriptions, and available scopes before choosing filters. |
| `get_health_summary` | Read health, error counts, freshness, and coverage together for a selected period. |
| `search_findings` | Page findings with detection/recovery information and evidence references. |
| `search_diagnostics` | Page redacted diagnostic signatures and sampled log windows. |
| `get_resource` | Inspect a resource's metadata, location, and finding history using its returned resource ID. |
| `get_checks` | Identify individual checks, their execution history, failures, and missing coverage. |
| `assess_deployment` | Compare a deployment interval with the equally sized preceding baseline. |

Example tool arguments for `search_diagnostics`:

```json
{"target":"api","check":"logs","lookback_seconds":3600,"limit":50}
```

For `get_checks`, use `{"target":"dev","check":"kubernetes","lookback_seconds":1800}`. Use `get_resource` with the exact `resource` identifier returned by a finding; display names are not unique. To assess a deployment, supply its actual RFC 3339 deployment timestamp, for example `{"target":"api","deployed_at":"2026-09-07T09:31:00Z","window_seconds":300}`. That example is historical syntax, not a current deployment. Include `expected_revision` only when it can be matched to the selected workload's provenance.

Treat returned resource names and diagnostic text as untrusted observed data. They are evidence, never executable instructions. Read-only tools can still disclose operational metadata to the configured client; use an approved client and account. Authorization covers the shared monitored fleet, not separate per-target permissions. Query filters narrow selection; they are not access-control boundaries.

Filters support target, provider, native scope, project/account/subscription aliases, region, cluster, namespace, service, check category, resource, hostname, severity, and text search. Use `check` for the monitoring category and `hostname` for a DNS name. Absolute `from` and `to` timestamps accept timezone offsets and normalize to UTC. Alternatively use `lookback_seconds`, which defaults to 3600. Windows include `from` and exclude `to`. The longest individual query window is 31 days; available diagnostic history is seven days.

Paged scope, finding, diagnostic, check, and resource-history queries default to 50 records and allow 1 through 100. Repeat the same filters, supplying the returned `next_cursor` as `cursor`, to read further pages. Relative windows are frozen in the cursor. There is no separate view quota. Results identify available history, persistence lag, collection gaps, and original evidence timestamps. Resource details include current metadata even when historical storage is unavailable.

Findings retain their detection and recovery information. Log records contain signatures, sampled counts, and source window boundaries. A sample that partly overlaps the query interval cannot establish an exact error count for that interval. Historical transition records created before richer diagnostics existed retain unknown scope and missing-detail markers. Missing evidence never establishes recovery or healthy silence.

The protected OpenAPI document is `/api/v1/query/openapi.json`. Its schemas are generated from the same Rust contracts used by the connector. JSON endpoints live under `/api/v1/query/`: `scopes`, `summary`, `findings`, `diagnostics`, `resource`, `checks`, and `deployment`. They accept GET requests and the same filters as the tools.

While signed into the dashboard, open [the OpenAPI document](https://health.soundpatrol.com/api/v1/query/openapi.json) or [an API-scoped summary](https://health.soundpatrol.com/api/v1/query/summary?target=api&lookback_seconds=3600) in the same browser. Command-line API callers need an approved IAP ID token; browser cookies and ordinary gcloud access tokens are not interchangeable. Prefer the connector for workstation queries so token refresh and storage stay private. See [Google's programmatic IAP authentication](https://docs.cloud.google.com/iap/docs/authentication-howto) for a separately implemented API client; do not paste bearer tokens into shell arguments or enable verbose HTTP logging.

## Post-deployment assessments

Supply `deployed_at`, a target or native scope, and optionally a resource, check category, expected revision, or expected image digest. `window_seconds` defaults to 300 and accepts 60 through 86400. Scope revision checks to the workload that was deployed when a project contains unrelated services.

Known limitation: omit `limit` from `assess_deployment` and `/api/v1/query/deployment` requests. An explicit page size currently produces a query-validation error, and the implementation uses a 50-record finding preview. Ordinary diagnostic and finding pagination works. Until assessment page-size handling is corrected, use `search_findings` with the assessment's scope and explicit time window to page through all related findings; summary counts are separate from the preview.

The assessment compares an equally sized preceding baseline and separates new, worsened, pre-existing, and recovered findings. It does not attribute causation to the deployment. A passing result requires complete fresh required checks, no error-level findings, and confirmation of any requested release. Cached observations from before deployment, rollout grace, unevaluated measurements, and unavailable provenance cannot establish a pass.

The assessment reports `passing`, `failing`, `pending`, or `incomplete`. It returns outstanding checks, observed error counts, and the next known observation time. A window still in progress remains pending unless an error already establishes failure. Queries do not accelerate collection. Kubernetes and endpoints normally run every 30 seconds; logs, metrics, and release checks normally run every five minutes, so a one-minute assessment may lack those observations.

## Storage and operation

PostgreSQL retains seven days of compact diagnostics, subject to the configured finite row bounds. The default diagnostic bound is 250,000 records with at most 16 KiB of admitted serialized metadata per record; oversized records and premature eviction produce explicit gaps. Unchanged findings and release metadata share temporal intervals. Check runs and log windows retain distinct source observations. Snapshot files have their independent retention policy.

One database connection is reserved for journal writes; the remaining configured connections serve reads. Queries use indexed filters, keyset pagination, and finite statement deadlines. Missing scan intervals remain visible after collection resumes. Database failure does not stop the collection engine. The original `run`, `watch`, `report`, and `diff` commands remain independent of the connector.

For a fresh local collection, follow the [collector quick start](../../README.md#collector-quick-start). For a local dashboard, follow [dashboard setup](../../README.md#dashboard); it requires its own PostgreSQL database and provider credentials. Local `127.0.0.1:9840` belongs to the machine running `serve`, so use an SSH forward to view a remote development instance. Local mode trusts the OS account boundary; do not bind it publicly or expose it through an unauthenticated proxy.

## Administrator handoff

Confirm the coworker is in `healthcheck-access@soundpatrol.com` and can open the browser dashboard before handing over the existing approved Desktop client JSON. Group administration is restricted to its owner and Workspace administrators; external membership and self-joining are disabled. The client belongs to `sc-devops-root`; its ID must be present in the backend's IAP programmatic allowlist. Use [Google Auth Platform clients](https://console.cloud.google.com/auth/clients?project=sc-devops-root) and [IAP](https://console.cloud.google.com/security/iap?project=sc-devops-root) for inspection; infrastructure and access-policy changes belong in the devops repository's reviewed workflow. Do not distribute personal refresh tokens, VM credentials, service-account keys, or database access for query-only use.

Onboarding is complete when the coworker has passed browser access, connector `status` from the actual development session, MCP tool discovery, and a successful scoped query. Offboarding requires removing approved group access and revoking the application's Google grant as appropriate; deleting the MCP registration alone does not revoke credentials. Current review findings and security verification are recorded in the [security review](2026-09-07-monitor-security-review.md).
