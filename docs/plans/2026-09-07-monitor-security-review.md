# Monitoring access security review

Review date: 2026-09-07. Scope: workstation connector authentication and credential lifecycle, MCP input/output boundaries, HTTP/IAP authorization, query selection and persistence projections, dashboard rendering, dependency advisories, and inspection of the deployed access configuration. This is a defensive source review with regression tests and limited live checks, not a penetration-test certification. A subsequent authorized access change adds a dedicated healthcheck reader group; provider credentials and production collection settings are unchanged.

## Corrections

| Finding | Correction | Verification |
| --- | --- | --- |
| OAuth JSON import checked file size before an unbounded read, permitting growth between the check and read and blocking on non-regular input. | Open with nonblocking/no-follow flags, require a regular file, and read at most 16,385 bytes before enforcing the 16 KiB limit. | Oversized files, directories, and symlinks fail; valid imports work and refuse overwrite. |
| Connection profiles accepted symlinks and group/world-readable files; import did not reject a parent directory writable by another user. | Require a regular owner-only profile, reject a symlinked final component, and validate the immediate parent on import/load. | Private profiles pass; readable profiles, symlinks, and replaceable immediate parents fail. |
| File-store access errors were all reported as missing Google login. | Only `NotFound` maps to login-required; other open errors map to unavailable credentials. | A rejected credential symlink reports a storage error without requesting another login. |
| Logout deleted the only local revocation handle when Google's revocation failed. | Keep credentials until Google confirms success and return an explicit retryable-by-the-operator revocation error. | Failed revocation retains synthetic credentials; confirmed success deletes them. No real account was logged out during verification. |

Existing profiles created by `configure` already use mode `0600`. Handwritten profiles may need permission correction before the updated connector will accept them. Put profiles and credentials in a private directory under the user's home; directory ancestors must also be trusted. The checks do not defend against compromise of the same OS user, root, or concurrent malicious replacement of ancestor directories. File storage remains an explicit alternative to the OS credential store, not encryption at rest by the connector.

## Reviewed boundaries

| Boundary | Evidence and limits |
| --- | --- |
| Google desktop login | PKCE S256, random state/nonce, loopback-only callback, three-minute lifetime, bounded callback input, duplicate state/code rejection, signed Google token verification, expected client audience, issuer, expiry, nonce, and verified corporate email. Corporate email validation is additional to IAP group authorization. |
| Credentials and transport | Refresh tokens are bound to server/client identity; refresh is serialized. Authenticated requests use HTTPS and reject redirects. Google exchanges have fixed destinations, response bounds and deadlines. MCP tools cannot select arbitrary endpoints, read credential files, or invoke login. Errors are fixed strings; OAuth bodies and tokens are not included. |
| IAP | ES256 signature, issuer, backend audience, expiry/issue time, subject, and domain checks; stale public keys fail closed and unknown key IDs cannot trigger unbounded refresh. Trusted peer and public host checks precede authorization. Unsigned identity headers do not authorize requests. |
| Routes | Authentication covers documents, embedded assets, SSE, all seven query endpoints and OpenAPI. Regression coverage now explicitly includes every query route. API writes, DNS rebinding hosts, and cross-site browser API requests are rejected. `/healthz` exposes a probe only to loopback/trusted load-balancer peers. |
| Queries | Typed filters, bounded dates/page sizes/cursors, parameterized Diesel queries, escaped search wildcards, response bounds, admission semaphores and deadlines. Cursors are selection state, not an authorization mechanism. Every authorized reader has access to the configured fleet; target filters do not implement tenant isolation. |
| Evidence and browser | Log payloads are reduced to a fixed diagnostic vocabulary and sampled counts. Query projections reuse allowlisted facts. Dashboard components render text rather than raw HTML; generated console destinations use fixed provider origins and encoded identifiers. External links use `noopener noreferrer`. JSON evidence uses `Cache-Control: no-store`; CSP restricts scripts and framing. Browser storage contains theme preference, not credentials. |
| Build supply chain | Workflow actions are pinned by commit, token permissions are read-only, checkout credentials are not persisted, and artifacts include checksums. Installation docs require a successful main-branch push artifact. Checksums alone are not signatures or attestations; verify the originating workflow and commit. |

The stdio caller can read operational metadata using the signed-in account. Treat observed names and diagnostics as untrusted data, and use approved MCP clients. The connector is a shared-fleet reader, not a sandbox for untrusted local software. Local dashboard mode likewise trusts the local OS boundary and must remain on loopback.

## Verification

Local macOS development checks passed: 13 connector tests, 32 server tests, and 19 shared query tests, including signed JWT rejection, all query-route authentication, input bounds, private credentials, and revocation regressions. Formatting and targeted Clippy passed. A connector-only locked development build succeeded without building dashboard assets. A fresh process using the existing private profile successfully refreshed Google credentials and queried the protected scopes API over SSH. Linux/macOS workspace and PostgreSQL checks run in the repository's Development checks workflow after push.

`cargo audit` examined 668 locked packages and reported zero known vulnerabilities or informational warnings against the advisory database updated on 2026-09-07. `npm --prefix dashboard audit` reported zero known vulnerabilities. The dependency tree has no `native-tls` package; HTTP transport uses rustls. Platform credential-store bindings are distinct from HTTP TLS. No dependency versions or lockfiles were changed in this pass.

Live inspection confirmed that the `healthcheck-monitor` backend in `sc-devops-root` has IAP enabled and uses HTTP/2 to its backend. Its reader policy grants the dedicated `healthcheck-access@soundpatrol.com` group alongside the existing infrastructure-admin group, without domain-wide access. The new group grants only backend access; it does not inherit infrastructure-admin or SSH permissions. External members, self-joining, and group mail are disabled, with owner-only membership administration. The VM has no external IP. Monitor-specific ingress rules allow TCP 9840 from Google's load-balancer ranges and TCP 22 from the IAP TCP-forwarding range. An anonymous request to `/api/v1/query/scopes` returned HTTP 302 to sign-in. This inspection did not enumerate all organization-level policies or perform denial-of-service testing.

## Remaining operational limits

The existing history-loss, stale-finding, missed-OOM, log-window, missing-telemetry, and explicit deployment-page-size issues remain separate tracked work. They were not fixed by this access review. Missing evidence must remain visible, and deployment assessments should not be used as an automatic release gate until the known reliability issues are resolved. No negative test with a real unprivileged coworker account was performed; source tests cover rejected identities and headers, while live access-policy inspection and anonymous rejection provide narrower evidence.

Use the [coworker setup runbook](2026-09-06-monitoring-query-operations.md) for onboarding, SSH operation, logout/revocation and support-safe troubleshooting. Google revocation can affect other sessions in the same application grant; restart connector processes after logout, and use the Google account connections page if a machine is lost or token revocation cannot be completed locally.
