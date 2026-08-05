//! The `difffuzz` command line: parses the options, builds the harness, runs a
//! campaign and writes the report. Everything it does lives in the library
//! beside it.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use difffuzz::campaign::{self, Options};
use difffuzz::dotnetside::{self, Harness};
use difffuzz::report;

const USAGE: &str = "\
difffuzz — differential fuzzer for smartformat-rs against SmartFormat.NET

    cargo run --manifest-path tools/difffuzz/Cargo.toml -- [options]

Options
    --seed N            campaign seed; the same seed reproduces the run exactly
                        (default: drawn from the clock and printed)
    --count N           how many cases to generate            (default: 200)
    --index N           run only case N of the campaign, for reproducing one
                        finding without re-running everything before it
    --cases PATH        run the cases in PATH instead of generating any. The
                        file is a `{\"cases\": [...]}` document (or a bare
                        array) of case objects — the corpus beside this tool,
                        the `case` objects of an earlier report, or a template
                        typed out by hand while triaging.
    --batch-size N      cases per `dotnet run`                (default: 100)
    --shrink-batch N    candidate reductions per shrinking round  (default: 120)
    --shrink-rounds N   how many rounds one finding is shrunk for (default: 12)
    --report PATH       where the report goes  (default: difffuzz-report.json)
    --no-dotnet         generate and render on the Rust side only; nothing is
                        compared. Use it to develop the generator, or when the
                        harness has no `--cases` mode yet.
    --no-confirm-alone  do not re-render a disagreeing case on its own. The
                        confirmation costs one `dotnet run` per disagreement
                        and is what tells a rendering difference apart from
                        .NET state left behind by an earlier case.
    --dotnet PATH       the dotnet executable                  (default: dotnet)
    --repo PATH         the repository root holding tools/goldens
    --timeout SECONDS   how long one `dotnet run` may take     (default: 600)
    --help              this text

Exit code is 1 when the campaign found a new disagreement, 0 otherwise.
";

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let settings = match parse(&arguments) {
        Ok(Some(settings)) => settings,
        Ok(None) => {
            print!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Err(error) => {
            eprintln!("difffuzz: {error}\n");
            eprint!("{USAGE}");
            return ExitCode::from(2);
        }
    };

    let repo_root = match settings
        .repo_root
        .clone()
        .or_else(dotnetside::find_repo_root)
    {
        Some(root) => root,
        None => {
            eprintln!("difffuzz: could not find the repository root; pass --repo PATH");
            return ExitCode::from(2);
        }
    };

    let harness = if settings.use_dotnet {
        let scratch = std::env::temp_dir().join("difffuzz");
        if let Err(error) = std::fs::create_dir_all(&scratch) {
            eprintln!("difffuzz: could not make a scratch directory: {error}");
            return ExitCode::from(2);
        }
        Some(Harness::new(
            settings.dotnet.clone(),
            repo_root,
            scratch,
            settings.timeout,
        ))
    } else {
        eprintln!("difffuzz: --no-dotnet — generating and rendering on the Rust side only");
        None
    };

    let supplied = match &settings.cases {
        Some(path) => match read_cases(path) {
            Ok(cases) => Some(cases),
            Err(error) => {
                eprintln!("difffuzz: {error}");
                return ExitCode::from(2);
            }
        },
        None => None,
    };

    match (&supplied, settings.options.only) {
        (Some(cases), _) => println!(
            "difffuzz: {} cases from {}",
            cases.len(),
            settings.cases.as_ref().expect("a path was read").display()
        ),
        (None, Some(index)) => println!(
            "difffuzz: seed {} — case {index} only",
            settings.options.seed
        ),
        (None, None) => println!(
            "difffuzz: seed {} — {} cases",
            settings.options.seed, settings.options.count
        ),
    }

    let (summary, findings) = match &supplied {
        Some(cases) => campaign::run_cases(cases, &settings.options, harness.as_ref()),
        None => campaign::run(&settings.options, harness.as_ref()),
    };
    let document = report::document(&settings.options, &summary, &findings);
    if let Err(error) = report::write(&settings.report, &document) {
        eprintln!("difffuzz: could not write {:?}: {error}", settings.report);
        return ExitCode::from(2);
    }
    report::print_summary(&summary, &findings, &settings.report);

    if summary.new > 0 {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

/// Reads a corpus or triage file into cases.
fn read_cases(path: &PathBuf) -> Result<Vec<difffuzz::case::Case>, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| format!("could not read {}: {error}", path.display()))?;
    let document: serde_json::Value = serde_json::from_str(&text)
        .map_err(|error| format!("{} is not JSON: {error}", path.display()))?;
    difffuzz::case::read_cases(&document).map_err(|error| format!("{}: {error}", path.display()))
}

