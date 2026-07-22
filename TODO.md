# Output decisions

## Output ordering

The default human-readable output is stable: operand order followed by lexical
order within each glob. It remains buffered and includes a total for multiple
inputs.

`--jsonl` emits one uniform `path`/`n_tokens` object per successfully processed
input in nondeterministic completion order. It is streamed without a total
record; diagnostics go to standard error and any input error produces a nonzero
exit. `--show-tokens` and `--show-token-ids` optionally add aligned `tokens` and
`token_ids` arrays.

## 0.2 backlog

### `-r` / `--recursive` — directory walking

Accept directory paths as operands and walk them recursively, expanding to all
files under each directory. Operand order is preserved; files within a directory
are visited in lexical order. Should compose with glob expansion (e.g.
`tc -r src/ '*.md'`). Binary files that fail UTF-8 decoding should be reported
to stderr and skipped, not fatal.

### `--limit <N>` — context window check

Accept a token count threshold (e.g. `--limit 128000` or `--limit 128k`).
After processing, exit nonzero and print a diagnostic to stderr for every input
that exceeds the limit. In human-readable mode, annotate over-limit lines (e.g.
`OVER`). In JSONL mode, add an `over_limit: true` field to affected records. The
total also participates in the check when multiple inputs are given.

### `-c` / `--count-only` — suppress filenames

Print only the token count(s), one per line, with no filename or `total` label.
Matches the spirit of `wc -c`. Useful when piping output into other tools.
Incompatible with `--jsonl` (which already gives structured output).

### `--cost-per-mtok <PRICE>` — cost estimation

Accept a price in USD per million tokens (e.g. `--cost-per-mtok 2.50`).
Print estimated cost alongside each token count in human-readable mode.
In JSONL mode, add a `cost_usd` field to each record. Format cost with enough
decimal places to be meaningful at small token counts.

### `--gitignore` — opt-in gitignore respect

When this flag is set, skip any file that would be ignored by git according to
`.gitignore` rules found in the file tree. Only applies to files reached via
`-r` directory walking or glob expansion — explicit file operands are always
processed. Should walk `.gitignore` files up to the repo root, matching
standard git precedence rules.

### CHANGELOG

Write a `CHANGELOG.md` following Keep a Changelog conventions. Document the
0.1.0 feature set as the initial release and 0.2.0 additions as they land.
