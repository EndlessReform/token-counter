use clap::Parser;
use glob::glob;
use std::fs::{self, File};
use std::io::{self, BufReader, Read};
use tokenizers::tokenizer::Tokenizer;

#[derive(Parser)]
#[command(author, version, about, long_about=None)]
struct Cli {
    #[arg(name = "FILE", default_value = "-")]
    files: Vec<String>,

    /// Recursively process directories
    #[arg(short, long)]
    recursive: bool,
}

fn process_files(files: &[String], tokenizer: &Tokenizer, recursive: bool) -> io::Result<usize> {
    let mut total_tokens = 0;
    let mut results = Vec::new();

    for pattern in files {
        if pattern == "-" {
            // Read from stdin
            let mut input = String::new();
            io::stdin().read_to_string(&mut input)?;
            let encoding = tokenizer
                .encode(input, false)
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
            let token_count = encoding.get_tokens().len();
            println!("\t{}", token_count);
        } else {
            for entry in glob(pattern).map_err(|e| io::Error::new(io::ErrorKind::Other, e))? {
                match entry {
                    Ok(path) => {
                        if path.is_dir() {
                            if recursive {
                                let dir_files: Vec<String> = fs::read_dir(&path)?
                                    .filter_map(|e| {
                                        e.ok().map(|d| d.path().to_string_lossy().into_owned())
                                    })
                                    .collect();
                                let dir_tokens = process_files(&dir_files, tokenizer, recursive)?;
                                total_tokens += dir_tokens;
                            } else {
                                results.push((
                                    0,
                                    format!("tc: {}: read: Is a directory", path.display()),
                                ));
                            }
                        } else {
                            let mut file = BufReader::new(File::open(&path)?);
                            let mut content = String::new();
                            file.read_to_string(&mut content)?;
                            let encoding = tokenizer
                                .encode(content, false)
                                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
                            let token_count = encoding.get_tokens().len();
                            results.push((token_count, path.to_string_lossy().into_owned()));
                            total_tokens += token_count;
                        }
                    }
                    Err(e) => eprint!("{:?}", e),
                }
            }
        }
    }
    // Find the maximum token count to determine column width
    let max_tokens = results.iter().map(|(count, _)| *count).max().unwrap_or(0);
    let width = max_tokens.to_string().len();

    // Print results
    for (count, name) in results {
        if name.contains("Is a directory") {
            println!("{}", name);
        } else {
            println!("{:width$} {}", count, name, width = width);
        }
    }

    if files.len() > 1 {
        println!("{:width$} total", total_tokens, width = width);
    }

    Ok(total_tokens)
}

fn main() {
    let cli = Cli::parse();

    // Initialize tokenizer.
    // TODO: Graceful error handling once we have arguments
    let tokenizer = Tokenizer::from_pretrained("BEE-spoke-data/cl100k_base", None).unwrap();

    // Read input
    process_files(&cli.files, &tokenizer, cli.recursive).unwrap();
}
