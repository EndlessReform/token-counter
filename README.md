# tc - Token Count

`tc` is a CLI tool for counting tokens in text files, as a lightweight wrapper around the Hugging Face [Tokenizers](https://docs.rs/tokenizers/latest/tokenizers/) crate. It's like the Unix `wc` command, but for tokens instead of words.

## Features

- Count tokens in files or from stdin
- Support for multiple files and glob patterns
- Works offline with the bundled GPT-4o tokenizer
- Uses any tokenizer available through Hugging Face Tokenizers

## Installation

```
cargo install token-counter
```

### Usage

Using the default tokenizer ([o200k](https://huggingface.co/Xenova/gpt-4o), the tokenizer used by GPT-4o):

```
tc file1.md file2.md
```

Using globs (quote the pattern if you want `tc`, rather than your shell, to expand it):

```
tc '*.md'
```

Reading from standard input:

```
printf 'Hello, world!' | tc
```

Arguments:

- `-m`, `--model`: Hugging Face model ID for the tokenizer (default: `Xenova/gpt-4o`; ex. `google-bert/bert-base-uncased`)
- `--jsonl`: Emit one `{"path": ..., "n_tokens": ...}` object per input as it completes
- `--show-tokens`: Include tokenizer vocabulary strings in JSONL output
- `--show-token-ids`: Include numeric token IDs in JSONL output

The default output preserves operand order and uses lexical order for matches
within each glob. JSONL output is streamed in nondeterministic completion order,
contains no total record, and writes diagnostics to standard error.
`--show-tokens` and `--show-token-ids` require `--jsonl`; when both are used,
their arrays align by index. Token strings are the tokenizer's vocabulary
representations and may contain markers such as `Ġ`, rather than clean decoded
substrings.

The default GPT-4o tokenizer is bundled and does not require a network connection.
Alternative tokenizers selected with `--model` are downloaded from Hugging Face
on first use and then cached. Counts do not include model-specific special tokens.
