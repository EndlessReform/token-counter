# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog], and this project adheres to
[Semantic Versioning].

## [0.2.0] - 2026-07-21

### Added

- Bundle the GPT-4o `o200k` tokenizer and use it by default, allowing the
  default configuration to work offline.
- Add streaming JSONL output with optional token strings and numeric token IDs.
- Add `-r`/`--recursive` directory traversal with deterministic lexical order.
- Add `-c`/`--count-only` output for shell pipelines.
- Add opt-in `.gitignore` filtering for files discovered through globs and
  recursive traversal.
- Add `--cost-per-mtok` estimates to human-readable and JSONL output.

### Changed

- Process files in parallel while preserving deterministic human-readable
  output order.
- Report and skip non-UTF-8 files discovered during traversal without failing
  the entire invocation.

## [0.1.0] - 2024-07-04

### Added

- Count tokens in files, glob matches, or standard input.
- Count multiple inputs and print their aggregate total.
- Select any Hugging Face tokenizer with `-m`/`--model`.

[Keep a Changelog]: https://keepachangelog.com/en/1.1.0/
[Semantic Versioning]: https://semver.org/spec/v2.0.0.html
[0.2.0]: https://github.com/EndlessReform/token-counter/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/EndlessReform/token-counter/releases/tag/v0.1.0
