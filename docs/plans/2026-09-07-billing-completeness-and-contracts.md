# Billing completeness and contract compatibility

The cost explorer must render every valid server response, explain which charges are present, and support attribution without double counting. This implements the financial reporting portion of the [gap-closure plan](2026-09-07-monitoring-gap-closure.md). Provider exports remain authoritative; the monitor stores replaceable, bounded aggregates.

## Prevent another valid-response rejection

The recent incident crossed three boundaries: provider decimals, PostgreSQL/Rust amounts, and frontend validation/arithmetic. Backend support for 38 fractional places shipped while the browser still enforced 18. A successful authenticated API query therefore did not establish that the website worked.

Keep one documented wire contract: canonical decimal strings, up to twenty integer digits and thirty-eight fractional digits, with no exponent notation, binary floating-point money, or implicit currency conversion. Arithmetic uses that same scale; display rounding is separate. Preserve negative credits, exact boundary values, and null comparisons. Define the rounding rule and make nonzero sub-cent values inspectable without placing long decimals in the default table.

Export or generate the API contract from the backend into a versioned artifact consumed by frontend validation and compatibility tests. If direct generation is impractical, require the same fixture corpus to pass Rust serialization, PostgreSQL round trips, Zod validation, exact TypeScript arithmetic, and browser rendering. Fail CI when an API enum, nullability rule, decimal bound, cursor shape, or source status changes without its consumers. Keep health evidence JSON semantics separate from the raw-token handling used for provider billing decimals.

Fixtures must exercise every amount position: total, previous total, daily/monthly series, contributors, breakdown, and resource costs. Include the observed submicro charge, the smallest supported value, the maximum integer magnitude, signed rounding boundaries, mixed source statuses, and out-of-range rejection. Check overview, Costs, and the resource Cost tab in Firefox, Chromium, and WebKit. An explicitly captured protected API response should be replayable locally; keep captures private and out of public artifacts.

Production verification must inspect the installed binary and embedded asset revision, query billing through IAP, validate the response with the frontend contract, and render the affected user flow. Anonymous denial remains required. An endpoint returning HTTP 200, a nonempty `series`, or a healthy load balancer cannot approve a UI/API contract change alone.

Code entry points are [Rust amounts](../../crates/costs/src/model.rs), [PostgreSQL billing schema](../../crates/history/src/cost_schema.rs), [frontend amounts and validation](../../dashboard/src/cost-schema.ts), [browser contract checks](../../dashboard/tests/browser/billing-contract.spec.ts), and [protected live capture](../../crates/connect/src/live_tests.rs).

## Coverage and scheduling

Expose source identity and measure separately from import state, last attempt, last successful publication, provider freshness, imported charge intervals, missing intervals, and the next eligible attempt. Distinguish daily scan allowance, throttling, authentication, missing export parts, invalid source data, database failure, and unsupported fields. Use messages such as “Older history paused until the daily query allowance resets”; do not collapse expected budget deferral and database failure into one unavailable state.

Compute completeness for the selected date range and measure. A failed old backfill must not imply that an already complete recent selection is missing; a recent successful import must not imply that the preceding comparison period is complete. Separate “no charges in a complete interval” from “no imported evidence.” Show imported coverage and last-success age beside the chart, with detailed source diagnostics below.

Reserve capacity for current-period refreshes before older backfill. Resume durable jobs and page checkpoints without reissuing paid queries unnecessarily. Bound retries by source, retain provider cooldowns, and prevent one slow import from delaying every other source. A permanently failing historical partition must not block current charges indefinitely. Test restart at reservation, query completion, partial staging, and publication, including a changed scan allowance.

Keep finite per-query, daily, row, response, and disk limits. Their exhaustion must be visible and recoverable; browser pagination is independent of import completeness. Validate that the remaining six GCP backfill days resume after the existing daily allowance resets before deciding whether any further increase is necessary.

## GCP: incremental corrections without repeated wide scans

Measure bytes scanned and billed for each import period, cache hit, and retry. The current backfill query considers later export partitions so late charges are not missed, but repeatedly scanning them for old usage weeks can consume the daily allowance. Preserve correctness while reducing repeated work.

Track an ingestion watermark and discover newly affected usage dates from bounded export-time/partition reads, then rebuild those date partitions atomically. Define a bounded overlap and periodic retained-history reconciliation for corrections outside the recent window. Preserve unknown streaming partitions where applicable. Verify the actual export schema and available partition pruning before choosing the query; do not assume ingestion date equals charge date. Google's export documents both processing time and corrections: [detailed export schema](https://docs.cloud.google.com/billing/docs/how-to/export-data-bigquery-tables/detailed-usage).

Acceptance: late usage, credit adjustments, invoice-month changes, empty days, retries, and corrections to old dates are reflected exactly once. Query cost for an unchanged export converges instead of scaling with the entire retained history on every refresh. Standard and detailed exports for the same account must never be imported as overlapping authorities.

