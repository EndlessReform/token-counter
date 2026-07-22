use clap::Parser;
use serde::Serialize;
use std::io::{self, Write};
use std::process::ExitCode;
use std::str::FromStr;
use std::sync::Mutex;
use token_counter::{
    expand_inputs_with_options, load_tokenizer, process_inputs, process_inputs_as_completed,
    CountResult, EncodingResult, ExpansionOptions, DEFAULT_MODEL,
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

    /// Recursively process files beneath directory operands
    #[arg(short = 'r', long)]
    recursive: bool,

    /// Print counts without filenames or a total
    #[arg(short = 'c', long, conflicts_with = "jsonl")]
    count_only: bool,

    /// Skip gitignored files discovered by globs or recursive walking
    #[arg(long)]
    gitignore: bool,

    /// Estimate cost at this USD price per million tokens
    #[arg(long, value_name = "PRICE")]
    cost_per_mtok: Option<PricePerMillion>,

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
    cost_usd: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tokens: Option<&'a [String]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    token_ids: Option<&'a [u32]>,
}

#[derive(Clone, Copy, Debug)]
struct PricePerMillion(f64);

impl PricePerMillion {
    fn cost(self, count: usize) -> f64 {
        count as f64 * self.0 / 1_000_000.0
    }
}

impl FromStr for PricePerMillion {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let price = value.parse::<f64>().map_err(|_| "price must be a number")?;
        if price.is_finite() && price >= 0.0 {
            Ok(Self(price))
        } else {
            Err("price must be a finite, non-negative number")
        }
    }
}

fn write_input_error(
    input: &token_counter::Input,
    error: &io::Error,
    mut stderr: impl Write,
) -> io::Result<bool> {
    if input.is_discovered() && error.kind() == io::ErrorKind::InvalidData {
        writeln!(stderr, "tc: {input}: {error}; skipping non-UTF-8 file")?;
        Ok(false)
    } else {
        writeln!(stderr, "tc: {input}: {error}")?;
        Ok(true)
    }
}

fn format_cost(cost: f64) -> String {
    let mut formatted = format!("{cost:.12}");
    let trimmed_length = formatted.trim_end_matches('0').len();
    let minimum_length = formatted.len() - 6;
    formatted.truncate(trimmed_length.max(minimum_length));
    formatted
}

fn write_results(
    results: Vec<CountResult>,
    count_only: bool,
    price_per_mtok: Option<PricePerMillion>,
    mut stdout: impl Write,
    mut stderr: impl Write,
) -> io::Result<bool> {
    let show_names = results.len() > 1;
    let mut had_error = false;
    let mut total_tokens = 0;

    for (input, result) in results {
        match result {
            Ok(count) => {
                total_tokens += count;
                let cost = price_per_mtok
                    .map(|price| format!(" ${}", format_cost(price.cost(count))))
                    .unwrap_or_default();
                if count_only || (input.is_stdin() && !show_names) {
                    writeln!(stdout, "{count:8}{cost}")?;
                } else {
                    writeln!(stdout, "{count:8}{cost} {input}")?;
                }
            }
            Err(error) => {
                had_error |= write_input_error(&input, &error, &mut stderr)?;
            }
        }
    }

    if show_names && !count_only {
        let cost = price_per_mtok
            .map(|price| format!(" ${}", format_cost(price.cost(total_tokens))))
            .unwrap_or_default();
        writeln!(stdout, "{total_tokens:8}{cost} total")?;
    }

    Ok(had_error)
}

