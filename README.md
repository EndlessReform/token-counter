# tc - Token Count

`tc` is a CLI tool for counting tokens in text files, as a lightweight wrapper around the HuggingFace [Tokenizers](https://docs.rs/tokenizers/latest/tokenizers/) crate. It's like the Unix `wc` command, but for tokens instead of words.

## Features

- Count tokens in files or from stdin
- Support for multiple files and glob patterns
- Uses any tokenizer in HuggingFace Tokenizers

## Installation

```
cargo install token-counter
```

### Usage

Using default tokenizer ([cl100k](https://huggingface.co/DWDMaiMai/tiktoken_cl100k_base), the tokenizer for GPT-3.5 and GPT-4):

```
tc file1.md file2.md
```

Using globs:

```
tc *.md
```

Arguments:

- `-m`, `--model`: HuggingFace ID of the model for tokenizer (ex. `google-bert/bert-base-uncased`)
