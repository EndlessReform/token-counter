use rayon::prelude::*;
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::hint::black_box;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};
use tokenizers::Tokenizer;

const EXTENSIONS: &[&str] = &["rs", "md", "toml", "txt"];
const EXPECTED_REVISION: &str = "3a612f88b805e14aef45bfa43e25a54abc6297fc";
const EXPECTED_FILES: usize = 134;
const EXPECTED_BYTES: u64 = 2_081_996;

type Counts = Vec<(PathBuf, usize)>;

fn git_output(root: &Path, arguments: &[&str]) -> Result<Vec<u8>, String> {
    let mut command = Command::new("git");
    command.args(["-C", root.to_str().ok_or("fixture path is not UTF-8")?]);
    command.args(arguments);
    let output = command
        .output()
        .map_err(|error| format!("failed to run git: {error}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned());
    }
    Ok(output.stdout)
}

fn tracked_fixture_files(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut files: Vec<_> = git_output(root, &["ls-files", "-z"])?
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .filter_map(|path| {
            let relative = PathBuf::from(String::from_utf8_lossy(path).into_owned());
            let included = relative
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| EXTENSIONS.contains(&extension));
            included.then(|| root.join(relative))
        })
        .collect();
    files.sort();
    Ok(files)
}

fn count_file(path: &Path, tokenizer: &Tokenizer) -> Result<usize, String> {
    let content = fs::read_to_string(path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    tokenizer
        .encode(content, false)
        .map(|encoding| encoding.len())
        .map_err(|error| format!("failed to tokenize {}: {error}", path.display()))
}

fn sequential(files: &[PathBuf], tokenizer: &Tokenizer) -> Result<Counts, String> {
    files
        .iter()
        .map(|path| count_file(path, tokenizer).map(|count| (path.clone(), count)))
        .collect()
}

fn parallel(files: &[PathBuf], tokenizer: &Tokenizer) -> Result<Counts, String> {
    files
        .par_iter()
        .map(|path| count_file(path, tokenizer).map(|count| (path.clone(), count)))
        .collect()
}

fn canonicalize(counts: &Counts) -> BTreeMap<&Path, usize> {
    counts
        .iter()
        .map(|(path, count)| (path.as_path(), *count))
        .collect()
}

fn timed(operation: impl FnOnce() -> Result<Counts, String>) -> Result<(Duration, Counts), String> {
    let start = Instant::now();
    let counts = operation()?;
    let elapsed = start.elapsed();
    black_box(&counts);
    Ok((elapsed, counts))
}

fn median(samples: &mut [Duration]) -> Duration {
    samples.sort_unstable();
    samples[samples.len() / 2]
}

fn change(sequential: Duration, parallel: Duration) -> f64 {
    (parallel.as_secs_f64() / sequential.as_secs_f64() - 1.0) * 100.0
}

fn verify(left: &Counts, right: &Counts) -> Result<usize, String> {
    let left = canonicalize(left);
    let right = canonicalize(right);
    if left != right {
        return Err("sequential and parallel path/count mappings differ".to_owned());
    }
    Ok(left.values().sum())
}

fn main() -> Result<(), String> {
    let mut args = env::args().skip(1);
    let Some(fixture_root) = args.next() else {
        eprintln!("benchmark skipped: pass fixture repository and tokenizer.json paths");
        return Ok(());
    };
    let fixture_root = PathBuf::from(fixture_root);
    let tokenizer_file = PathBuf::from(args.next().ok_or("missing tokenizer.json path")?);
    let iterations: usize = args
        .next()
        .unwrap_or_else(|| "15".to_owned())
        .parse()
        .map_err(|error| format!("invalid iteration count: {error}"))?;
    if iterations < 3 {
        return Err("iteration count must be at least 3".to_owned());
    }

    let files = tracked_fixture_files(&fixture_root)?;
    if files.is_empty() {
        return Err("fixture contains no selected files".to_owned());
    }
    let bytes: u64 = files
        .iter()
        .map(|path| fs::metadata(path).map(|metadata| metadata.len()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("failed to inspect fixture: {error}"))?
        .into_iter()
        .sum();
    let revision = String::from_utf8(git_output(&fixture_root, &["rev-parse", "HEAD"])?)
        .map_err(|error| format!("fixture revision is not UTF-8: {error}"))?;
    if revision.trim() != EXPECTED_REVISION
        || files.len() != EXPECTED_FILES
        || bytes != EXPECTED_BYTES
    {
        return Err(format!(
            "fixture mismatch: revision={}, files={}, bytes={bytes}",
            revision.trim(),
            files.len()
        ));
    }
    let largest = files
        .iter()
        .max_by_key(|path| {
            fs::metadata(path)
                .map(|metadata| metadata.len())
                .unwrap_or(0)
        })
        .expect("fixture is non-empty")
        .clone();
    let tokenizer = Tokenizer::from_file(&tokenizer_file)
        .map_err(|error| format!("failed to load tokenizer: {error}"))?;

    let warm_sequential = sequential(&files, &tokenizer)?;
    let warm_parallel = parallel(&files, &tokenizer)?;
    let total = verify(&warm_sequential, &warm_parallel)?;

    println!("fixture_files={}", files.len());
    println!("fixture_bytes={bytes}");
    println!("fixture_revision={}", revision.trim());
    println!("fixture_largest={}", largest.display());
    println!("rayon_threads={}", rayon::current_num_threads());
    println!("total_tokens={total}");
    println!("correctness=path_count_mapping_equal");
    println!("scope,sample,mode,nanoseconds");

    let mut multi_sequential = Vec::with_capacity(iterations);
    let mut multi_parallel = Vec::with_capacity(iterations);
    let single = [largest];
    let mut single_sequential = Vec::with_capacity(iterations);
    let mut single_parallel = Vec::with_capacity(iterations);

    for sample in 0..iterations {
        let parallel_first = sample % 2 == 1;
        let (sequential_result, parallel_result) = if parallel_first {
            let parallel_result = timed(|| parallel(&files, &tokenizer))?;
            let sequential_result = timed(|| sequential(&files, &tokenizer))?;
            (sequential_result, parallel_result)
        } else {
            let sequential_result = timed(|| sequential(&files, &tokenizer))?;
            let parallel_result = timed(|| parallel(&files, &tokenizer))?;
            (sequential_result, parallel_result)
        };
        verify(&sequential_result.1, &parallel_result.1)?;
        println!(
            "multi,{sample},sequential,{}",
            sequential_result.0.as_nanos()
        );
        println!("multi,{sample},parallel,{}", parallel_result.0.as_nanos());
        multi_sequential.push(sequential_result.0);
        multi_parallel.push(parallel_result.0);

        let (sequential_result, parallel_result) = if parallel_first {
            let parallel_result = timed(|| parallel(&single, &tokenizer))?;
            let sequential_result = timed(|| sequential(&single, &tokenizer))?;
            (sequential_result, parallel_result)
        } else {
            let sequential_result = timed(|| sequential(&single, &tokenizer))?;
            let parallel_result = timed(|| parallel(&single, &tokenizer))?;
            (sequential_result, parallel_result)
        };
        verify(&sequential_result.1, &parallel_result.1)?;
        println!(
            "single,{sample},sequential,{}",
            sequential_result.0.as_nanos()
        );
        println!("single,{sample},parallel,{}", parallel_result.0.as_nanos());
        single_sequential.push(sequential_result.0);
        single_parallel.push(parallel_result.0);
    }

    let multi_sequential = median(&mut multi_sequential);
    let multi_parallel = median(&mut multi_parallel);
    let single_sequential = median(&mut single_sequential);
    let single_parallel = median(&mut single_parallel);
    println!("multi_median_sequential_ns={}", multi_sequential.as_nanos());
    println!("multi_median_parallel_ns={}", multi_parallel.as_nanos());
    println!(
        "multi_parallel_change_percent={:.2}",
        change(multi_sequential, multi_parallel)
    );
    println!(
        "single_median_sequential_ns={}",
        single_sequential.as_nanos()
    );
    println!("single_median_parallel_ns={}", single_parallel.as_nanos());
    println!(
        "single_parallel_change_percent={:.2}",
        change(single_sequential, single_parallel)
    );

    Ok(())
}
