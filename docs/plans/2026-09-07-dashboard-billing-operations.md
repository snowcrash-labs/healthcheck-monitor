# Dashboard billing operations

The dashboard has separate operational and billing views. Overview summarizes current problems, collection gaps, configured targets, and reported cloud costs. Problems contains Active, Recent errors, and Changes. Resources opens a URL-addressable inspector with Summary, Diagnostics, History, Configuration, and Cost. Checks lists every registration and its collection operations. Monitor status contains persistence and collector details.

Current health is not a historical reconstruction. Recent errors and Changes have explicit time windows; an old unresolved problem remains Active. Pause freezes loaded observations and their display clock while collection continues. Resume refreshes the view. Billing refreshes once per minute in the browser; opening a page never starts a provider query.

## Billing access and sources

Billing is disabled unless server configuration explicitly sets `costs.enabled = true` and `costs.reader_access = "dashboard_readers"`. This reuses the existing authenticated dashboard reader boundary. It does not establish a separate administrator role. Keep billing disabled if a narrower reader group is required. IAP protects billing endpoints, resource cost views, and assets.

| Adapter | Authentication and collection | Accounting limits |
| --- | --- | --- |
| GCP billing export | Native Google credentials and bounded BigQuery REST query jobs against one configured export table. Values and date ranges are parameters; table identifiers are validated. | Billed cost includes nested credits exactly once. The export remains provisional; imported charge dates and invoice month are separate. Standard exports lack resource detail. |
| AWS Cost Explorer, explicitly configured | Official AWS SDK, shared profiles or workload credentials, and a verified account identity. Four pages per daily attempt, with the SDK retry bound. | Aggregate bootstrap using UnblendedCost by service and linked account. Resource identity, charge category, invoice period, and amortized cost are not supplied by this adapter. A complete invoice reconciliation needs an export. |
| Azure Cost Management query, explicitly configured | Native ARM requests with Azure identity. Only continuations within the original subscription and API path are accepted. | ActualCost by service and resource; invoice period and charge category remain unknown. Subscription query coverage can differ from the invoice, including tax and billing-level adjustments. Throttling is an explicit import failure. |

External vendors are not imported. AWS/Azure export-manifest ingestion remains a follow-up to the explicit query adapters. Do not interpret a connected monitoring account as a connected billing source or an invoice-complete cost total. Source errors retain the previous published costs.

Cloud collection uses no application database connections, customer records, queue payloads, or secret values. Billing projections contain dates, resource/scope identifiers, product, category where available, currency, and exact amounts. Provider payloads, account descriptions, tags, credentials, and query error bodies do not enter billing storage.

## Configuration

Add the following to the server TOML, replacing the example identifiers. Production configuration and grants belong in the private devops repository.

```toml
[costs]
enabled = true
reader_access = "dashboard_readers"
interval_seconds = 3600
backfill_days = 90
retention_days = 400
max_rows = 100000
max_bytes_billed = 2147483648
daily_bytes_billed = 21474836480

[[costs.sources]]
id = "gcp"
provider = "gcp"
billing_scope = "000000-000000-000000"
[costs.sources.gcp]
project = "example-operations"
dataset = "billing_export"
table = "gcp_billing_export_resource_v1_000000_000000_000000"
location = "US"
detailed = true

[[costs.scope_targets]]
provider = "gcp"
scope = "example-development"
target = "dev"
valid_from = "2026-01-01"
```

Mappings may also specify an exclusive `valid_to`. Ambiguous mappings remain unallocated. Mapping applies at import time to the charge date, and published partitions retain the resulting target until deliberately rebuilt. The native resource Cost tab requires an exact resource match; it does not estimate resource spending from inventory counts.

GCP needs export dataset read access and query-job permission in the configured project. Grant only the billing export dataset, not project-wide BigQuery data access. Production uses workload credentials. Local ADC is separate from a regular `gcloud auth login`; initialize it with `gcloud auth application-default login`. Collection never starts interactive authentication.

AWS sources use `aws_query = { region = "us-east-1" }` and a twelve-digit billing account scope. Azure sources use `azure_query = { api_version = "2026-06-01" }` and a subscription UUID. An optional `credential` table has the same typed profile settings as collector credentials. Use managed/workload credentials for unattended deployment. Azure CLI token requests select a tenant or a subscription; passing both is invalid in the current CLI.

