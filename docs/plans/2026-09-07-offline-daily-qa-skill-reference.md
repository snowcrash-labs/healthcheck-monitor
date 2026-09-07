# Offline Daily Q/A Skill Reference

## Goal

Make the Agent Deck daily Q/A skill easy for healthcheck-monitor contributors
to inspect without making it part of the deployed monitor, MCP server, or an
automatically discovered repository skill set.

## Layout

Store the adapted snapshot under
`docs/offline-agent-skills/daily-q-a-and-review/`. Place its small risk catalog
beneath `references/`, and add a local README that records provenance, explains
how to use the skill from a local agent, and states the reference-only boundary.

Remove the Agent Deck pad, transaction state, PDF-report, column-eight feedback,
and Ben-specific path assumptions from the copied instructions. Those helpers
remain in `move-agent-deck`, their source of truth. Copying them here would
create a second executable workflow that could drift. The portable version
instead reviews a caller-specified window, defaulting to the preceding 24 hours,
and returns a compact Markdown report in the current session.

## Safety and maintenance

- Nothing under this directory is linked from service configuration, Rust code,
  dashboard code, or deployment manifests.
- The path is deliberately outside `.agents/skills`, `.claude/skills`, and the
  plugin tree, so ordinary skill discovery does not load it.
- Provenance includes the upstream repository, source path, commit, snapshot
  date, and a concise list of intentional adaptations.
- Future refreshes replace the copied files from a named upstream revision and
  update the provenance record in the same change.

## Verification

Verify that the risk catalog matches its upstream file byte for byte, validate
the adapted skill's frontmatter, assert that Agent Deck-specific terms and paths
are absent from the runnable instructions, check documentation links, and
inspect the final diff for any runtime integration.
