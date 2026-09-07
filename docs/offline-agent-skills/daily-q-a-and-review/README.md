# Daily Q/A and Review skill

This directory contains a reference-only, locally runnable adaptation of the
SoundPatrol Agent Deck daily Q/A skill. It is documentation: the monitor,
dashboard, polling worker, and MCP server do not discover or execute it.

## Use it locally

An agent running on a colleague's development machine can read `SKILL.md`
directly, or the directory can be copied into that agent's personal skill
directory. Invoke it as `$daily-q-a-and-review` after authenticating the local
tools needed for the evidence sources you intend to inspect. The skill does not
grant access or expand the agent's existing permissions.

The portable version has no scheduler, cursor store, Agent Deck transaction, or
PDF renderer. Each invocation reviews the time window supplied by the caller,
defaulting to the preceding 24 hours, and returns its report in the current
session. The healthcheck-monitor MCP interface is a preferred evidence source
when it is available because it exposes already-collected, read-only evidence.

## Provenance

- Upstream repository: [snowcrash-labs/move-agent-deck](https://github.com/snowcrash-labs/move-agent-deck)
- Upstream file: `codex-skills/agentdeck-daily-q-a-and-review/SKILL.md`
- Upstream revision: `c5b3487b093c7eff6e1a35cf1d9334946216847d`
- Snapshot date: 2026-09-07

This adaptation removes the Agent Deck invocation, Ben-specific paths and
feedback file, durable cursor transaction, branded PDF generation, and
column-eight response instructions. It retains the upstream read-only boundary,
query budgets, evidence discipline, risk-driven checks, and compact report
shape. The risk catalog is copied without modification.

When refreshing this reference, start from a named upstream revision, reapply
only the portability changes above, and update the revision and snapshot date in
the same pull request.