## Storage and recovery

PostgreSQL owns source identities, import attempts, daily aggregates, and the published day-to-import mapping. Primary keys use native `uuidv7()`; money uses `numeric(58,38)`, with validated decimal strings at the API boundary. Azure returns valid fractional usage charges beyond eighteen decimal places; the wider scale preserves their source values. Unknown dimensions are SQL nulls. Raw exports remain authoritative. Every stored aggregate can be rebuilt by importing the same source period.

Imports stage bounded batches and publish all affected day mappings in one short transaction. A stopped or malformed import cannot replace the previous published partitions. BigQuery jobs use the durable database import identifier, so retrying after a timeout resumes the existing paid job. Scan allowance is reserved before submission and settled to reported bytes after completion. Imports, billing reads, and health-history writes have separate bounded database connections; billing queries have separate admission.

GCP and Azure backfill use at most seven days per import, prioritize the current period, and continue missing intervals in bounded passes. AWS queries its configured history once daily to reduce paid requests. Defaults retain 400 days and revisit closed retained partitions for late adjustments. Response bytes, page counts, aggregate rows, scan bytes, and execution time are finite. An exhausted allowance or limit creates a source fault; it never substitutes zero costs.

Embedded migrations use a native PostgreSQL advisory lock on their dedicated connection. The lock survives cancellation of the startup await until the migration connection closes. The decimal-scale migration preserves existing amounts. After higher-precision charges publish, older binaries cannot decode those values; disable billing when rolling back across this change. Keep the widened database columns and retained charges. Retention removes only monitor-owned derived rows.

## Read API

`GET /api/v1/query/costs/summary`, `series`, and `breakdown` return a consistent selection containing totals, daily/monthly series, a paged contributor table, source status, and publication revision. `GET /api/v1/query/costs/sources` reports import status. The existing monitoring and MCP APIs retain their behavior.

Cost dates are UTC and use an inclusive `from` and exclusive `to`. The default is thirty completed days; requests can cover up to 400 days. Filters include provider, scope, target, resource, product, region, category, currency, grouping, selected day, and contributor. Currency totals are separate; no implicit conversion occurs.

Amounts are list-price spend; every total, series point, and contributor also carries a signed `credits` figure, so spend after credits is `amount + credits`. The GCP adapter stores spend as `billed` and spend after promotions, discounts, and committed-use credits as `effective`; AWS and Azure report no separate credits yet, so their credits are zero. Rows imported before this split hold net amounts with no effective value and therefore show zero credits until the scheduler re-imports their days. The chart keeps bars at spend height and fades the credited band with a dotted overlay, so a promotion that covers an account does not read as an import stall. On 2026-09-06 a promotional credit began covering all Google Cloud spend; net totals for that account are zero from 2026-09-07, which prompted this change.

Breakdown cursors bind filters, currency, period, publication revision, and the ordered amount/key boundary. A changed publication returns HTTP 409 with `refresh_required`. The client keeps the previous view until refresh. Contributor keys are opaque; `v:` represents an unallocated dimension and `__other__` is the chart remainder. Totals and chart reduction include all matched contributors, independently of the visible page.

Selecting Other excludes the parent period's seven largest contributors and opens the remaining searchable, paged breakdown. Search then narrows those remaining contributors. Selecting a day retains the parent date range for that exclusion and cursor binding. Contributor comparisons are batched for the visible page and remain unavailable until both comparison periods have complete imported day coverage. Missing resource or environment attribution is labeled Unallocated; configured environment mappings are distinguished from provider-reported dimensions.

## Verification and rollout

Local PostgreSQL tests cover exact credits, staged replacements, interrupted imports, native UUIDv7 keys, and cursor invalidation. Browser fixtures cover navigation, inspector state, bounded resource paging, chart selection, themes, and revoked authorization. A live native GCP probe imported 3,528 daily aggregates across seventeen projects for a two-day period; it used a temporary credential from the existing CLI sign-in. This validates query, projection, and publication, not local ADC renewal or production billing IAM.

Production monitoring now includes four AWS accounts and two Azure subscriptions alongside GCP. All six cloud identities passed preflight from the VM using Google workload federation. Native inventory and metric probes returned evidence from both clouds; the continuous service schedules inventory, metrics, managed dependencies, queues, logs, alerts, edge discovery, and releases. Empty services and unavailable telemetry remain explicit outcomes. Registering an account does not establish full service coverage.