## AWS: resource attribution and richer cost measures

Keep the existing management-account Cost Explorer query as a bounded aggregate source. Its current service/account totals do not supply resource IDs, full charge categories, or invoice reconciliation. Do not infer those fields from inventory or distribute account totals across resources.

Configure a versioned CUR 2.0 export with resource detail in the private devops repository, using the management account once for organization coverage. AWS documents `INCLUDE_RESOURCES` as the setting that adds resource-level granularity and `line_item_resource_id`: [CUR 2.0 schema](https://docs.aws.amazon.com/cur/latest/userguide/table-dictionary-cur2.html). Select only required billing columns; omit account names, arbitrary tags, and unrelated metadata unless an attribution requirement explicitly needs them.

Implement a native SDK reader with access limited to the configured export bucket/prefix and encryption key when required. This is a billing-specific object-read capability, not permission for general collectors to read application objects. Validate manifest identity, schema version, complete part set, object versions, and bounded checksums. Stream rows or spool bounded parts to owner-only files; do not accumulate an entire export in memory.

Map payer/member scope, resource, region, service, charge category, currency, usage day, and invoice period. Define billed versus amortized/effective measures explicitly for commitments and credits. Run the new source in comparison mode before cutover; once selected as authority for a period, exclude the overlapping Cost Explorer source from totals. Never add both representations together.

Acceptance: multi-account totals reconcile with the provider for matching scope, dates, currency, and measure; resource views use exact native IDs; missing IDs remain unallocated. Cover revised manifests, partial delivery, duplicate parts, renamed objects, malformed rows, and interrupted publication. A missing part cannot replace a complete published period.

## Azure: export revisions and accounting scope

Retain the working subscription queries while validating a versioned export for each appropriate billing scope. Prefer an export that supplies the required billed/effective fields and resource identity, without assuming every subscription has the same agreement, permissions, or export capabilities. Azure supports versioned actual, amortized, and FOCUS export datasets, with partitioned files and overwritten daily revisions: [export configuration](https://learn.microsoft.com/en-us/azure/cost-management-billing/costs/tutorial-improved-exports).

Use the dedicated workload identity and narrowly scoped Storage permissions. Add only the required native token audience and storage adapter. Validate the complete export revision and all parts before atomic publication; retain earlier totals on partial or changed delivery. Apply the same bounds and redaction rules as the AWS reader.

Keep invoice scope and charge scope distinct. Subscription totals may omit billing-level charges or adjustments. Preserve resource IDs as case-normalized native identifiers, and keep unavailable invoice fields null. Compare like measures: Azure's FOCUS validation guidance distinguishes billed-cost and effective-cost comparisons, so actual and amortized datasets must not be summed as separate spending sources. See [FOCUS reconciliation](https://learn.microsoft.com/en-us/cloud-computing/finops/focus/validate).

Acceptance: exact fractional charges survive provider parsing, PostgreSQL, API validation, and browser rendering; revised periods replace previous partitions without duplication; unsupported billing scopes remain explicit. Verify provider cooldowns and credential refresh over a full token lifetime without local CLI login caches.

## Cross-provider reconciliation and overview

Create a reconciliation view by provider, billing scope, period, currency, and measure, with provider-reported total, imported total, difference, and known exclusions. Define rounding tolerance from the source contract; do not use an arbitrary percentage or binary floating-point comparison. Keep tax, credits, refunds, purchases, commitments, usage date, and invoice month explicit where supplied. Unexplained differences prevent an invoice-complete label.

The overview chart can aggregate comparable billed values within one currency. Keep other currencies separate and label sources with different or incomplete measures. Contributor selection, Other, comparison periods, search, and pagination must retain the same filters and publication revision. Show sub-cent amounts and missing attribution meaningfully; reserve raw precision and import mechanics for details.

External vendors remain unconnected until an explicit list of services, authoritative billing APIs/exports, read identities, and attribution requirements is provided. Publish that missing-source list instead of labeling the chart “all company costs.” Reuse the same source, revision, and reconciliation model when those adapters are added.

## Rollout and completion

Ship contract gates and source-status improvements first, then GCP scheduling, followed by AWS/Azure export readers and accounting reconciliation. Keep migrations additive, use native UUIDv7 keys and exact numeric columns, and test supported prior schemas. Do not narrow decimal scale on rollback; disable billing if an older binary cannot read published values.

Preserve IAP access for every existing dashboard reader. Export/IAM administration stays in devops and does not become an action available to dashboard users. Roll out one source at a time with comparison evidence, bounded backfill, restart recovery, and a tested authority cutover. Completion requires correct selected-period coverage, reconciled matching totals, an actual rendered browser check, and a documented list of remaining unavailable sources or measures.
