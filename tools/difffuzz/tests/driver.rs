//! Drives the whole pipeline against the stand-in harness in
//! `src/bin/fake_harness.rs`.
//!
//! What is under test here is the *driver*, not the rendering: that a batch is
//! written, invoked and matched up by id; that a case which kills the harness
//! is found by bisection rather than blamed on the whole batch; that a hang is
//! killed and bisected the same way; that a case the harness silently skips is
//! not silently counted as an agreement; and that a disagreement shrinks.
//!
//! The stand-in echoes the template back as the result, so a case's answer is
//! predictable from its template and the tests can say exactly what should
//! happen.

use std::path::PathBuf;
use std::time::Duration;

use difffuzz::campaign::{self, Options, Runner};
use difffuzz::case::{Case, NetOutcome};
use difffuzz::dotnetside::Harness;
use difffuzz::gen::{Node, Template};
use difffuzz::rustside::PINNED_NOW;
use serde_json::{json, Map};

fn harness(timeout_secs: u64) -> Harness {
    let scratch = std::env::temp_dir().join(format!("difffuzz-test-{}", std::process::id()));
    std::fs::create_dir_all(&scratch).expect("a scratch directory");
    Harness::new(
        env!("CARGO_BIN_EXE_fake-harness").to_string(),
        PathBuf::from(env!("CARGO_MANIFEST_DIR")),
        scratch,
        Duration::from_secs(timeout_secs),
    )
}

/// A case whose template is exactly `template`, so the stand-in's answer is
/// known in advance.
fn case(id: &str, template: &str) -> Case {
    Case {
        id: id.to_string(),
        tree: Template {
            nodes: vec![Node::Literal(template.to_string())],
        },
        args: json!([]),
        culture: String::new(),
        settings: Map::new(),
    }
}

#[test]
fn a_batch_comes_back_matched_up_by_id() {
    let harness = harness(60);
    let cases = vec![case("a", "one"), case("b", "two"), case("c", "three")];
    let answers = harness.run(&cases).expect("the batch runs").outcomes;
    assert_eq!(
        answers,
        vec![
            NetOutcome::Result("one".into()),
            NetOutcome::Result("two".into()),
            NetOutcome::Result("three".into()),
        ]
    );
    assert_eq!(harness.invocations.get(), 1, "one batch, one invocation");
    assert_eq!(*harness.now.borrow(), PINNED_NOW);
}

#[test]
fn an_error_comes_back_as_its_exception_name() {
    let harness = harness(60);
    let answers = harness
        .run(&[case("a", "THROW")])
        .expect("the batch runs")
        .outcomes;
    assert_eq!(
        answers,
        vec![NetOutcome::Error("FormattingException".into())]
    );
}

#[test]
fn a_case_that_kills_the_harness_is_found_by_bisection() {
    let harness = harness(60);
    let mut cases: Vec<Case> = (0..16)
        .map(|index| case(&format!("c{index}"), &format!("plain {index}")))
        .collect();
    cases[11] = case("c11", "BOOM");

    let answers = harness.run(&cases).expect("the batch survives").outcomes;
    assert_eq!(answers[11], NetOutcome::Died, "the culprit is blamed");
    for (index, answer) in answers.iter().enumerate() {
        if index != 11 {
            assert_eq!(
                *answer,
                NetOutcome::Result(format!("plain {index}")),
                "case {index} still got its answer"
            );
        }
    }
    // Sixteen cases, one killer: bisection costs a handful of invocations, not
    // one per case.
    assert!(
        harness.invocations.get() < 16,
        "bisection took {} invocations",
        harness.invocations.get()
    );
}

#[test]
fn two_killers_in_one_batch_are_both_found() {
    let harness = harness(60);
    let mut cases: Vec<Case> = (0..8)
        .map(|index| case(&format!("c{index}"), &format!("plain {index}")))
        .collect();
    cases[1] = case("c1", "BOOM one");
    cases[6] = case("c6", "BOOM two");

    let answers = harness.run(&cases).expect("the batch survives").outcomes;
    assert_eq!(answers[1], NetOutcome::Died);
    assert_eq!(answers[6], NetOutcome::Died);
    assert_eq!(answers[0], NetOutcome::Result("plain 0".into()));
    assert_eq!(answers[7], NetOutcome::Result("plain 7".into()));
}

