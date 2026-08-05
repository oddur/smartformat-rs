//! The disagreement report.
//!
//! One JSON document, so a finding can be read by a person and re-fed to the
//! tool by a script. Each entry carries the four inputs — template, args,
//! culture, settings — both engines' answers, the class the campaign put it in,
//! and the minimal case the shrinker got to. The `case` object of an entry is
//! exactly the shape `goldens/m1.json` uses, so a triaged finding can be lifted
//! straight into the golden table.

use std::io::Write;
use std::path::Path;

use serde_json::{json, Map, Value as Json};

use crate::campaign::{Judged, Options, Summary};
use crate::classify::Class;

pub fn document(options: &Options, summary: &Summary, findings: &[Judged]) -> Json {
    let mut entries: Vec<&Judged> = findings.iter().collect();
    entries.sort_by_key(|finding| finding.verdict.class());

    json!({
        "tool": "difffuzz",
        "reproduce": reproduce(options),
        "seed": options.seed,
        "count": summary.cases,
        "summary": summary_json(summary),
        "disagreements": entries.iter().map(|finding| entry(finding)).collect::<Vec<_>>(),
    })
}

/// The command that runs this campaign again.
fn reproduce(options: &Options) -> String {
    let head = "cargo run --manifest-path tools/difffuzz/Cargo.toml --";
    match options.only {
        Some(index) => format!("{head} --seed {} --index {index}", options.seed),
        None => format!("{head} --seed {} --count {}", options.seed, options.count),
    }
}

fn summary_json(summary: &Summary) -> Json {
    json!({
        "cases": summary.cases,
        "agreements": summary.agreements,
        "disagreements": {
            "new": summary.new,
            "known_divergence": summary.known,
            "harness_died": summary.harness_died,
            "order_dependent": summary.order_dependent,
        },
        "rust_panics": summary.rust_panics,
        "dotnet_invocations": summary.dotnet_invocations,
        "seconds": summary.seconds,
    })
}

fn entry(finding: &Judged) -> Json {
    let class = finding
        .verdict
        .class()
        .map_or("agreement", |class| class.label());
    let mut node = Map::new();
    node.insert("id".into(), json!(finding.case.id));
    node.insert("class".into(), json!(class));
    if let Some(known) = finding.verdict.known() {
        node.insert("rule".into(), json!(known.rule));
        node.insert("reason".into(), json!(known.reason));
    }
    node.insert("template".into(), json!(finding.case.template()));
    node.insert("args".into(), finding.case.args.clone());
    node.insert("culture".into(), json!(finding.case.culture));
    node.insert(
        "settings".into(),
        crate::case::settings_json(&finding.case.settings),
    );
    node.insert("dotnet".into(), finding.net.to_json());
    node.insert("rust".into(), finding.rust.to_json());
    if let Some(minimal) = &finding.minimal {
        node.insert(
            "minimal".into(),
            json!({
                "template": minimal.case.template(),
                "args": minimal.case.args.clone(),
                "culture": minimal.case.culture.clone(),
                "settings": crate::case::settings_json(&minimal.case.settings),
                // The shrunk case's *own* answers. The `dotnet` and `rust`
                // above belong to the template above them.
                "dotnet": minimal.net.to_json(),
                "rust": minimal.rust.to_json(),
            }),
        );
    }
    // Ready to paste into the golden table once it has been triaged.
    node.insert(
        "case".into(),
        match &finding.minimal {
            Some(minimal) => minimal.case.to_json(),
            None => finding.case.to_json(),
        },
    );
    Json::Object(node)
}

pub fn write(path: &Path, document: &Json) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    let mut file = std::fs::File::create(path)?;
    file.write_all(serde_json::to_string_pretty(document)?.as_bytes())?;
    file.write_all(b"\n")
}

/// The end-of-campaign summary, for a terminal rather than a file.
pub fn print_summary(summary: &Summary, findings: &[Judged], report: &Path) {
    println!();
    println!("cases run          {}", summary.cases);
    println!("agreements         {}", summary.agreements);
    println!("disagreements      {}", summary.disagreements());
    println!("  new              {}", summary.new);
    println!("  known divergence {}", summary.known);
    println!("  order dependent  {}", summary.order_dependent);
    println!("  harness died     {}", summary.harness_died);
    println!("rust panics        {}", summary.rust_panics);
    println!("dotnet runs        {}", summary.dotnet_invocations);
    println!("elapsed            {:.1}s", summary.seconds);

    let new: Vec<&Judged> = findings
        .iter()
        .filter(|finding| finding.verdict.class() == Some(Class::New))
        .collect();
    if !new.is_empty() {
        println!();
        println!("new disagreements, smallest form:");
        for finding in &new {
            // Inputs and answers have to come from the same rendering, or the
            // line reads as a disagreement nobody can reproduce.
            let (case, net, rust) = match &finding.minimal {
                Some(minimal) => (&minimal.case, &minimal.net, &minimal.rust),
                None => (&finding.case, &finding.net, &finding.rust),
            };
            println!("  {}", finding.case.id);
            println!("    template  {:?}", case.template());
            println!("    args      {}", case.args);
            if !case.culture.is_empty() {
                println!("    culture   {}", case.culture);
            }
            if !case.settings.is_empty() {
                println!("    settings  {}", Json::Object(case.settings.clone()));
            }
            println!("    .NET      {net}");
            println!("    rust      {rust}");
        }
    }
    println!();
    println!("report written to {}", report.display());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::campaign::{Minimal, Verdict};
    use crate::case::{NetOutcome, RustOutcome};
    use crate::gen;

    #[test]
    fn a_report_carries_every_input_and_both_answers() {
        let case = gen::generate(1, 1);
        let smaller = gen::generate(1, 2);
        let finding = Judged {
            case: case.clone(),
            net: NetOutcome::Result("a".into()),
            rust: RustOutcome::Result("b".into()),
            verdict: Verdict::Disagree {
                class: Class::New,
                known: None,
            },
            minimal: Some(Minimal {
                case: smaller.clone(),
                net: NetOutcome::Result("c".into()),
                rust: RustOutcome::Result("d".into()),
            }),
        };
        let options = Options {
            seed: 1,
            count: 1,
            batch_size: 1,
            shrink_batch: 1,
            shrink_rounds: 1,
            confirm_alone: false,
            only: None,
        };
        let summary = Summary {
            cases: 1,
            new: 1,
            ..Summary::default()
        };
        let document = document(&options, &summary, &[finding]);
        let entry = &document["disagreements"][0];
        assert_eq!(entry["class"], "new");
        assert_eq!(entry["template"], case.template());
        assert_eq!(entry["args"], case.args);
        assert_eq!(entry["dotnet"]["result"], "a");
        assert_eq!(entry["rust"]["result"], "b");
        // The minimal case reports the answers *it* got, not the original's.
        assert_eq!(entry["minimal"]["template"], smaller.template());
        assert_eq!(entry["minimal"]["dotnet"]["result"], "c");
        assert_eq!(entry["minimal"]["rust"]["result"], "d");
        assert_eq!(entry["case"]["id"], smaller.id);
        assert_eq!(document["summary"]["disagreements"]["new"], 1);
    }
}
