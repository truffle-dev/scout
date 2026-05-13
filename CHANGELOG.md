# Changelog

All notable changes to scout are documented here. Format roughly follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); this project
follows [SemVer](https://semver.org/).

## [0.1.3] - 2026-05-13

### Added

- `drop_if_copilot_assigned` hard filter at planner time, gated by a
  new `[filters]` knob (default `true`). Issues whose timeline carries
  an `assigned` event naming GitHub's Copilot swe-agent (`Copilot` or
  `copilot-swe-agent`) are dropped before scoring. This closes the
  race window where a maintainer has handed the issue to Copilot but
  the auto-PR has not yet emitted its cross-referenced event, which
  `drop_if_open_pr` cannot see.

### Notes

The motivating shape is vitest#10307: hi-ogawa assigned Copilot at
2026-05-09T01:34Z, the auto-PR cross-reference fired at
2026-05-11T23:02Z. A scout run inside that ~2-day window would have
surfaced an issue a human had no business taking. `fetch::TimelineEvent`
gains an optional `assignee: Option<UserRef>` field via
`#[serde(default)]`, so existing fixtures and cached transcripts
continue to deserialize without change.

## [0.1.2] - 2026-05-13

### Added

- `drop_if_open_pr` hard filter at planner time, gated by a new
  `[filters]` knob (default `true`). Issues with a cross-referenced
  open PR are dropped before scoring; the soft `no_pr` weight stays
  for users who opt out and want to see the candidates anyway with
  the penalty applied.
- `drop_if_pending_discussion` hard filter at planner time, gated by
  a new `[filters]` knob (default `true`). Issues with maintainer
  comments matching "should be an RFC", "we need to decide",
  "let's discuss this first", "haven't decided yet", "before
  implementing", and similar gate-shapes are dropped before scoring.
  These typically close as "should be an issue, not a PR" and are
  best surfaced as discussion targets rather than PR targets.

### Notes

Both filters stem from concrete scouting incidents where strong
root-cause + reproducer signals lifted candidates above `min_score`
despite a cross-linked PR or an explicit maintainer "discuss-first"
note. Moving the check from a soft penalty to a hard filter
short-circuits the wasted-PR trap at the planner layer instead of
relying on the user to read every `--explain` breakdown.

`fetch::CommentMeta` gains an optional `body` field via
`#[serde(default)]` for the discussion-detection inference. Existing
callers that fed only user + author_association into the struct
continue to deserialize cleanly.

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

[0.1.2]: https://github.com/truffle-dev/scout/releases/tag/v0.1.2
[0.1.1]: https://github.com/truffle-dev/scout/releases/tag/v0.1.1
[0.1.0]: https://github.com/truffle-dev/scout/releases/tag/v0.1.0
