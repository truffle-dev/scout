# scout

[![CI](https://github.com/truffle-dev/scout/actions/workflows/ci.yml/badge.svg)](https://github.com/truffle-dev/scout/actions/workflows/ci.yml)

Rank open-source issues worth contributing to.

Finding a ready first PR is a filter-plus-score problem. The
repos are known. The issues are public. The filter is the
product.

## Status

Pre-alpha. The scoring math and the config parser are
implemented and tested. The GitHub fetch layer is not yet
wired; every `scout` subcommand currently exits with `fetch
layer not implemented yet` (exit code 2). The CLI shape is
locked so the upcoming fetch-layer commits slot in without
renaming flags.

What's shipping first is the layer that can't be tested with
HTTP mocks: the scoring function.

## What it does

scout reads a list of GitHub repos you care about, looks at the
open issues on each, and returns the ones most likely to be
worth a PR. Ranking is a weighted sum of eight heuristics. Six
are binary, two have linear decay. Every weight lives in your
config, so the ranking is auditable and tunable per-user.

`scout explain OWNER/REPO#N` shows the per-heuristic breakdown
for a single issue so you can see which signals are carrying
the score.

## Install

```
cargo install --git https://github.com/truffle-dev/scout
```

Requires Rust 1.95 or newer (edition 2024).

## Commands

```
scout init [--force]
    Write starter config + watchlist under ~/.config/scout/.

scout scan [--limit N] [--json]
    Rank open issues across the watchlist.

scout took OWNER/REPO#N
    Record a contribution in the local ledger so cooldown_days
    filters the issue on subsequent scans.

scout explain OWNER/REPO#N
    Show the score breakdown for a single issue.
```

Global flags: `--config PATH`, `--watchlist PATH`.

## How it ranks

Eight heuristics, weight in `[0, 1]`, summed per issue, capped at
1.0 for display.

| heuristic          | default weight | what it measures                                      |
|--------------------|----------------|-------------------------------------------------------|
| root_cause         | 0.30           | issue body names a `file:line` or `file + symbol`     |
| no_pr              | 0.20           | no cross-linked open PR                               |
| recent             | 0.15           | updated within 14 days (linear decay)                 |
| contributing_ok    | 0.15           | CONTRIBUTING has no CLA or "contact first" block      |
| reproducer         | 0.10           | body has fenced code, stack trace, or repro link      |
| effort_ok          | 0.10           | not labeled `enhancement`, `question`, `design`, etc. |
| maintainer_touched | 0.05           | a top-5 committer commented on the thread             |
| active_repo        | 0.00           | repo `pushed_at` within 30 days (linear decay)        |

`active_repo` defaults to zero because dead repos are already
filtered at the watchlist level; it's there for users who want
to turn it on.

## Design principles

- No ML. The ranking is a weighted sum, not a model.
- No web UI. The terminal is the surface.
- No GitHub App, no SaaS backend. A personal access token is
  the auth story; everything runs locally.

## License

MIT. See [LICENSE](LICENSE).
