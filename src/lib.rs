use glob::glob;
use ignore::WalkBuilder;
use rayon::prelude::*;
use std::collections::HashSet;
use std::fmt;
use std::fs::File;
use std::io::{self, BufReader, Read};
use std::path::{Path, PathBuf};
use tokenizers::{Encoding, Tokenizer};

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
    DiscoveredFile(PathBuf),
}

impl Input {
    pub fn is_stdin(&self) -> bool {
        matches!(self, Self::Stdin)
    }

    pub fn is_discovered(&self) -> bool {
        matches!(self, Self::DiscoveredFile(_))
    }

    fn path(&self) -> Option<&Path> {
        match self {
            Self::Stdin => None,
            Self::File(path) | Self::DiscoveredFile(path) => Some(path),
        }
    }
}

impl fmt::Display for Input {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Stdin => formatter.write_str("-"),
            Self::File(path) | Self::DiscoveredFile(path) => path.display().fmt(formatter),
        }
    }
}

pub type CountResult = (Input, io::Result<usize>);
pub type EncodingResult = (Input, io::Result<Encoding>);

pub fn encode_tokens(content: &str, tokenizer: &Tokenizer) -> io::Result<Encoding> {
    tokenizer.encode(content, false).map_err(io::Error::other)
}

pub fn count_tokens(content: &str, tokenizer: &Tokenizer) -> io::Result<usize> {
    encode_tokens(content, tokenizer).map(|encoding| encoding.len())
}

pub fn count_file(path: &Path, tokenizer: &Tokenizer) -> io::Result<usize> {
    count_reader(BufReader::new(File::open(path)?), tokenizer)
}

pub fn count_reader(mut reader: impl Read, tokenizer: &Tokenizer) -> io::Result<usize> {
    encode_reader(&mut reader, tokenizer).map(|encoding| encoding.len())
}

pub fn encode_file(path: &Path, tokenizer: &Tokenizer) -> io::Result<Encoding> {
    encode_reader(BufReader::new(File::open(path)?), tokenizer)
}

pub fn encode_reader(mut reader: impl Read, tokenizer: &Tokenizer) -> io::Result<Encoding> {
    let mut buffer = String::new();
    reader.read_to_string(&mut buffer)?;
    encode_tokens(&buffer, tokenizer)
}

fn has_glob_metacharacters(value: &str) -> bool {
    value.contains(['*', '?', '['])
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ExpansionOptions {
    pub recursive: bool,
    pub gitignore: bool,
}

fn walk_paths(
    path: &Path,
    respect_gitignore: bool,
    include: impl Fn(&ignore::DirEntry) -> bool,
) -> (Vec<PathBuf>, Vec<String>) {
    let mut builder = WalkBuilder::new(path);
    builder
        .hidden(false)
        .ignore(false)
        .git_ignore(respect_gitignore)
        .git_global(false)
        .git_exclude(false)
        .parents(respect_gitignore)
        .require_git(false);
    if respect_gitignore {
        builder.filter_entry(|entry| {
            !(entry.file_type().is_some_and(|kind| kind.is_dir()) && entry.file_name() == ".git")
        });
    }

    let mut paths = Vec::new();
    let mut errors = Vec::new();
    for entry in builder.build() {
        match entry {
            Ok(entry) if include(&entry) => {
                paths.push(entry.into_path());
            }
            Ok(_) => {}
            Err(error) => errors.push(error.to_string()),
        }
    }
    paths.sort();
    (paths, errors)
}

fn walk_directory(path: &Path, respect_gitignore: bool) -> (Vec<PathBuf>, Vec<String>) {
    walk_paths(path, respect_gitignore, |entry| entry.path().is_file())
}

fn gitignored_glob_matches(pattern: &str) -> (HashSet<PathBuf>, Vec<String>) {
    let root = pattern
        .split(['*', '?', '['])
        .next()
        .map(|prefix| {
            let path = PathBuf::from(prefix);
            if prefix.ends_with(std::path::MAIN_SEPARATOR) {
                path
            } else {
                path.parent().map(Path::to_path_buf).unwrap_or_default()
            }
        })
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| PathBuf::from("."));

    let (paths, errors) = walk_paths(&root, true, |_| true);
    (paths.into_iter().collect(), errors)
}

