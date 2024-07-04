use clap::Parser;
use glob::glob;
use rayon::prelude::*;
use std::fs::File;
use std::io::{self, BufReader, Read};
use std::path::Path;
use tokenizers::tokenizer::{Result as TokenizerResult, Tokenizer};

#[derive(Parser)]
#[command(author, version, about, long_about=None)]
struct Cli {
    #[arg(name = "FILE", default_value = "-")]
    files: Vec<String>,

    /// HuggingFace tokenizer model ID
    #[arg(long, default_value = "DWDMaiMai/tiktoken_cl100k_base")]
    model: String,
}

fn count_tokens(content: &str, tokenizer: &Tokenizer) -> TokenizerResult<usize> {
    let encoding = tokenizer.encode(content, false)?;
    Ok(encoding.get_tokens().len())
}

fn process_file(path: &Path, tokenizer: &Tokenizer) -> io::Result<usize> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut buffer = String::new();
    reader.read_to_string(&mut buffer)?;
    Ok(count_tokens(&buffer, tokenizer).map_err(|e| io::Error::new(io::ErrorKind::Other, e))?)
}

fn stdin_tokens(tokenizer: &Tokenizer) -> io::Result<usize> {
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;
    count_tokens(&input, tokenizer).map_err(|e| io::Error::new(io::ErrorKind::Other, e))
}

fn process_files(files: &[String], tokenizer: &Tokenizer) -> io::Result<usize> {
    let results: Vec<_> = files
        .par_iter()
        .flat_map(|pattern| {
            if pattern == "-" {
                vec![Ok((None, stdin_tokens(tokenizer)))]
            } else {
                glob(pattern)
                    .into_iter()
                    .flatten()
                    .map(|entry| {
                        entry.map(|path| (Some(path.clone()), process_file(&path, tokenizer)))
                    })
                    .collect::<Vec<_>>()
            }
        })
        .collect();

    let mut total_tokens = 0;
    for result in results {
        match result {
            Ok((path, Ok(count))) => match path {
                Some(path) => {
                    println!("{:8} {}", count, path.display());
                    total_tokens += count;
                }
                None => {
                    println!("{:8}", count);
                }
            },
            Err(e) => eprintln!("Error: {:?}", e),
            Ok((_, Err(e))) => eprintln!("Error processing file: {:?}", e),
        }
    }

    if files.len() > 1 {
        println!("{:8} total", total_tokens);
    }

    Ok(total_tokens)
}

fn main() {
    let cli = Cli::parse();

    // Initialize tokenizer.
    // TODO: Graceful error handling once we have arguments
    let tokenizer = Tokenizer::from_pretrained(&cli.model, None).unwrap();

    // Read input
    process_files(&cli.files, &tokenizer).unwrap();
}
