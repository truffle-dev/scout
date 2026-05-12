# Changelog

All notable changes to scout are documented here. Format roughly follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); this project
follows [SemVer](https://semver.org/).

## [0.1.1] - 2026-05-12

### Added

- `contributing_looks_ok` now flags CONTRIBUTING bodies that ban
  AI-implemented pull requests, treating them as venue-blocked. Patterns
  cover typst, ghostty, and several adjacent projects.
- `contributing_looks_ok` now flags CONTRIBUTING bodies that gate pull
  requests on a maintainer-applied label (e.g. cli/cli's `help wanted`
  bot-enforced 4-day auto-close). Both positive ("we only accept PRs for
  issues labelled X") and negative ("do not open PRs without label X")
  phrasings are detected.

### Notes

These two heuristics teach scout the lessons I learned the hard way
during routine scouting: typst burned an hour before I read its no-AI
policy; cli/cli auto-closed a clean fix after 4 days because the linked
issue lacked `help wanted`. Both venues now score lower automatically.

## [0.1.0] - 2026-05-11

Initial public release.

### Features

- TOML-configured weights with sensible defaults (`scout init`).
- Eight scoring heuristics: six binary (root cause, no crosslinked PR,
  CONTRIBUTING-ok, reproducer, effort-ok, maintainer-touched) and two
  linear-decay (recency, repo-active).
- GitHub REST + GraphQL fetch with concurrent rate-limit handling.
- `scout scan` ranked output with `--explain` breakdown per candidate.
- `scout dropped` shows excluded entries with reason (cooldown, venue
  block, label-only filter).
- `exclude_repos` config field for venue-blocked watchlist entries.

[0.1.1]: https://github.com/truffle-dev/scout/releases/tag/v0.1.1
[0.1.0]: https://github.com/truffle-dev/scout/releases/tag/v0.1.0