struct Settings {
    options: Options,
    /// Cases to run instead of generating any.
    cases: Option<PathBuf>,
    report: PathBuf,
    use_dotnet: bool,
    dotnet: String,
    repo_root: Option<PathBuf>,
    timeout: Duration,
}

/// `Ok(None)` is `--help`.
fn parse(arguments: &[String]) -> Result<Option<Settings>, String> {
    let mut settings = Settings {
        options: Options {
            seed: seed_from_the_clock(),
            count: 200,
            batch_size: 100,
            shrink_batch: 120,
            shrink_rounds: 12,
            confirm_alone: true,
            only: None,
        },
        cases: None,
        report: PathBuf::from("difffuzz-report.json"),
        use_dotnet: true,
        dotnet: "dotnet".to_string(),
        repo_root: None,
        timeout: Duration::from_secs(600),
    };

    let mut rest = arguments.iter();
    while let Some(argument) = rest.next() {
        let mut value = || {
            rest.next()
                .cloned()
                .ok_or_else(|| format!("{argument} needs a value"))
        };
        match argument.as_str() {
            "--help" | "-h" => return Ok(None),
            "--seed" => settings.options.seed = number(&value()?, argument)?,
            "--count" => settings.options.count = number(&value()?, argument)? as usize,
            "--index" => settings.options.only = Some(number(&value()?, argument)? as usize),
            "--batch-size" => settings.options.batch_size = number(&value()?, argument)? as usize,
            "--shrink-batch" => {
                settings.options.shrink_batch = number(&value()?, argument)? as usize
            }
            "--shrink-rounds" => {
                settings.options.shrink_rounds = number(&value()?, argument)? as usize;
            }
            "--cases" => settings.cases = Some(PathBuf::from(value()?)),
            "--report" => settings.report = PathBuf::from(value()?),
            "--no-dotnet" => settings.use_dotnet = false,
            "--no-confirm-alone" => settings.options.confirm_alone = false,
            "--dotnet" => settings.dotnet = value()?,
            "--repo" => settings.repo_root = Some(PathBuf::from(value()?)),
            "--timeout" => settings.timeout = Duration::from_secs(number(&value()?, argument)?),
            other => return Err(format!("unknown option {other}")),
        }
    }
    Ok(Some(settings))
}

fn number(text: &str, option: &str) -> Result<u64, String> {
    text.parse()
        .map_err(|_| format!("{option} wants a number, not {text:?}"))
}

/// A seed for a run that did not ask for one. It is printed before anything
/// else happens, because it is the only way back to the run.
fn seed_from_the_clock() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(1, |since| since.as_nanos() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_are_a_usable_campaign() {
        let settings = parse(&[]).expect("parses").expect("not help");
        assert_eq!(settings.options.count, 200);
        assert!(settings.use_dotnet);
        assert!(settings.options.confirm_alone);
    }

    #[test]
    fn options_are_read() {
        let arguments: Vec<String> = ["--seed", "9", "--count", "7", "--no-dotnet", "--index", "3"]
            .iter()
            .map(|text| (*text).to_string())
            .collect();
        let settings = parse(&arguments).expect("parses").expect("not help");
        assert_eq!(settings.options.seed, 9);
        assert_eq!(settings.options.count, 7);
        assert_eq!(settings.options.only, Some(3));
        assert!(!settings.use_dotnet);
    }

    #[test]
    fn an_unknown_option_is_an_error() {
        let arguments = vec!["--nope".to_string()];
        assert!(parse(&arguments).is_err());
    }

    #[test]
    fn a_missing_value_is_an_error() {
        let arguments = vec!["--seed".to_string()];
        assert!(parse(&arguments).is_err());
    }

    #[test]
    fn help_is_not_a_campaign() {
        assert!(parse(&["--help".to_string()]).expect("parses").is_none());
    }
}
