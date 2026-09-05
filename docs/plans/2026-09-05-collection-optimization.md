# Collection optimization

The release-versus-Python benchmark showed lower CPU and memory use in Rust, but sequential Kubernetes inventory and KEDA requests increased elapsed time. This work parallelizes independent reads, shares compatible transport clients, and replaces provider protocols with native SDK operations where the SDK preserves collection controls.

## Implementation

- Enforce global and provider-scope admission at the remote-operation boundary. Retain charges across reload, release permits on cancellation, and avoid reserving global capacity while waiting for a busy scope.
- Run independent Kubernetes resource kinds, KEDA metrics, endpoint probes, and cloud inventory branches concurrently. Keep pagination within each resource ordered and bound intermediate evidence before accumulation.
- Keep GitHub reads serialized, following GitHub's guidance to avoid secondary rate limits. GitHub remains independent of cloud and Kubernetes collection and reuses the shared transport pool.
- Index queue worker ownership once per collection. Preserve pod UID and ReplicaSet ownership semantics.
- Share credential-free reqwest pools by connection settings across targets. Apply request deadlines and response limits from the executing check. Share a bounded DNS cache and inspect the certificate on the HTTP connection rather than opening another TLS connection.
- Use persistent official `aws-sdk-rust` clients for every configured AWS collection family. The 26 added service crates cover compute, containers, functions, load balancers, DNS/CDN/certificates, databases/caches, storage/backups, messaging/events, registries/builds/pipelines, keys/secret metadata, quotas, organization discovery, provider health and logs. CloudWatch, STS and Application Signals remain native. Remove handwritten signing and AWS REST request transmission. Preserve explicit pagination, allowlisted projections, partial failures, and credential refresh.
- Keep GCP REST collection while its released service transport lacks a configurable response bound and custom HTTP client. The inspected `google-cloud-gax-internal` 0.7.18 creates its own HTTP/1-only reqwest client and collects entire response bodies before parsing. Native credentials and typed service models remain in use. Revisit when the public transport supports these controls.
- Retain Azure management REST adapters. The current official Rust catalog covers identity and selected data-plane services rather than the ARM, Resource Graph, and Monitor operations used for fleet health. The existing native identity provider remains shared; no CLI collection or additional TLS stack is introduced.

## Verification

All 243 workspace tests pass on macOS. New tests cover concurrent Kubernetes and queue collection, scope isolation, reload admission, cancellation, shared inventory memory, native AWS request serialization, cross-target HTTPS connection reuse, certificate inspection, and bounded Kubernetes error responses. Formatting and Clippy with warnings denied pass. RustSec reports no advisories or warnings across 522 locked packages. The transport tree uses reqwest 0.13.4 and rustls 0.23.43 without native-tls/OpenSSL. Current changes have not been rerun on Linux. Missing AWS/Azure credentials and GCP ADC remain cloud live-validation gaps.

The final full-scope dev run attempted all 17 selected checks and completed in approximately 27 seconds, with GitHub determining total elapsed time. Kubernetes, KEDA and the configured NATS utility report completed; GitHub returned 1,292 observations. The report retained 11 health findings and the existing GCP authentication, flow-mapping, and provenance coverage gaps, exiting 3. There were no persistence faults or dropped transitions. Evidence is in `evidence/optimized-full-dev/`. The final dependency sweep reports all 75 direct libraries current.

The paired live comparison uses one unoptimized development executable on the M4 Max, one warm-up and five measured runs per variant, alternating order. Both variants execute the same checks on `dev`; `scope_concurrency` is one or four, and global concurrency remains four. No compilation ran during measurements. The [measurement utility and caveats](2026-09-05-release-python-benchmark.md) also apply here: CPU includes waited descendants, and sampled tree RSS sums resident pages and can count shared pages twice.

| Check | Scope concurrency | Median elapsed | Median CPU | Median peak tree RSS |
|---|---:|---:|---:|---:|
| Kubernetes | 1 | 7.14 s | 1.31 s | 83.4 MiB |
| Kubernetes | 4 | 3.11 s | 0.89 s | 93.2 MiB |
| KEDA queues | 1 | 6.32 s | 1.03 s | 84.3 MiB |
| KEDA queues | 4 | 2.21 s | 0.67 s | 86.5 MiB |

Four-way collection reduced median elapsed time by 56% for Kubernetes and 65% for queues, with completion 2.30x and 2.86x faster for these isolated invocations. CPU fell by 32% and 35%; peak summed RSS increased by 12% and 3%. These are collection-mode comparisons within the development build, not new release-versus-Python measurements or sustained-watch capacity estimates.

Every required read completed in all 24 invocations. Kubernetes covered 16 resource kinds and 27-28 pages; unavailable optional HTTPRoute discovery remained explicit. Queue runs collected seven prerequisite kinds over 12-13 pages and 18 KEDA demand observations. Kubernetes exited 1 for existing health findings; queues exited 0. There were no timeouts, incomplete memory samples, dropped transitions, or persistence faults. Live cluster churn accounts for minor record/page-count variation. NATS was disabled in both variants; native and fallback NATS operations also use the shared admission limits.

Reproduce with `cargo build --workspace`, then `target/debug/monitor-bench crates/bench/concurrency-plan.toml`, after selecting an unused output directory in the plan. The utility refuses to reuse existing measurement files. [Committed measurements](2026-09-05-concurrency-benchmark.json) include all 20 measured trials and min/median/max summaries; private snapshots are under `evidence/concurrency-benchmark/`. The measured binary SHA-256 is `9555e90e0909a82b511e9a5e242fce8a1818ee94a839f261419e5f698544b7ba`. The existing release binary remains `3dcc929fb2d85047f3ac81ca01f95720aa04cc576977e6de69663a0a5ab5c629`; all 37 recorded original-source/configuration files still match their checksums. No release builds, source-bundle changes, or infrastructure changes were performed.

## Sources

- [AWS Rust SDK](https://docs.aws.amazon.com/sdk-for-rust/).
- [Google client builder](https://docs.rs/google-cloud-gax/latest/google_cloud_gax/client_builder/struct.ClientBuilder.html); installed service transport source inspected for body collection and client construction.
- [Azure Resource Graph REST operations](https://learn.microsoft.com/en-us/rest/api/azureresourcegraph/resourcegraph/resources/resources?view=rest-azureresourcegraph-resourcegraph-2024-04-01).
- [Current official Azure Rust library catalog](https://learn.microsoft.com/en-us/azure/developer/rust/azure-sdk-library-package-index).
- [GitHub REST concurrency guidance](https://docs.github.com/en/rest/using-the-rest-api/best-practices-for-using-the-rest-api#avoid-concurrent-requests).
