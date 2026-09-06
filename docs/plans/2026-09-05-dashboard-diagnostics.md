# Dashboard diagnostics and navigation

The dashboard previously rendered coverage enum names without explanations, linked targets only to filtered resources, and retained one observation's facts per resource. Operators could see incomplete coverage or an error without identifying the underlying check, source, location, or triggering values.

## Behavior

Target panels open dedicated overviews containing current problems, configured checks, coverage counts, resources and history links. Coverage counts open the check list. Every configured check has a name and description, including checks awaiting their first observation. Complete required coverage, optional gaps and resource health remain distinct.

Check details expose operation identity, coverage explanation, required versus optional status, records, pages, attempts, collection time and recent runs. Cards preview three gaps and explicitly link to every operation. Search and keyset pagination cover all collected operations. Generic truncation outcomes do not identify the cutoff; the interface says when that detail was not recorded.

Resource details lead with active findings and retained triggering values. Native provider identity, scope, region, zone, namespace, pod UID, container, available reason code and exit status accompany observation and detection times. Separate source checks and components retain their timestamps. List previews use triggering values when available. Unresolved findings remain navigable when current inventory disappears.

First detection belongs to an active finding episode. Repeated failures preserve it; recovery evaluations do not advance the latest confirmed failure timestamp. Legacy snapshots retain an unknown first detection time. Triggering evidence survives restart and failed collection, with a separate bounded allocation allowance within the configured memory limit.

Console links use native identifiers and fixed HTTPS origins. GCP and AWS use service-specific routes where supported; Azure uses ARM resource identity. Broader destinations are labeled as console or search links. Unrecognized Kubernetes context aliases do not fabricate cluster identities. GCP log links carry source labels and absolute time bounds; mixed-source groups retain common location fields. Secret values, environment values and raw log messages remain excluded.

## Interfaces

The API adds `/api/v1/checks`, `/api/v1/check`, `/api/v1/check/operations` and `/api/v1/resource/evidence`. Resource and finding lists accept check filters; finding lists also accept an exact resource filter. Run history accepts a check filter backed by a combined target/check/time/UUID index. Resource details request findings separately through pagination; the original combined endpoint remains available for older clients.

Resource and finding DTOs include typed diagnostic context. Snapshot additions are optional when reading older evidence. Published views share immutable evidence collections, so paging and overview refreshes do not clone all observations or operation arrays. Dashboard reads do not initiate cloud collection. The compressed Solid/Vite bundle remains embedded at compile time.

## Dependency currency

`cargo upgrade --incompatible` confirmed all 94 direct registry dependencies were current. `cargo update` advanced seven transitive packages. Current upstream crates constrain three transitive versions: `crypto-common 0.1.7` requires `generic-array =0.14.7`; `axum 0.8.9` requires `matchit =0.8.4`; `serde-saphyr 0.0.29`, through `kube-client 4.2.0`, requires `smallvec <1.16.0`. Resolver checks confirmed that newer versions cannot satisfy those requirements.

Frontend packages match current registry releases. The requested Solid 2 channel uses `solid-js` and `@solidjs/web` `2.0.0-rc.6`, router `2.0.0-next.21`, Vite `8.2.2`, and Vite plugin `3.0.0-next.39`. TypeScript remains strict; API JSON enters as unknown before schema validation.

## Verification and rollout

Regression tests cover readable labels, complete check lists, operation pagination beyond previews, scoped navigation, direct-load target selection, multi-source evidence, detection timestamps, restart, legacy snapshots, pod replacement, console destinations, log-query encoding and retained findings without inventory. Desktop and mobile browser scenarios cover both themes. Existing authentication, response bounds, freshness and collection tests remain required.

Use development builds and the existing IAP-protected deployment path. The embedded migration adds only a history-query index. The existing VM, compile-time assets and devops-only access policy continue to serve the dashboard.
