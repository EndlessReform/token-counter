use clap::Parser;
use std::io::{self, Write};
use std::process::ExitCode;
use token_counter::{expand_inputs, process_inputs, CountResult, DEFAULT_MODEL};
use tokenizers::Tokenizer;

#[derive(Parser)]
#[command(author, version, about, long_about=None)]
struct Cli {
    #[arg(name = "FILE", default_value = "-")]
    files: Vec<String>,

    /// Hugging Face tokenizer model ID
    #[arg(short = 'm', long, default_value = DEFAULT_MODEL)]
    model: String,
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

    let tokenizer = match Tokenizer::from_pretrained(&cli.model, None) {
        Ok(tokenizer) => tokenizer,
        Err(error) => {
            eprintln!("tc: failed to load tokenizer '{}': {error}", cli.model);
            return ExitCode::FAILURE;
        }
    };

    let results = process_inputs(&inputs, &tokenizer, io::stdin().lock());
    match write_results(results, io::stdout().lock(), io::stderr().lock()) {
        Ok(processing_error) => had_error |= processing_error,
        Err(error) => {
            eprintln!("tc: {error}");
            had_error = true;
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
}