Google metadata identity tokens are exchanged through native AWS STS and Azure Identity clients. AWS trusts the VM's subject, authorized party, and account-specific audience. Azure trusts the same subject and its token-exchange audience through a user-assigned identity. Refresh uses the VM identity; personal credential stores and cloud CLIs are absent from this production authentication path. Private devops configuration owns identifiers, trust, and grants.

AWS organizational billing has imported through the management account. Azure billing exposed the provider's client-type rate limit; requests identify the monitor and honor the longest reported cooldown within a bounded deadline. The remaining older GCP backfill exceeded its original 2 GiB scan limit. Production configuration raises that limit to 4 GiB with a separate 20 GiB daily allowance. Resumed jobs reserve any increase before execution, and pending reservations survive midnight.

Revision `a4bff7d` was deployed on 2026-09-08 UTC with billing approved for every existing IAP dashboard reader. All five configured billing sources have published through the VM identity. An authenticated query returned thirty daily points over HTTP/2, and anonymous access was denied. AWS organization billing, the previous GCP account, and Azure research have ninety published days. The current GCP account has eighty-four; its oldest six days await the daily query allowance. Azure primary is backfilling after the precision correction. Source status remains provisional, and incomplete periods are not invoice-complete totals.

Linux and macOS CI passed with 329 Rust tests, Clippy, PostgreSQL integration tests, and 36 browser cases. The live Azure response passed typed projection, and PostgreSQL preserved a charge of `1e-38` through publication and retrieval. The VM remained active without automatic restarts; the post-deployment sample used approximately 459 MiB, with a 564 MiB peak. Cross-cloud startup and retained evidence still require sustained observation; these samples do not prove steady-state capacity. The deployment was explicitly authorized outside the pending CI/CD PR; routine delivery retains its main-branch and review boundaries.

Startup verification includes an existing production snapshot. Global JSON arbitrary-precision mode is incompatible with tagged floating-point health observations; exact Azure billing numbers use raw JSON tokens only in that adapter. Readiness requires successful state restoration and a runtime heartbeat, so a listening HTTP socket alone cannot approve a deployment. The incompatible initial release was rolled back before the corrected release was activated.

Remaining acceptance includes authenticated resource-link checks across configured families, sustained VM load measurements, reconciled imported billing periods, and resolution of provider coverage gaps. AWS Health entitlement, Azure Resource Health authentication despite provider registration, missing metrics, and history persistence gaps remain visible. Inventory and completed authentication alone cannot establish normal client behavior.

The billing frontend subsequently rejected valid 38-place amounts because its validator and integer arithmetic still used eighteen places. Revision `304bda3` corrected both and was deployed on 2026-09-08. Verification compared the protected served asset checksum with the tested bundle and rendered a fresh billing response in Firefox, Chromium, and WebKit. Existing open browser sessions need a reload to acquire the new JavaScript. The [gap-closure roadmap](2026-09-07-monitoring-gap-closure.md) separates this repaired contract regression from remaining import, evidence, and coverage work.

## References

- [Dashboard experience specification](2026-09-07-dashboard-experience.md).
- [Implementation and acceptance plan](2026-09-07-dashboard-structure-clouds-and-costs.md).
- [Google billing export query examples](https://docs.cloud.google.com/billing/docs/how-to/bq-examples).
- [BigQuery scan controls](https://docs.cloud.google.com/bigquery/docs/best-practices-costs).
- [AWS Cost Explorer query API](https://docs.aws.amazon.com/aws-cost-management/latest/APIReference/API_GetCostAndUsage.html).
- [Azure Cost Management query API](https://learn.microsoft.com/en-us/rest/api/cost-management/query/usage?view=rest-cost-management-2026-06-01).
- [Microsoft cost query pagination and rate-limit guidance](https://github.com/microsoft/azure-skills/blob/main/skills/azure-cost/cost-query/workflow.md).
- [AWS Google workload identity](https://aws.amazon.com/blogs/security/access-aws-using-a-google-cloud-platform-native-workload-identity/).
- [Azure Google workload federation](https://learn.microsoft.com/en-us/entra/workload-id/workload-identity-federation-google-cloud).
