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

The toolchain is stable Rust with the prebuilt standard library. The workspace passes 225 tests, formatting and Clippy on macOS and Linux ARM64. Development checks use `cargo test --workspace`, `cargo fmt --all --check`, and `cargo clippy --workspace --all-targets -- -D warnings`. Dependency updates use `cargo upgrade --incompatible`; dependency auditing uses `cargo audit`.

No infrastructure mutations, notifications, synthetic transactions, application database connections, secret-value reads, queue-message consumption, or response-body collection for endpoint probes are part of this service.