pub fn expand_inputs_with_options(
    files: &[String],
    options: ExpansionOptions,
) -> (Vec<Input>, Vec<String>) {
    let mut inputs = Vec::new();
    let mut errors = Vec::new();

    for value in files {
        if value == "-" {
            inputs.push(Input::Stdin);
        } else if has_glob_metacharacters(value) {
            let allowed = options.gitignore.then(|| {
                let (paths, walk_errors) = gitignored_glob_matches(value);
                errors.extend(
                    walk_errors
                        .into_iter()
                        .map(|error| format!("{value}: {error}")),
                );
                paths
            });
            match glob(value) {
                Ok(paths) => {
                    let mut matched = false;
                    let mut matched_paths = Vec::new();
                    for entry in paths {
                        match entry {
                            Ok(path) => {
                                matched = true;
                                if allowed.as_ref().is_none_or(|paths| paths.contains(&path)) {
                                    matched_paths.push(path);
                                }
                            }
                            Err(error) => errors.push(format!("{value}: {error}")),
                        }
                    }
                    matched_paths.sort();
                    for path in matched_paths {
                        if options.recursive && path.is_dir() {
                            let (walked, walk_errors) = walk_directory(&path, options.gitignore);
                            inputs.extend(walked.into_iter().map(Input::DiscoveredFile));
                            errors.extend(
                                walk_errors
                                    .into_iter()
                                    .map(|error| format!("{value}: {error}")),
                            );
                        } else {
                            inputs.push(Input::DiscoveredFile(path));
                        }
                    }
                    if !matched {
                        errors.push(format!("{value}: no matches"));
                    }
                }
                Err(error) => errors.push(format!("{value}: invalid glob: {error}")),
            }
        } else {
            let path = PathBuf::from(value);
            if options.recursive && path.is_dir() {
                let (walked, walk_errors) = walk_directory(&path, options.gitignore);
                inputs.extend(walked.into_iter().map(Input::DiscoveredFile));
                errors.extend(
                    walk_errors
                        .into_iter()
                        .map(|error| format!("{value}: {error}")),
                );
            } else {
                inputs.push(Input::File(path));
            }
        }
    }

    (inputs, errors)
}

pub fn expand_inputs(files: &[String]) -> (Vec<Input>, Vec<String>) {
    expand_inputs_with_options(files, ExpansionOptions::default())
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
    let mut results: Vec<_> = inputs
        .iter()
        .enumerate()
        .filter(|(_, input)| input.is_stdin())
        .map(|(index, input)| (index, (input.clone(), count_reader(&mut stdin, tokenizer))))
        .collect();

    let file_results: Vec<_> = inputs
        .par_iter()
        .enumerate()
        .filter_map(|(index, input)| match input {
            Input::Stdin => None,
            Input::File(path) | Input::DiscoveredFile(path) => {
                Some((index, (input.clone(), count_file(path, tokenizer))))
            }
        })
        .collect();
    results.extend(file_results);

    results.sort_unstable_by_key(|(index, _)| *index);
    results.into_iter().map(|(_, result)| result).collect()
}

