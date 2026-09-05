# Soundpatrol healthcheck monitor

Read-only Rust monitoring for explicitly configured infrastructure, with one-off checks and continuous monitoring through the same engine. The [requirements and acceptance plan](docs/plans/2026-09-04-configurable-health-monitor.md) includes the July 10 operational checklist and documented live-validation gaps.

```text
cargo run -p healthcheck-monitor -- config validate --show-effective
cargo run -p healthcheck-monitor -- run --profile quick --target dev
cargo run -p healthcheck-monitor -- run --target api --check queues --samples 1
cargo run -p healthcheck-monitor -- watch --profile full --interval 60s --duration 30m
cargo run -p healthcheck-monitor -- report evidence/monitor-latest.json
cargo run -p healthcheck-monitor -- diff older.json newer.json
```

Edit `monitor.toml` or pass `--config`. Collection does not start interactive login. Use `auth status` for configured identities and `auth login <profile>` only when login is intended. Reports distinguish collection coverage from health and retain the selected scope.

See the [operations runbook](docs/plans/2026-09-05-monitor-operations.md) for configuration, permissions, evidence and foreground supervision, and the [verification record](docs/plans/2026-09-05-monitor-verification.md) for the source-test crosswalk and validation gaps.

The [persistent service and dashboard assessment](docs/plans/2026-09-05-health-dashboard-assessment.md) describes the proposed server/frontend architecture, implementation effort, operational requirements and comparison with Datadog.

The [release/Python benchmark](docs/plans/2026-09-05-release-python-benchmark.md) records live wall time, CPU time and process-tree memory measurements, scope differences, and reproduction commands.

The [collection optimization record](docs/plans/2026-09-05-collection-optimization.md) describes bounded parallel reads, shared HTTP/DNS pools, native AWS SDK coverage, and the paired concurrency benchmark. Four-way collection reduced development-build elapsed time by 56% for Kubernetes and 65% for KEDA queues on the configured dev target.

The toolchain is stable Rust with the prebuilt standard library. The original monitoring implementation was verified with 225 tests on macOS and Linux ARM64. The current workspace has 243 passing tests on macOS, including the benchmark utility and collection optimizations; current changes have not been rerun on Linux. Formatting, Clippy, and dependency/TLS audits pass. Development checks use `cargo test --workspace`, `cargo fmt --all --check`, and `cargo clippy --workspace --all-targets -- -D warnings`. Dependency updates use `cargo upgrade --incompatible`; dependency auditing uses `cargo audit`.

No infrastructure mutations, notifications, synthetic transactions, application database connections, secret-value reads, queue-message consumption, or response-body collection for endpoint probes are part of this service.
