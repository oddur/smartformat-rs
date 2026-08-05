//! Drives `tools/goldens` in case-file mode and reads its answers back.
//!
//! The contract, which `tools/goldens/Program.cs` implements:
//!
//! ```text
//! dotnet run --project tools/goldens -- --cases <path>
//! ```
//!
//! `<path>` holds `{"cases": [ {"id", "template", "args", "culture",
//! "settings"?}, … ]}` — the golden document's case objects with `expected`
//! left out. The harness renders each one and writes the golden document back
//! on stdout, every case now carrying `"expected": {"result": …}` or
//! `{"error": "<ExceptionTypeName>"}`. Cases are matched up by `id`, never by
//! position, and anything before the first `{` of the JSON (a build banner, a
//! restore line) is skipped.
//!
//! Two things go wrong often enough to be designed for. A template can take the
//! CLR down — a deep enough nesting is a stack overflow, which .NET makes
//! unhandleable on purpose — and then the process dies with no output at all
//! and no clue which case did it. And a pathological pattern can make it hang.
//! Both are handled the same way: the batch is halved and halved again until
//! the culprit is alone, which costs a logarithmic number of start-ups rather
//! than the one-per-case a naive answer would.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde_json::Value as Json;

use crate::case::{cases_document, Case, NetOutcome};

pub struct Harness {
    dotnet: String,
    project: PathBuf,
    repo_root: PathBuf,
    scratch: PathBuf,
    timeout: Duration,
    /// Rebuilding once per campaign rather than once per batch: after the first
    /// invocation the driver adds `--no-build`.
    built: std::cell::Cell<bool>,
    pub invocations: std::cell::Cell<u32>,
    /// The clock the harness pins, read back from its first response.
    pub now: std::cell::RefCell<String>,
}

/// What a batch cost, and what it produced.
pub struct BatchResult {
    pub outcomes: Vec<NetOutcome>,
}

/// Why a batch produced nothing.
#[derive(Debug)]
pub struct HarnessError {
    pub message: String,
    /// Whether carrying on would be pointless. A harness that renders its own
    /// hardcoded table instead of the case file has no `--cases` mode yet, and
    /// every batch after this one would fail the same way.
    pub fatal: bool,
}

impl std::fmt::Display for HarnessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

/// Why one invocation produced nothing.
enum Failure {
    /// The process died, hung, or wrote something unreadable — bisect.
    Died(String),
    /// The harness answered, but about something else entirely.
    NoCaseFileMode(String),
}

impl Harness {
    pub fn new(dotnet: String, repo_root: PathBuf, scratch: PathBuf, timeout: Duration) -> Self {
        let project = repo_root.join("tools").join("goldens");
        Self {
            dotnet,
            project,
            repo_root,
            scratch,
            timeout,
            built: std::cell::Cell::new(false),
            invocations: std::cell::Cell::new(0),
            now: std::cell::RefCell::new(crate::rustside::PINNED_NOW.to_string()),
        }
    }

    /// Renders a batch, bisecting around any case the harness cannot survive.
    /// The answers come back in the order the cases were given.
    pub fn run(&self, cases: &[Case]) -> Result<BatchResult, HarnessError> {
        let mut outcomes = vec![NetOutcome::Died; cases.len()];
        self.run_into(cases, &mut outcomes)?;
        Ok(BatchResult { outcomes })
    }

    fn run_into(&self, cases: &[Case], outcomes: &mut [NetOutcome]) -> Result<(), HarnessError> {
        if cases.is_empty() {
            return Ok(());
        }
        match self.invoke(cases) {
            Ok(answers) => {
                let mut unanswered = Vec::new();
                for (index, case) in cases.iter().enumerate() {
                    match answers.get(&case.id) {
                        Some(outcome) => outcomes[index] = outcome.clone(),
                        None => unanswered.push(index),
                    }
                }
                if unanswered.is_empty() {
                    return Ok(());
                }
                // A partial answer: re-run only what is missing. Splitting it
                // out rather than bisecting the whole batch keeps the cost
                // proportional to what actually went wrong.
                let missing: Vec<Case> = unanswered.iter().map(|i| cases[*i].clone()).collect();
                if missing.len() == cases.len() {
                    return self.bisect(cases, outcomes, "no answer for any case in the batch");
                }
                let mut missing_outcomes = vec![NetOutcome::Died; missing.len()];
                self.run_into(&missing, &mut missing_outcomes)?;
                for (slot, outcome) in unanswered.into_iter().zip(missing_outcomes) {
                    outcomes[slot] = outcome;
                }
                Ok(())
            }
            Err(Failure::Died(reason)) => self.bisect(cases, outcomes, &reason),
            Err(Failure::NoCaseFileMode(message)) => Err(HarnessError {
                message,
                fatal: true,
            }),
        }
    }

