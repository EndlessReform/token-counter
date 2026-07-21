use glob::glob;
use rayon::prelude::*;
use std::fs::File;
use std::io::{self, BufReader, Read};
use std::path::{Path, PathBuf};
use tokenizers::Tokenizer;

pub const DEFAULT_MODEL: &str = "Xenova/gpt-4o";
const DEFAULT_TOKENIZER_JSON: &[u8] =
    include_bytes!("../assets/tokenizers/xenova-gpt-4o/tokenizer.json");

/// Loads the bundled default tokenizer or downloads a requested alternative.
pub fn load_tokenizer(model: &str) -> tokenizers::Result<Tokenizer> {
    if model == DEFAULT_MODEL {
        Tokenizer::from_bytes(DEFAULT_TOKENIZER_JSON)
    } else {
        Tokenizer::from_pretrained(model, None)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Input {
    Stdin,
    File(PathBuf),
}

impl Input {
    pub fn label(&self) -> String {
        match self {
            Self::Stdin => "-".to_owned(),
            Self::File(path) => path.display().to_string(),
        }
    }

    pub fn is_stdin(&self) -> bool {
        matches!(self, Self::Stdin)
    }
}

pub type CountResult = (Input, io::Result<usize>);

pub fn count_tokens(content: &str, tokenizer: &Tokenizer) -> io::Result<usize> {
    tokenizer
        .encode(content, false)
        .map(|encoding| encoding.len())
        .map_err(io::Error::other)
}

pub fn count_file(path: &Path, tokenizer: &Tokenizer) -> io::Result<usize> {
    count_reader(BufReader::new(File::open(path)?), tokenizer)
}

pub fn count_reader(mut reader: impl Read, tokenizer: &Tokenizer) -> io::Result<usize> {
    let mut buffer = String::new();
    reader.read_to_string(&mut buffer)?;
    count_tokens(&buffer, tokenizer)
}

fn has_glob_metacharacters(value: &str) -> bool {
    value.contains(['*', '?', '['])
}

pub fn expand_inputs(files: &[String]) -> (Vec<Input>, Vec<String>) {
    let mut inputs = Vec::new();
    let mut errors = Vec::new();

    for value in files {
        if value == "-" {
            inputs.push(Input::Stdin);
        } else if has_glob_metacharacters(value) {
            match glob(value) {
                Ok(paths) => {
                    let mut matched = false;
                    for entry in paths {
                        match entry {
                            Ok(path) => {
                                matched = true;
                                inputs.push(Input::File(path));
                            }
                            Err(error) => errors.push(format!("{value}: {error}")),
                        }
                    }
                    if !matched {
                        errors.push(format!("{value}: no matches"));
                    }
                }
                Err(error) => errors.push(format!("{value}: invalid glob: {error}")),
            }
        } else {
            inputs.push(Input::File(PathBuf::from(value)));
        }
    }

    (inputs, errors)
}

/// Counts all inputs, processing files in parallel while reading stdin only once.
///
/// Results correspond to the input vector. A repeated `-` observes EOF after the
/// first read, matching ordinary sequential stream behavior.
pub fn process_inputs(
    inputs: &[Input],
    tokenizer: &Tokenizer,
    mut stdin: impl Read,
) -> Vec<CountResult> {
    let mut results: Vec<Option<CountResult>> = (0..inputs.len()).map(|_| None).collect();
    let mut stdin_read = false;

    for (index, input) in inputs.iter().enumerate() {
        if input.is_stdin() {
            let result = if stdin_read {
                Ok(0)
            } else {
                stdin_read = true;
                count_reader(&mut stdin, tokenizer)
            };
            results[index] = Some((input.clone(), result));
        }
    }

    let file_results: Vec<_> = inputs
        .par_iter()
        .enumerate()
        .filter_map(|(index, input)| match input {
            Input::Stdin => None,
            Input::File(path) => Some((index, input.clone(), count_file(path, tokenizer))),
        })
        .collect();

    for (index, input, result) in file_results {
        results[index] = Some((input, result));
    }

    results
        .into_iter()
        .map(|result| result.expect("every input is processed exactly once"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use tokenizers::models::bpe::BPE;
    use tokenizers::pre_tokenizers::whitespace::Whitespace;

    fn test_tokenizer() -> Tokenizer {
        let model = BPE::builder()
            .vocab_and_merges(
                [
                    ("[UNK]".to_owned(), 0),
                    ("hello".to_owned(), 1),
                    ("world".to_owned(), 2),
                ],
                Vec::new(),
            )
            .unk_token("[UNK]".to_owned())
            .build()
            .expect("valid BPE model");
        let mut tokenizer = Tokenizer::new(model);
        tokenizer.with_pre_tokenizer(Some(Whitespace {}));
        tokenizer
    }

    #[test]
    fn bundled_default_tokenizer_matches_o200k() {
        let tokenizer = load_tokenizer(DEFAULT_MODEL).expect("bundled tokenizer is valid");
        let encoding = tokenizer
            .encode("hello world", false)
            .expect("text can be encoded");

        assert_eq!(encoding.get_ids(), &[24912, 2375]);
    }

    #[test]
    fn expands_globs_and_preserves_stdin() {
        let files = vec!["src/*.rs".to_owned(), "-".to_owned()];

        let (inputs, errors) = expand_inputs(&files);

        assert!(errors.is_empty());
        assert!(inputs.contains(&Input::File(PathBuf::from("src/main.rs"))));
        assert!(inputs.contains(&Input::File(PathBuf::from("src/lib.rs"))));
        assert!(inputs.contains(&Input::Stdin));
    }

    #[test]
    fn reports_invalid_and_unmatched_globs() {
        let files = vec!["[".to_owned(), "definitely-not-present-*.txt".to_owned()];

        let (inputs, errors) = expand_inputs(&files);

        assert!(inputs.is_empty());
        assert_eq!(errors.len(), 2);
        assert!(errors[0].contains("invalid glob"));
        assert!(errors[1].contains("no matches"));
    }

    #[test]
    fn stdin_is_read_once() {
        let results = process_inputs(
            &[Input::Stdin, Input::Stdin],
            &test_tokenizer(),
            Cursor::new("hello world"),
        );

        assert_eq!(results[0].1.as_ref().unwrap(), &10);
        assert_eq!(results[1].1.as_ref().unwrap(), &0);
    }

    #[test]
    fn file_errors_are_returned_with_their_input() {
        let results = process_inputs(
            &[Input::File(PathBuf::from("definitely-not-present.txt"))],
            &test_tokenizer(),
            Cursor::new([]),
        );

        assert_eq!(
            results[0].0,
            Input::File(PathBuf::from("definitely-not-present.txt"))
        );
        assert!(results[0].1.is_err());
    }
}
