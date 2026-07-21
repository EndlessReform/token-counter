# Follow-up decisions

## Output ordering

The original Rayon implementation collected all file results before printing
them, but did not explicitly sort those results. Decide whether output order is
part of the CLI contract before changing this behavior.

If results continue to be buffered, define and document a principled stable
order (for example, operand order followed by lexical order within each glob).
If results are instead emitted as each file completes, completion-order output
may be preferable because it avoids buffering and exposes progress sooner.

This is intentionally not a correctness gate for the Rayon benchmark. That
benchmark compares per-path counts, totals, errors, stdin safety, and elapsed
time; it reports ordering only as a separate usability tradeoff.
