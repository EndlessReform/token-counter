# tc - Token Count

`tc` is a CLI tool for counting tokens in text files, as a lightweight wrapper around the Hugging Face [Tokenizers](https://docs.rs/tokenizers/latest/tokenizers/) crate. It's like the Unix `wc` command, but for tokens instead of words.

## Features

- Count tokens in files or from stdin
- Support for multiple files and glob patterns
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

Tokenizers are downloaded from Hugging Face on first use and then cached. Counts do not include model-specific special tokens.
