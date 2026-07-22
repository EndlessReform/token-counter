use clap::Parser;
use serde::Serialize;
use std::io::{self, Write};
use std::process::ExitCode;
use std::sync::Mutex;
use token_counter::{
    expand_inputs, load_tokenizer, process_inputs, process_inputs_as_completed, CountResult,
    EncodingResult, DEFAULT_MODEL,
};

#[derive(Parser)]
#[command(author, version, about, long_about=None)]
struct Cli {
    #[arg(name = "FILE", default_value = "-")]
    files: Vec<String>,

    /// Hugging Face tokenizer model ID
    #[arg(short = 'm', long, default_value = DEFAULT_MODEL)]
    model: String,

    /// Emit one JSON object per input as processing completes
    #[arg(long)]
    jsonl: bool,

    /// Include tokenizer vocabulary strings in JSONL output
    #[arg(long, requires = "jsonl")]
    show_tokens: bool,

    /// Include numeric token IDs in JSONL output
    #[arg(long, requires = "jsonl")]
    show_token_ids: bool,
}

#[derive(Serialize)]
struct JsonlRecord<'a> {
    path: &'a str,
    n_tokens: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    tokens: Option<&'a [String]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    token_ids: Option<&'a [u32]>,
}

fn write_results(
    results: Vec<CountResult>,
    mut stdout: impl Write,
    mut stderr: impl Write,
) -> io::Result<bool> {
    let show_names = results.len() > 1;
    let mut had_error = false;
    let mut total_tokens = 0;

    for (input, result) in results {
        let name = input.label();
        match result {
            Ok(count) => {
                total_tokens += count;
                if input.is_stdin() && !show_names {
                    writeln!(stdout, "{count:8}")?;
                } else {
                    writeln!(stdout, "{count:8} {name}")?;
                }
            }
            Err(error) => {
                had_error = true;
                writeln!(stderr, "tc: {name}: {error}")?;
            }
        }
    }

    if show_names {
        writeln!(stdout, "{total_tokens:8} total")?;
    }

    Ok(had_error)
}

