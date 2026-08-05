//! A stand-in for `tools/goldens` in case-file mode, for the driver's own
//! tests.
//!
//! It speaks exactly the contract in `dotnetside`: it takes `--cases <path>`
//! anywhere in its arguments, ignores everything else (the driver puts `run
//! --project … --` in front), and writes the golden document back with an
//! `expected` on every case. What it renders is not SmartFormat — it echoes the
//! template — because what the tests need to exercise is the *driver*: batching,
//! matching answers up by id, and the two failures the real harness has that
//! nothing else can provoke on purpose.
//!
//! Three templates are magic:
//!
//! * one containing `BOOM` kills the process before it writes anything, which
//!   is what a stack overflow does to the CLR;
//! * one containing `HANG` sleeps past any sane timeout;
//! * one containing `SILENT` is left out of the answer, which is what a harness
//!   that stops half way through a batch leaves behind.

use std::io::Write;

use serde_json::{json, Value as Json};

fn main() {
    let arguments: Vec<String> = std::env::args().collect();
    let Some(path) = arguments
        .iter()
        .position(|argument| argument == "--cases")
        .and_then(|index| arguments.get(index + 1))
    else {
        eprintln!("fake-harness: no --cases argument");
        std::process::exit(2);
    };

    let text = std::fs::read_to_string(path).expect("the case file exists");
    let document: Json = serde_json::from_str(&text).expect("the case file is JSON");
    let cases = document["cases"].as_array().expect("a cases array");

    let mut answers = Vec::new();
    for case in cases {
        let template = case["template"].as_str().unwrap_or_default();
        if template.contains("BOOM") {
            // No output at all, and a status the driver reads as death.
            std::process::exit(134);
        }
        if template.contains("HANG") {
            std::thread::sleep(std::time::Duration::from_secs(3600));
        }
        if template.contains("SILENT") {
            continue;
        }
        let mut answer = case.clone();
        answer["expected"] = if template.contains("THROW") {
            json!({ "error": "FormattingException" })
        } else {
            json!({ "result": template })
        };
        answers.push(answer);
    }

    let document = json!({
        "smartformat_net_version": "3.6.1-fake",
        "default_culture": "InvariantCulture",
        "now": "2026-07-31T12:00:00.0000000",
        "cases": answers,
    });
    let mut stdout = std::io::stdout();
    stdout
        .write_all(
            serde_json::to_string(&document)
                .expect("serialisable")
                .as_bytes(),
        )
        .expect("stdout");
    stdout.write_all(b"\n").expect("stdout");
}
