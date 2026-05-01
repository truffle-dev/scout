# architecture decision record

Backfilled 2026-05-01. The candidate spec (in my private wiki, dated
2026-04-22) called for a one-page architecture decision document on
day one. I shipped commits before I shipped the doc. This file is
that doc, written after enough of the system exists that the choices
are real.

## the insight, in one sentence

Finding a ready first PR is a filter-plus-score problem, not a
discovery problem. The repos are known. The issues are public. The
filter is the product.

## what scout is

A single-binary CLI that reads a list of GitHub repos a user already
cares about, walks their open issues, and returns the ones most
likely to be worth a PR right now. The answer is a ranked list with
a per-issue score and an `explain` breakdown so a human can audit
why an issue ranked where it did.

## what scout refuses to be

- **Not ML.** The ranking is a weighted sum of explicit heuristics,
  and every weight lives in the user's config. A user disagreeing
  with the score should be able to read the math and tune it. A
  model would hide that surface and force every dispute through
  retraining.
- **Not a discovery tool.** scout doesn't suggest repos; the user
  brings the watchlist. Discovery is a different problem with
  different signals (taste, ecosystem fit, language preference) and
  bundling it would dilute the ranking story.
- **Not a web UI.** Terminal output is the surface. Markdown table
  by default, JSON with `--json`. A future TUI is possible but it's
  not the v0.1 shape.
- **Not a GitHub App, not a SaaS backend.** Personal access token
  auth, local config, local ledger. Nothing leaves the machine
  except the GitHub API calls themselves.
- **Not a list of curated repos.** That's `goodfirstissue.dev` and
  `goodfirstissues.com`. Both exist. Both are static. The gap is
  the per-user, per-watchlist, live-rerun-able filter, not a curated
  index.

## the data flow

```
watchlist.yaml ─┐
                ├─► fetcher ─► planner ─► rank ─► render
config.toml ────┤
                │
ledger.jsonl ───┘
```

- **watchlist.yaml**: hand-rolled YAML, one repo per line. Parsed
  by `src/watchlist.rs` with a strict "unknown keys fail" stance.
- **config.toml**: weights, filter thresholds, output preferences,
  optional auth path. TOML with partial-hydration so users can
  override only the fields they care about, but unknown keys fail
  the parse loud rather than silently being ignored.
- **ledger.jsonl**: append-only `OWNER/REPO#N timestamp` lines,
  written by `scout took`. The cooldown filter reads the ledger and
  drops issues that were touched within the configured window so
  the same issue doesn't keep re-surfacing during a contribution
  attempt.
- **fetcher**: async layer (reqwest + tokio). Walks the watchlist,
  pulls `RepoMeta`, optional `CONTRIBUTING.md`, paginated open
  issues, and per-issue comments + timeline. Pre-filters
  PR-shaped issue rows before their per-issue pages would be
  fetched, since the planner is going to drop them anyway.
- **planner** (sync): folds the fetched payloads, the ledger, and
  the config filters into a `Vec<RankInput>` ready for scoring.
  Drops items that fail age, score-floor, label, or cooldown
  filters.
- **rank** (sync, pure): runs each `RankInput` through the
  weighted-sum scoring against a `Factors` struct, returns
  `Vec<RankedRow>` sorted descending.
- **render**: markdown table by default, JSON when asked.

The split between fetcher (async, network-bound) and planner +
rank (sync, pure) keeps the testable surface large. Wiremock
covers fetch. Plain unit tests cover planner, scoring, and render.

## the scoring shape

Eight heuristics. Six binary, two with linear decay (`recent` and
`active_repo`). Default weights sum to 1.0, capped at 1.0 for
display.

| heuristic          | default | what it measures                                      |
|--------------------|--------:|-------------------------------------------------------|
| root_cause         |   0.30  | issue body names a `file:line` or `file + symbol`     |
| no_pr              |   0.20  | no cross-linked open PR                               |
| recent             |   0.15  | updated within 14 days (linear decay)                 |
| contributing_ok    |   0.15  | CONTRIBUTING has no CLA or "contact first" block      |
| reproducer         |   0.10  | body has fenced code, stack trace, or repro link      |
| effort_ok          |   0.10  | not labeled `enhancement`, `question`, `design`, etc. |
| maintainer_touched |   0.05  | a top-5 committer commented on the thread             |
| active_repo        |   0.00  | repo `pushed_at` within 30 days (linear decay)        |

`active_repo` defaults to zero because dead repos are already
filtered at the watchlist level. It's there for users who want it.

## decisions worth naming

- **Rust over Go.** Single-binary ergonomics and a strict type
  system make a scoring layer with eight independent heuristics
  easier to keep consistent. The cost: I haven't shipped a public
  Rust CLI before this. Risk acknowledged, learning is a deliberate
  part of the bet.
- **`reqwest` direct, not `gh` shell-out.** Earlier candidate text
  flirted with shelling to `gh`. I rejected that on day one. A
  binary that needs `gh` on `$PATH` is two binaries, and the auth
  story gets muddier. Direct GitHub REST with a personal access
  token is one binary, one config field.
- **Strict-unknown-keys on config and watchlist.** Loud failures
  beat silent typos. A misspelled weight should be a parse error,
  not a silently-ignored line.
- **JSONL ledger, not SQLite.** A few hundred lines per year,
  read once per scan. SQLite would buy nothing and add a build
  dependency.
- **Serial fetcher in the v0.1.0 path.** The first cut walks repos
  and issues serially. It's slow on large watchlists. A bounded-
  concurrency rewrite is the v0.2 work; until then the doc-comment
  in `src/fetcher.rs` names the limitation explicitly.

## what comes next

Named in `docs/monthly-updates/2026-05.md`.
