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