/// Tokenizes all inputs, emitting stdin results first and file results as they complete.
///
/// File result order is intentionally nondeterministic. A repeated `-` observes
/// EOF after the first read, matching ordinary sequential stream behavior.
pub fn process_inputs_as_completed(
    inputs: &[Input],
    tokenizer: &Tokenizer,
    mut stdin: impl Read,
    emit: impl Fn(EncodingResult) + Sync,
) {
    for input in inputs {
        if input.is_stdin() {
            emit((input.clone(), encode_reader(&mut stdin, tokenizer)));
        }
    }

    inputs.par_iter().for_each(|input| {
        if let Some(path) = input.path() {
            emit((input.clone(), encode_file(path, tokenizer)));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Cursor;
    use tempfile::TempDir;
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
        assert!(inputs.contains(&Input::DiscoveredFile(PathBuf::from("src/main.rs"))));
        assert!(inputs.contains(&Input::DiscoveredFile(PathBuf::from("src/lib.rs"))));
        assert!(inputs.contains(&Input::Stdin));
    }

    #[test]
    fn glob_expansion_is_lexically_ordered() {
        let files = vec!["src/*.rs".to_owned()];

        let (inputs, errors) = expand_inputs(&files);

        assert!(errors.is_empty());
        assert_eq!(
            inputs,
            [
                Input::DiscoveredFile(PathBuf::from("src/lib.rs")),
                Input::DiscoveredFile(PathBuf::from("src/main.rs")),
            ]
        );
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
    fn recursively_expands_directories_in_lexical_order() {
        let directory = TempDir::new().unwrap();
        fs::create_dir(directory.path().join("nested")).unwrap();
        fs::write(directory.path().join("z.txt"), "z").unwrap();
        fs::write(directory.path().join("a.txt"), "a").unwrap();
        fs::write(directory.path().join("nested/m.txt"), "m").unwrap();

        let (inputs, errors) = expand_inputs_with_options(
            &[directory.path().display().to_string()],
            ExpansionOptions {
                recursive: true,
                gitignore: false,
            },
        );

        assert!(errors.is_empty());
        assert_eq!(
            inputs,
            [
                Input::DiscoveredFile(directory.path().join("a.txt")),
                Input::DiscoveredFile(directory.path().join("nested/m.txt")),
                Input::DiscoveredFile(directory.path().join("z.txt")),
            ]
        );
    }

    #[test]
    fn gitignore_filters_discovered_files_but_not_explicit_files() {
        let directory = TempDir::new().unwrap();
        let kept = directory.path().join("kept.txt");
        let ignored = directory.path().join("ignored.txt");
        fs::write(directory.path().join(".gitignore"), "ignored.txt\n").unwrap();
        fs::write(&kept, "kept").unwrap();
        fs::write(&ignored, "ignored").unwrap();

        let pattern = format!("{}/*.txt", directory.path().display());
        let (glob_inputs, errors) = expand_inputs_with_options(
            &[pattern],
            ExpansionOptions {
                recursive: false,
                gitignore: true,
            },
        );
        assert!(errors.is_empty());
        assert_eq!(glob_inputs, [Input::DiscoveredFile(kept)]);

        let (explicit_inputs, errors) = expand_inputs_with_options(
            &[ignored.display().to_string()],
            ExpansionOptions {
                recursive: false,
                gitignore: true,
            },
        );
        assert!(errors.is_empty());
        assert_eq!(explicit_inputs, [Input::File(ignored)]);
    }

    #[test]
    fn recursive_gitignore_uses_nested_precedence() {
        let directory = TempDir::new().unwrap();
        fs::create_dir(directory.path().join("nested")).unwrap();
        fs::write(directory.path().join(".gitignore"), "*.log\n").unwrap();
        fs::write(directory.path().join("root.log"), "ignored").unwrap();
        fs::write(directory.path().join("nested/.gitignore"), "!keep.log\n").unwrap();
        fs::write(directory.path().join("nested/keep.log"), "kept").unwrap();

        let (inputs, errors) = expand_inputs_with_options(
            &[directory.path().display().to_string()],
            ExpansionOptions {
                recursive: true,
                gitignore: true,
            },
        );

        assert!(errors.is_empty());
        assert!(!inputs.contains(&Input::DiscoveredFile(directory.path().join("root.log"))));
        assert!(inputs.contains(&Input::DiscoveredFile(
            directory.path().join("nested/keep.log")
        )));
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

    #[test]
    fn completion_processing_emits_every_input() {
        use std::sync::Mutex;

        let emitted = Mutex::new(Vec::new());
        process_inputs_as_completed(
            &[Input::Stdin, Input::Stdin],
            &test_tokenizer(),
            Cursor::new("hello world"),
            |result| emitted.lock().unwrap().push(result),
        );

        let emitted = emitted.into_inner().unwrap();
        assert_eq!(emitted.len(), 2);
        assert_eq!(emitted[0].1.as_ref().unwrap().len(), 10);
        assert_eq!(emitted[1].1.as_ref().unwrap().len(), 0);
    }
}
