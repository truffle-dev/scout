# scout

[![CI](https://github.com/truffle-dev/scout/actions/workflows/ci.yml/badge.svg)](https://github.com/truffle-dev/scout/actions/workflows/ci.yml)

Rank open-source issues worth contributing to.

Finding a ready first PR is a filter-plus-score problem. The
repos are known. The issues are public. The filter is the
product.

## Status

Alpha (v0.2.0). The pipeline runs end to end. `scout init && scout
scan` on the default empty watchlist exercises every layer without
HTTP and prints a header-only markdown table; `scout scan` against
a real watchlist returns ranked rows. Hard planner-time filters
drop candidates with a cross-linked open PR, a maintainer
"discuss-first" comment, or an issue handed to Copilot's swe-agent
before they reach the scoring layer. Since v0.2.0 a fresh `scout
init` also bundles `exclude_repos` defaults for seven venues whose
maintainers have publicly stated anti-AI-contribution positions
(`astral-sh/*`, `atuinsh/atuin`, `clap-rs/*`, `helix-editor/helix`,
`starship/*`, `stjude-rust-labs/*`, `typst/typst`); override by
editing the generated `config.toml`.

Layer-by-layer:

1. Scoring math and config parser. Implemented + tested.
2. Fetch layer: types, decoders, async HTTP clients for
   `/repos/:o/:r`, the issues listing with `Link: rel="next"`
   pagination, per-issue comments and timeline, and the
   CONTRIBUTING body. Implemented + tested (wiremock integration
   + live smoke against `api.github.com`).
3. Signal inference (body pattern-match, label classifiers,
   ISO-8601 day deltas) and the `factors_from` aggregator binding
   fetch output to a scoring `Factors`. All eight heuristics now
   have live fetch coverage. Implemented + tested.
4. CLI wiring. `scout init` writes the starter `config.toml` and
   `watchlist.yaml` under `~/.config/scout/` (XDG-aware). `scout
   took OWNER/REPO#N` appends a JSONL entry to
   `~/.config/scout/ledger.jsonl` so the cooldown filter can skip
   the issue on subsequent scans. `scout scan` runs the full
   fetch + plan + rank + render pipeline. `scout explain
   OWNER/REPO#N` prints the per-heuristic breakdown for one
   issue so the weighted sum is auditable.

Prebuilt tarballs for Linux x86_64, Linux ARM, and macOS ARM are
attached to each release (see
[`.github/workflows/release.yml`](.github/workflows/release.yml)
for the build matrix). Intel Mac users build from source via
`cargo install --git`. The full month-two assessment lives in
[`docs/monthly-updates/2026-05.md`](docs/monthly-updates/2026-05.md);
the design rationale is at
[`docs/architecture.md`](docs/architecture.md).

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

Tagged releases:

```
cargo install --git https://github.com/truffle-dev/scout --tag v0.2.0
```

Latest from `main`:

```
cargo install --git https://github.com/truffle-dev/scout
```

Or download a prebuilt tarball from the
[releases page](https://github.com/truffle-dev/scout/releases).
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

scout dropped OWNER/REPO#N
    Record an investigated-and-abandoned engagement. Same cooldown
    effect as `took`; different event tag so the two outcomes are
    distinguishable in the ledger.

scout explain OWNER/REPO#N
    Show the score breakdown for a single issue.
```

Global flags: `--config PATH`, `--watchlist PATH`, `--ledger PATH`.

## Example output

`scout scan` returns a ranked markdown table. Shape:

| score | issue | title |
| ----: | :---- | :---- |
| 0.85 | [mlc-ai/mlc-llm#3487](https://github.com/mlc-ai/mlc-llm/issues/3487) | Tensor parallelism on Vulkan fails with SequentiallyConsistent SPIR-V barrier on Windows |
| 0.70 | [clap-rs/clap#6372](https://github.com/clap-rs/clap/issues/6372) | Zsh completion scripts break when ksharrays is enabled |
| 0.60 | [starship/starship#7450](https://github.com/starship/starship/issues/7450) | Exit status does not clear on empty command for the `status` and `character` modules |

Scores depend on each issue's current state and your config weights, so
rerunning `scout scan` reflects updates and any tuning in `config.toml`.

`scout explain clap-rs/clap#6353` shows the per-heuristic breakdown
behind a single score:

```
# `ValueCompleter::complete_at` for indexed multi-value completion

https://github.com/clap-rs/clap/issues/6353

**Score**: 0.85

| factor             | weighted |
| :----------------- | -------: |
| root_cause         |    0.300 |
| no_pr              |    0.200 |
| recent             |    0.150 |
| contributing_ok    |    0.150 |
| reproducer         |    0.000 |
| effort_ok          |    0.100 |
| maintainer_touched |    0.050 |
| active_repo        |    0.000 |
```

`scout scan --json` returns the same ranked rows as a single-line JSON
array (one element per row, with `score` and `parts` for the per-factor
contributions) so downstream tools like `jq` can re-rank or annotate.

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