    /// Halves a batch the harness could not get through. A single case left
    /// standing is the culprit and is recorded as `Died`.
    fn bisect(
        &self,
        cases: &[Case],
        outcomes: &mut [NetOutcome],
        reason: &str,
    ) -> Result<(), HarnessError> {
        if cases.len() == 1 {
            eprintln!(
                "  the harness died on {} ({reason}); template {:?}",
                cases[0].id,
                cases[0].template()
            );
            outcomes[0] = NetOutcome::Died;
            return Ok(());
        }
        eprintln!(
            "  the harness died on a batch of {} ({reason}); bisecting",
            cases.len()
        );
        let middle = cases.len() / 2;
        let (left, right) = cases.split_at(middle);
        let (left_out, right_out) = outcomes.split_at_mut(middle);
        self.run_into(left, left_out)?;
        self.run_into(right, right_out)?;
        Ok(())
    }

    /// One `dotnet run`. `Err` means the process produced no usable document —
    /// it crashed, timed out, or wrote something that is not the contract.
    fn invoke(&self, cases: &[Case]) -> Result<HashMap<String, NetOutcome>, Failure> {
        // Process-wide rather than per-harness: two harnesses in one process —
        // which is what the driver's own tests run — must not write over each
        // other's case file.
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let serial = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = self
            .scratch
            .join(format!("cases-{}-{serial}.json", std::process::id()));
        self.invocations.set(self.invocations.get() + 1);
        let document = cases_document(cases);
        write_json(&path, &document)
            .map_err(|error| Failure::Died(format!("writing {path:?}: {error}")))?;

        let mut command = Command::new(&self.dotnet);
        command
            .current_dir(&self.repo_root)
            .arg("run")
            .arg("--project")
            .arg(&self.project)
            .arg("--verbosity")
            .arg("quiet");
        if self.built.get() {
            command.arg("--no-build");
        }
        command.arg("--").arg("--cases").arg(&path);

        let output = run_with_timeout(command, self.timeout);
        let _ = std::fs::remove_file(&path);
        let output = output.map_err(Failure::Died)?;

        if !output.status_ok {
            return Err(Failure::Died(format!(
                "dotnet exited with {} — stderr: {}",
                output.status,
                tail(&output.stderr)
            )));
        }
        self.built.set(true);

        let document = parse_leading_json(&output.stdout).ok_or_else(|| {
            Failure::Died(format!(
                "the harness wrote no JSON document — stdout: {} stderr: {}",
                tail(&output.stdout),
                tail(&output.stderr)
            ))
        })?;

        if let Some(now) = document.get("now").and_then(Json::as_str) {
            *self.now.borrow_mut() = now.to_string();
        }

        let answers = document
            .get("cases")
            .and_then(Json::as_array)
            .or_else(|| document.as_array())
            .ok_or_else(|| {
                Failure::Died("the harness document has no `cases` array".to_string())
            })?;

        let mut map = HashMap::new();
        for answer in answers {
            let Some(id) = answer.get("id").and_then(Json::as_str) else {
                continue;
            };
            let expected = answer.get("expected").unwrap_or(&Json::Null);
            let outcome = if let Some(result) = expected.get("result").and_then(Json::as_str) {
                NetOutcome::Result(result.to_string())
            } else if let Some(error) = expected.get("error").and_then(Json::as_str) {
                NetOutcome::Error(error.to_string())
            } else {
                continue;
            };
            map.insert(id.to_string(), outcome);
        }

        // A harness with no `--cases` mode ignores the file and renders its own
        // hardcoded table, so it answers — at length — about ids nobody asked
        // for. Bisecting that would cost one `dotnet run` per case and end with
        // every case blamed for a crash that never happened.
        if answers.len() > cases.len() && !cases.iter().any(|case| map.contains_key(&case.id)) {
            return Err(Failure::NoCaseFileMode(format!(
                "the harness answered about {} cases, none of them the {} it was given: \
                 `dotnet run --project tools/goldens -- --cases <file>` is not implemented yet. \
                 Run with --no-dotnet until it is.",
                answers.len(),
                cases.len()
            )));
        }

        Ok(map)
    }
}

