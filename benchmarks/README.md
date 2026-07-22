# File-parallelism benchmark

This benchmark compares sequential per-file tokenization with Rayon
file-parallel tokenization. Tokenizer loading and fixture discovery happen
outside the timed region. Both modes read each file and tokenize its complete
contents inside the timed region.

## Frozen fixture

- Repository: <https://github.com/BurntSushi/ripgrep>
- Tag: `15.0.0`
- Commit: `3a612f88b805e14aef45bfa43e25a54abc6297fc`
- Included tracked files: `.rs`, `.md`, `.toml`, and `.txt`
- Expected fixture size: 134 files and 2,081,996 bytes
- Tokenizer: `Xenova/gpt-4o`
- Tokenizer revision: `7956d98f2a83b2751a98ea7136fdf7fe6cf54e69`

The repository tag and tokenizer revision are both pinned so later upstream
changes cannot silently change the workload.

## Run

Clone the fixture at the pinned tag, download the pinned tokenizer snapshot,
then run:

```console
cargo bench --bench rayon_file_processing -- \
  /path/to/ripgrep \
  /path/to/tokenizer.json \
  15
```

The benchmark performs one untimed warm-up per mode and then alternates which
mode runs first. It prints every raw sample, median times, relative changes,
and a correctness receipt. The single-file control uses the largest included
fixture file.

## Decision criterion

Correctness is a gate: the modes must return the same path/count mapping and
total, with no omitted or duplicated paths. Stdin safety is assessed in the
CLI design because this file-only benchmark never reads stdin.

Retain explicit Rayon file parallelism if its median is at least 10% faster on
the multi-file fixture without making the single-file control more than 5%
slower. Results within those bands favor preserving the existing architecture;
removing the dependency would require separate evidence of a meaningful build
or maintenance benefit.

Output order is deliberately outside this criterion; see `TODO.md`.
