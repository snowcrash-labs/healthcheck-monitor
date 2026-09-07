# Offline Daily Q/A Skill Reference

## Goal

Make the Agent Deck daily Q/A skill easy for healthcheck-monitor contributors
to inspect without making it part of the deployed monitor, MCP server, or an
automatically discovered repository skill set.

## Layout

Store the snapshot under
`docs/offline-agent-skills/agentdeck-daily-q-a-and-review/`. Keep the upstream
`SKILL.md` unchanged, place its small risk catalog beneath `references/`, and
add a local README that records provenance, runtime dependencies, and the
reference-only boundary.

The large state and PDF-report helpers remain in `move-agent-deck`, their source
of truth. Copying those helpers here would create a second executable workflow
that could drift. The README will point colleagues to the upstream repository
when they want to install or run the skill.

## Safety and maintenance

- Nothing under this directory is linked from service configuration, Rust code,
  dashboard code, or deployment manifests.
- The path is deliberately outside `.agents/skills`, `.claude/skills`, and the
  plugin tree, so ordinary skill discovery does not load it.
- Provenance includes the upstream repository, source path, commit, and snapshot
  date.
- Future refreshes replace the copied files from a named upstream revision and
  update the provenance record in the same change.

## Verification

Verify that the copied skill and risk catalog match their upstream files byte
for byte, validate the copied skill's frontmatter, check documentation links,
and inspect the final diff for any runtime integration.
