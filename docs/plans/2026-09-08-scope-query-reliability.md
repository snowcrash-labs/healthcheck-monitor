# Responsive scope discovery

Scope discovery lists monitored accounts and environments. It must remain usable when historical evidence is slow or unavailable, while stating which retained scopes may be missing.

## Observed failure

The authenticated overview and readiness endpoint remained responsive while scope discovery returned HTTP 503. PostgreSQL repeatedly cancelled its distinct target/provider/scope query at the five-second statement deadline. The selected index-scan/incremental-sort plan estimated 550 scope combinations, but the production-shaped dataset returned 11. A read-only diagnostic transaction that disabled incremental sort selected bitmap scans and hash aggregation and completed in 996 ms against roughly 180,000 rows and a 325 MB heap.

A temporary database-role-specific planner override restored the existing service after old pooled connections retired: scope discovery completed in 1.4 seconds and an API summary in 2.0 seconds. Statement deadlines were not increased and collection was not restarted. The permanent implementation restricts the planner setting to the scope query's read-only transaction; it must not leak into later queries on a pooled connection.

## In-process cache and query budget

Scope responses use a bounded RAM cache: at most 64 entries and 32 MiB of charged response bytes plus filter keys. Successful historical responses expire after 15 seconds. Explicitly incomplete current-only responses expire after one second, allowing a burst of callers to share a failure without prolonging recovery. Expiry is measured from insertion, not refreshed by hits. Least-recently-used entries are evicted first. There is no external cache service or durable cached response.

Normalized filters, page size, cursor and current-view generation identify a response. Collector running state, history readiness, journal availability, pending-work presence, persisted watermark and loss counters also participate in the key. Coverage changes invalidate cached completeness claims immediately. Relative-window cache hits retain their original response window and cursor; they are not relabeled as newly collected evidence. Absolute windows and distinct cursors remain separate cache entries. The Age and X-Healthcheck-Cache response headers disclose reuse; HTTP responses remain no-store and authentication runs before every cache lookup.

Identical misses share an in-process request lock and recheck the cache after acquisition. The lock registry holds weak references and is bounded to 64 live keys; cancelled requests cannot retain an unbounded set of locks. Lock waits are bounded to three seconds. Historical availability and scope lookup share a two-second budget. A lookup error or timeout returns matching current scopes with history_available=false, complete=false and an explicit retained-scope gap. A successful watermark lookup does not override a later scope-query failure.

The five-second database statement limit and existing HTTP admission/response bounds remain unchanged. Query-local planner settings roll back on success, error and connection recovery. No Redis, Valkey, extra service or provider scan is introduced.

## Verification and rollout

Tests cover expiry without sliding renewal, short degraded-response lifetime, filter/cursor/generation isolation, entry and byte eviction, bounded/coalesced request locks, cached authentication, collector-state invalidation, failure after successful availability, a stalled history future and native PostgreSQL pagination. A PostgreSQL test verifies that the planner setting remains unchanged on the reused connection after both success and an invalid-bound-value failure. Database tests refuse non-test databases.

Before deployment, run formatting, Clippy, the Rust suite and isolated PostgreSQL contracts on the supported platforms. Verify cold and repeated authenticated scope requests against the retained dataset, original pagination windows, visible history gaps and independent overview/readiness access. Use the existing main-branch development artifact and guarded deployment path. Preserve the previous executable/configuration and snapshot recovery path.

After the query-local implementation is verified, remove the temporary role override and repeat cold queries on new connections. Rollback restores the prior executable and, if required, the explicitly recorded temporary planner mitigation; it does not raise statement limits or hide missing history. Keep existing history-loss, collection-freshness and resource-removal defects open. CPU profiling also found substantial collection-side JSON serialization; this scoped change does not establish that the collector's separate throughput problems are resolved.

## References

- [SOU-1496: restore monitoring scope queries](https://linear.app/soundpatrol/issue/SOU-1496).
- [SOU-1480: retain monitoring history reliably](https://linear.app/soundpatrol/issue/SOU-1480).
- [Query operations and pagination](2026-09-06-monitoring-query-operations.md).