fn write_jsonl_result(
    (input, result): EncodingResult,
    show_tokens: bool,
    show_token_ids: bool,
    mut stdout: impl Write,
    mut stderr: impl Write,
) -> io::Result<bool> {
    let name = input.label();
    match result {
        Ok(encoding) => {
            serde_json::to_writer(
                &mut stdout,
                &JsonlRecord {
                    path: &name,
                    n_tokens: encoding.len(),
                    tokens: show_tokens.then(|| encoding.get_tokens()),
                    token_ids: show_token_ids.then(|| encoding.get_ids()),
                },
            )?;
            writeln!(stdout)?;
            stdout.flush()?;
            Ok(false)
        }
        Err(error) => {
            writeln!(stderr, "tc: {name}: {error}")?;
            Ok(true)
        }
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let (inputs, expansion_errors) = expand_inputs(&cli.files);
    let mut had_error = !expansion_errors.is_empty();
    for error in expansion_errors {
        eprintln!("tc: {error}");
    }

    if inputs.is_empty() {
        return if had_error {
            ExitCode::FAILURE
        } else {
            ExitCode::SUCCESS
        };
    }

    let tokenizer = match load_tokenizer(&cli.model) {
        Ok(tokenizer) => tokenizer,
        Err(error) => {
            eprintln!("tc: failed to load tokenizer '{}': {error}", cli.model);
            return ExitCode::FAILURE;
        }
    };

    if cli.jsonl {
        let state = Mutex::new((false, None));
        process_inputs_as_completed(&inputs, &tokenizer, io::stdin().lock(), |result| {
            let mut state = state.lock().expect("JSONL output state is not poisoned");
            if state.1.is_some() {
                return;
            }
            match write_jsonl_result(
                result,
                cli.show_tokens,
                cli.show_token_ids,
                io::stdout().lock(),
                io::stderr().lock(),
            ) {
                Ok(processing_error) => state.0 |= processing_error,
                Err(error) => state.1 = Some(error),
            }
        });
        let (processing_error, output_error) = state
            .into_inner()
            .expect("JSONL output state is not poisoned");
        had_error |= processing_error;
        if let Some(error) = output_error {
            eprintln!("tc: {error}");
            had_error = true;
        }
    } else {
        let results = process_inputs(&inputs, &tokenizer, io::stdin().lock());
        match write_results(results, io::stdout().lock(), io::stderr().lock()) {
            Ok(processing_error) => had_error |= processing_error,
            Err(error) => {
                eprintln!("tc: {error}");
                had_error = true;
            }
        }
    }

    if had_error {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;
    use std::io;
    use std::path::PathBuf;
    use token_counter::Input;

    #[test]
    fn cli_contract_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn model_has_documented_short_option() {
        let cli = Cli::try_parse_from(["tc", "-m", "bert-base-uncased", "input.txt"])
            .expect("valid arguments");

        assert_eq!(cli.model, "bert-base-uncased");
        assert_eq!(cli.files, ["input.txt"]);
    }

    #[test]
    fn default_model_uses_o200k_tokenizer() {
        let cli = Cli::try_parse_from(["tc"]).expect("valid arguments");

        assert_eq!(cli.model, DEFAULT_MODEL);
        assert_eq!(cli.files, ["-"]);
    }

    #[test]
    fn jsonl_is_opt_in() {
        let cli = Cli::try_parse_from(["tc", "--jsonl", "input.txt"]).expect("valid arguments");

        assert!(cli.jsonl);
        assert_eq!(cli.files, ["input.txt"]);
    }

    #[test]
    fn token_details_require_jsonl() {
        assert!(Cli::try_parse_from(["tc", "--show-tokens", "input.txt"]).is_err());
        assert!(Cli::try_parse_from(["tc", "--show-token-ids", "input.txt"]).is_err());

        let cli = Cli::try_parse_from([
            "tc",
            "--jsonl",
            "--show-tokens",
            "--show-token-ids",
            "input.txt",
        ])
        .expect("valid arguments");
        assert!(cli.show_tokens);
        assert!(cli.show_token_ids);
    }

    #[test]
    fn formats_stdin_and_totals() {
        let results = vec![
            (Input::Stdin, Ok(10)),
            (Input::File(PathBuf::from("input.txt")), Ok(5)),
        ];
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let had_error = write_results(results, &mut stdout, &mut stderr).unwrap();

        assert!(!had_error);
        assert_eq!(
            String::from_utf8(stdout).unwrap(),
            "      10 -\n       5 input.txt\n      15 total\n"
        );
        assert!(stderr.is_empty());
    }

    #[test]
    fn reports_processing_errors() {
        let results = vec![(
            Input::File(PathBuf::from("missing.txt")),
            Err(io::Error::new(io::ErrorKind::NotFound, "not found")),
        )];
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let had_error = write_results(results, &mut stdout, &mut stderr).unwrap();

        assert!(had_error);
        assert!(stdout.is_empty());
        assert_eq!(
            String::from_utf8(stderr).unwrap(),
            "tc: missing.txt: not found\n"
        );
    }

    #[test]
    fn formats_uniform_jsonl_without_total() {
        let encoding = load_tokenizer(DEFAULT_MODEL)
            .unwrap()
            .encode("hello world", false)
            .unwrap();
        let result = (Input::File(PathBuf::from("input.txt")), Ok(encoding));
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let had_error = write_jsonl_result(result, false, false, &mut stdout, &mut stderr).unwrap();

        assert!(!had_error);
        assert_eq!(
            String::from_utf8(stdout).unwrap(),
            "{\"path\":\"input.txt\",\"n_tokens\":2}\n"
        );
        assert!(stderr.is_empty());
    }

    #[test]
    fn jsonl_can_include_token_strings_and_ids() {
        let encoding = load_tokenizer(DEFAULT_MODEL)
            .unwrap()
            .encode("hello world", false)
            .unwrap();
        let result = (Input::File(PathBuf::from("input.txt")), Ok(encoding));
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let had_error = write_jsonl_result(result, true, true, &mut stdout, &mut stderr).unwrap();

        assert!(!had_error);
        assert_eq!(
            String::from_utf8(stdout).unwrap(),
            "{\"path\":\"input.txt\",\"n_tokens\":2,\"tokens\":[\"hello\",\"Ġworld\"],\"token_ids\":[24912,2375]}\n"
        );
        assert!(stderr.is_empty());
    }
}
