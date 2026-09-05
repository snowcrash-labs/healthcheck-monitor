# Soundpatrol healthcheck monitor

Read-only Rust monitoring for explicitly configured infrastructure. The implementation is in progress; the [requirements and acceptance plan](docs/plans/2026-09-04-configurable-health-monitor.md) includes the July 10 operational checklist and remaining verification work.

```text
cargo run -p healthcheck-monitor -- config validate --show-effective
cargo run -p healthcheck-monitor -- run --profile quick --target dev
cargo run -p healthcheck-monitor -- run --target api --check queues --samples 1
cargo run -p healthcheck-monitor -- watch --profile full --interval 60s --duration 30m
cargo run -p healthcheck-monitor -- report evidence/monitor-latest.json
cargo run -p healthcheck-monitor -- diff older.json newer.json
```

Edit `monitor.toml` or pass `--config`. Collection does not start interactive login. Use `auth status` for configured identities and `auth login <profile>` only when login is intended. Reports distinguish collection coverage from health and retain the selected scope.

The toolchain is stable Rust with the prebuilt standard library. Development checks use `cargo test --workspace`, `cargo fmt --all --check`, and `cargo clippy --workspace --all-targets -- -D warnings`. Dependency updates use `cargo upgrade --incompatible`.

No infrastructure mutations, notifications, synthetic transactions, application database connections, secret-value reads, queue-message consumption, or response-body collection for endpoint probes are part of this service.