#[test]
fn a_hang_is_killed_and_bisected() {
    let harness = harness(2);
    let mut cases: Vec<Case> = (0..4)
        .map(|index| case(&format!("c{index}"), &format!("plain {index}")))
        .collect();
    cases[2] = case("c2", "HANG");

    let answers = harness.run(&cases).expect("the batch survives").outcomes;
    assert_eq!(answers[2], NetOutcome::Died);
    assert_eq!(answers[0], NetOutcome::Result("plain 0".into()));
    assert_eq!(answers[3], NetOutcome::Result("plain 3".into()));
}

#[test]
fn a_case_the_harness_leaves_out_is_not_an_agreement() {
    let harness = harness(60);
    let cases = vec![case("a", "one"), case("b", "SILENT"), case("c", "three")];
    let answers = harness.run(&cases).expect("the batch runs").outcomes;
    assert_eq!(answers[0], NetOutcome::Result("one".into()));
    assert_eq!(answers[1], NetOutcome::Died);
    assert_eq!(answers[2], NetOutcome::Result("three".into()));
}

#[test]
fn an_empty_batch_costs_nothing() {
    let harness = harness(60);
    assert!(harness.run(&[]).expect("nothing to do").outcomes.is_empty());
    assert_eq!(harness.invocations.get(), 0);
}

#[test]
fn a_disagreement_shrinks_to_the_part_that_disagrees() {
    // The stand-in echoes the template, so every case where the port renders
    // something other than the template's own text is a disagreement — which
    // makes the whole template one big finding. Shrinking has to reduce it to
    // the smallest piece that still disagrees, and that is a single
    // placeholder: literal-only templates render to themselves and agree.
    let harness = harness(60);
    let runner = Runner::new(Some(&harness));
    let options = Options {
        seed: 0,
        count: 1,
        batch_size: 32,
        shrink_batch: 64,
        shrink_rounds: 10,
        confirm_alone: false,
        only: None,
    };

    let start = Case {
        id: "big".into(),
        tree: Template {
            nodes: vec![
                Node::Literal("a long stretch of literal text ".into()),
                Node::Literal("and more of it, all of which agrees ".into()),
                Node::Placeholder(Box::new(difffuzz::gen::Placeholder {
                    selector: "0".into(),
                    alignment: Some(-9),
                    format: Some(difffuzz::gen::FormatSpec {
                        name: "list".into(),
                        options: Some("unused".into()),
                        parts: vec![
                            vec![Node::Literal("N2".into())],
                            vec![Node::Literal(", ".into())],
                        ],
                    }),
                })),
                Node::Literal(" and a tail".into()),
            ],
        },
        args: json!([[1, 2, 3]]),
        culture: "de".into(),
        settings: Map::new(),
    };

    let judged = runner
        .judge(std::slice::from_ref(&start))
        .expect("the case runs");
    assert!(
        judged[0].verdict.class().is_some(),
        "the stand-in and the port must disagree for this test to mean anything"
    );

    let minimal = campaign::minimise(&runner, &start, &options);
    assert!(
        minimal.size() < start.size(),
        "shrinking got nowhere: {:?}",
        minimal.template()
    );
    assert!(
        minimal.template().len() < 20,
        "not minimal enough: {:?}",
        minimal.template()
    );
    assert!(
        minimal.template().contains('{'),
        "a literal-only template agrees with the stand-in, so it cannot be the answer: {:?}",
        minimal.template()
    );
}

#[test]
fn a_whole_campaign_runs_against_the_stand_in() {
    let harness = harness(120);
    let options = Options {
        seed: 20260731,
        count: 12,
        batch_size: 6,
        shrink_batch: 24,
        shrink_rounds: 2,
        confirm_alone: false,
        only: None,
    };
    let (summary, findings) = campaign::run(&options, Some(&harness));
    assert_eq!(summary.cases, 12);
    assert_eq!(summary.rust_panics, 0);
    assert_eq!(
        summary.agreements + summary.disagreements(),
        12,
        "every case is accounted for"
    );
    let document = difffuzz::report::document(&options, &summary, &findings);
    assert_eq!(document["seed"], 20260731);
    assert_eq!(document["summary"]["cases"], 12);
    assert_eq!(
        document["disagreements"].as_array().map(Vec::len),
        Some(findings.len())
    );
}

#[test]
fn one_case_of_a_campaign_can_be_reproduced_on_its_own() {
    let options = Options {
        seed: 4242,
        count: 50,
        batch_size: 50,
        shrink_batch: 8,
        shrink_rounds: 1,
        confirm_alone: false,
        only: Some(37),
    };
    let (summary, findings) = campaign::run(&options, None);
    assert_eq!(summary.cases, 1);
    assert_eq!(findings[0].case.id, "fz-4242-37");
    assert_eq!(
        findings[0].case.template(),
        difffuzz::gen::generate(4242, 37).template()
    );
}
