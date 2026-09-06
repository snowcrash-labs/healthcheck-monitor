# Dashboard frontend conventions

The dashboard uses SolidJS 2.0 RC and `@solidjs/web` for JSX and rendering, with the compatible Solid router. Global connection, target selection, current overview and refresh state live in Solid context. Effects use separate compute and apply functions; setup and cleanup use `onSettled` or returned effect cleanup. Props are read reactively inside JSX or derived computations.

All styling is in centralized CSS. Native form controls, visible focus, semantic tables and explicit empty/error states support keyboard and screen-reader use. Status colors are accompanied by text. Observation timestamps retain their source age, and a disconnected browser cannot present an apparently current green state.

Resource rows show the two highest-severity active finding summaries, total finding count, evidence age or staleness, and up to three observed facts when findings exist. The API joins findings to one bounded resource page in a single pass; the browser does not issue per-resource requests. Each summary links to the full resource detail.

API responses are validated before entering state. Lists use server pagination and obsolete requests are cancelled. Event streams announce revisions rather than transferring full evidence. Refreshing a view reads current state and never launches collection. Dependencies are locked; Solid and its renderer use the requested 2.0 release candidate while other dependencies track current compatible releases.