fn write_json(path: &Path, document: &Json) -> std::io::Result<()> {
    let mut file = std::fs::File::create(path)?;
    file.write_all(serde_json::to_string(document)?.as_bytes())?;
    file.write_all(b"\n")
}

struct Output {
    status_ok: bool,
    status: String,
    stdout: String,
    stderr: String,
}

/// Runs a command, killing it if it outlives the timeout. Both pipes are drained
/// on their own threads: a harness that fills one while the driver waits on the
/// other would deadlock.
fn run_with_timeout(mut command: Command, timeout: Duration) -> Result<Output, String> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("could not start the harness: {error}"))?;

    let stdout = drain(child.stdout.take());
    let stderr = drain(child.stderr.take());

    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {}
            Err(error) => return Err(format!("waiting on the harness: {error}")),
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(20));
    };

    let stdout = stdout.recv().unwrap_or_default();
    let stderr = stderr.recv().unwrap_or_default();

    match status {
        Some(status) => Ok(Output {
            status_ok: status.success(),
            status: status.to_string(),
            stdout,
            stderr,
        }),
        None => Err(format!(
            "the harness did not finish within {}s and was killed",
            timeout.as_secs()
        )),
    }
}

fn drain<R: Read + Send + 'static>(pipe: Option<R>) -> mpsc::Receiver<String> {
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buffer = Vec::new();
        if let Some(mut pipe) = pipe {
            let _ = pipe.read_to_end(&mut buffer);
        }
        let _ = sender.send(String::from_utf8_lossy(&buffer).into_owned());
    });
    receiver
}

/// Reads the first complete JSON value out of `text`, ignoring anything before
/// it and anything after it. MSBuild is free to write a line of its own.
fn parse_leading_json(text: &str) -> Option<Json> {
    let start = text.find('{')?;
    let mut stream = serde_json::Deserializer::from_str(&text[start..]).into_iter::<Json>();
    stream.next()?.ok()
}

fn tail(text: &str) -> String {
    let trimmed = text.trim();
    let start = trimmed.len().saturating_sub(400);
    let start = (start..=trimmed.len())
        .find(|index| trimmed.is_char_boundary(*index))
        .unwrap_or(trimmed.len());
    format!("{:?}", &trimmed[start..])
}

/// Walks up from this crate to the repository root — the directory holding
/// `tools/goldens/goldens.csproj`.
pub fn find_repo_root() -> Option<PathBuf> {
    let mut directory = Path::new(env!("CARGO_MANIFEST_DIR"));
    loop {
        if directory.join("tools/goldens/goldens.csproj").is_file() {
            return Some(directory.to_path_buf());
        }
        directory = directory.parent()?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_is_found_past_a_build_banner() {
        let text = "  Determining projects to restore...\nfoo\n{\"cases\":[]}\ntrailing";
        let document = parse_leading_json(text).expect("a document");
        assert!(document.get("cases").is_some());
    }

    #[test]
    fn text_without_a_document_is_none() {
        assert!(parse_leading_json("error MSB1009: project file not found").is_none());
    }

    #[test]
    fn the_repo_root_is_the_one_holding_the_harness() {
        let root = find_repo_root().expect("the worktree holds tools/goldens");
        assert!(root.join("crates/smartformat/Cargo.toml").is_file());
    }

    #[test]
    fn a_tail_never_splits_a_character() {
        let text = "é".repeat(500);
        let _ = tail(&text);
    }
}