fn write_jsonl_result(
    (input, result): EncodingResult,
    show_tokens: bool,
    show_token_ids: bool,
    price_per_mtok: Option<PricePerMillion>,
    mut stdout: impl Write,
    stderr: impl Write,
) -> io::Result<bool> {
    match result {
        Ok(encoding) => {
            let path = input.to_string();
            serde_json::to_writer(
                &mut stdout,
                &JsonlRecord {
                    path: &path,
                    n_tokens: encoding.len(),
                    cost_usd: price_per_mtok.map(|price| price.cost(encoding.len())),
                    tokens: show_tokens.then(|| encoding.get_tokens()),
                    token_ids: show_token_ids.then(|| encoding.get_ids()),
                },
            )?;
            writeln!(stdout)?;
            stdout.flush()?;
            Ok(false)
        }
        Err(error) => write_input_error(&input, &error, stderr),
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let (inputs, expansion_errors) = expand_inputs_with_options(
        &cli.files,
        ExpansionOptions {
            recursive: cli.recursive,
            gitignore: cli.gitignore,
        },
    );
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
        let state = Mutex::new(Ok(false));
        process_inputs_as_completed(&inputs, &tokenizer, io::stdin().lock(), |result| {
            let mut state = state.lock().expect("JSONL output state is not poisoned");
            if state.is_err() {
                return;
            }
            match write_jsonl_result(
                result,
                cli.show_tokens,
                cli.show_token_ids,
                cli.cost_per_mtok,
                io::stdout().lock(),
                io::stderr().lock(),
            ) {
                Ok(processing_error) => {
                    if let Ok(had_error) = state.as_mut() {
                        *had_error |= processing_error;
                    }
                }
                Err(error) => *state = Err(error),
            }
        });
        match state
            .into_inner()
            .expect("JSONL output state is not poisoned")
        {
            Ok(processing_error) => had_error |= processing_error,
            Err(error) => {
                eprintln!("tc: {error}");
                had_error = true;
            }
        }
    } else {
        let results = process_inputs(&inputs, &tokenizer, io::stdin().lock());
        match write_results(
            results,
            cli.count_only,
            cli.cost_per_mtok,
            io::stdout().lock(),
            io::stderr().lock(),
        ) {
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
    fn count_only_is_normal_output_only() {
        assert!(Cli::try_parse_from(["tc", "-c", "--jsonl", "input.txt"]).is_err());
        let cli = Cli::try_parse_from(["tc", "-c", "input.txt"]).expect("valid arguments");
        assert!(cli.count_only);
    }

    #[test]
    fn rejects_invalid_costs() {
        assert!(Cli::try_parse_from(["tc", "--cost-per-mtok", "-1"]).is_err());
        assert!(Cli::try_parse_from(["tc", "--cost-per-mtok", "NaN"]).is_err());
        assert!(Cli::try_parse_from(["tc", "--cost-per-mtok", "2.50"]).is_ok());
    }

    #[test]
    fn formats_stdin_and_totals() {
        let results = vec![
            (Input::Stdin, Ok(10)),
            (Input::File(PathBuf::from("input.txt")), Ok(5)),
        ];
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let had_error = write_results(results, false, None, &mut stdout, &mut stderr).unwrap();

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

        let had_error = write_results(results, false, None, &mut stdout, &mut stderr).unwrap();

        assert!(had_error);
        assert!(stdout.is_empty());
        assert_eq!(
            String::from_utf8(stderr).unwrap(),
            "tc: missing.txt: not found\n"
        );
    }

    #[test]
    fn count_only_suppresses_names_and_total() {
        let results = vec![
            (Input::File(PathBuf::from("one.txt")), Ok(10)),
            (Input::File(PathBuf::from("two.txt")), Ok(5)),
        ];
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let had_error = write_results(results, true, None, &mut stdout, &mut stderr).unwrap();

        assert!(!had_error);
        assert_eq!(String::from_utf8(stdout).unwrap(), "      10\n       5\n");
        assert!(stderr.is_empty());
    }

    #[test]
    fn formats_costs_for_human_output() {
        let results = vec![(Input::File(PathBuf::from("input.txt")), Ok(2))];
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let had_error = write_results(
            results,
            false,
            Some(PricePerMillion(2.5)),
            &mut stdout,
            &mut stderr,
        )
        .unwrap();

        assert!(!had_error);
        assert_eq!(
            String::from_utf8(stdout).unwrap(),
            "       2 $0.000005 input.txt\n"
        );
        assert!(stderr.is_empty());
    }

    #[test]
    fn discovered_non_utf8_files_are_skipped_without_failure() {
        let results = vec![(
            Input::DiscoveredFile(PathBuf::from("binary.dat")),
            Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "stream did not contain valid UTF-8",
            )),
        )];
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let had_error = write_results(results, false, None, &mut stdout, &mut stderr).unwrap();

        assert!(!had_error);
        assert!(stdout.is_empty());
        assert!(String::from_utf8(stderr)
            .unwrap()
            .contains("skipping non-UTF-8 file"));
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

        let had_error =
            write_jsonl_result(result, false, false, None, &mut stdout, &mut stderr).unwrap();

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

        let had_error =
            write_jsonl_result(result, true, true, None, &mut stdout, &mut stderr).unwrap();

        assert!(!had_error);
        assert_eq!(
            String::from_utf8(stdout).unwrap(),
            "{\"path\":\"input.txt\",\"n_tokens\":2,\"tokens\":[\"hello\",\"Ġworld\"],\"token_ids\":[24912,2375]}\n"
        );
        assert!(stderr.is_empty());
    }

    #[test]
    fn jsonl_can_include_cost() {
        let encoding = load_tokenizer(DEFAULT_MODEL)
            .unwrap()
            .encode("hello world", false)
            .unwrap();
        let result = (Input::File(PathBuf::from("input.txt")), Ok(encoding));
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let had_error = write_jsonl_result(
            result,
            false,
            false,
            Some(PricePerMillion(2.5)),
            &mut stdout,
            &mut stderr,
        )
        .unwrap();

        assert!(!had_error);
        assert_eq!(
            String::from_utf8(stdout).unwrap(),
            "{\"path\":\"input.txt\",\"n_tokens\":2,\"cost_usd\":5e-6}\n"
        );
        assert!(stderr.is_empty());
    }
}
