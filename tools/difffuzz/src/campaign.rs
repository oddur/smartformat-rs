//! A campaign: generate, render on both sides, diff, classify, shrink.

use std::time::Instant;

use crate::case::{Case, NetOutcome, RustOutcome};
use crate::classify::{self, Class, Known};
use crate::dotnetside::{Harness, HarnessError};
use crate::gen;
use crate::rustside::{self, PINNED_NOW};
use crate::shrink;

pub struct Options {
    pub seed: u64,
    pub count: usize,
    pub batch_size: usize,
    /// Cap on how many candidates one shrinking round renders.
    pub shrink_batch: usize,
    /// Cap on how many rounds a single disagreement is shrunk for.
    pub shrink_rounds: usize,
    /// Re-run a disagreeing case on its own before reporting it, so a
    /// difference caused by .NET state left over from an earlier case in the
    /// batch is told apart from a difference in the rendering.
    pub confirm_alone: bool,
    /// Only this case of the campaign, for reproducing one finding.
    pub only: Option<usize>,
}

/// What a case came to.
#[derive(Clone, Debug)]
pub enum Verdict {
    Agree,
    Disagree { class: Class, known: Option<Known> },
}

impl Verdict {
    pub fn class(&self) -> Option<Class> {
        match self {
            Verdict::Agree => None,
            Verdict::Disagree { class, .. } => Some(*class),
        }
    }

    pub fn known(&self) -> Option<&Known> {
        match self {
            Verdict::Agree => None,
            Verdict::Disagree { known, .. } => known.as_ref(),
        }
    }
}

/// One case, both answers, and what they add up to.
pub struct Judged {
    pub case: Case,
    pub net: NetOutcome,
    pub rust: RustOutcome,
    pub verdict: Verdict,
    /// The smallest case still showing the same kind of disagreement, with the
    /// answers *it* got. Reporting the shrunk inputs beside the original
    /// outputs would describe a rendering that never happened, and the minimal
    /// case is the one a person reads.
    pub minimal: Option<Minimal>,
}

/// The result of shrinking: a case and what each engine said about it.
pub struct Minimal {
    pub case: Case,
    pub net: NetOutcome,
    pub rust: RustOutcome,
}

#[derive(Default)]
pub struct Summary {
    pub cases: usize,
    pub agreements: usize,
    pub new: usize,
    pub known: usize,
    pub harness_died: usize,
    pub order_dependent: usize,
    pub rust_panics: usize,
    pub dotnet_invocations: u32,
    pub seconds: f64,
}

impl Summary {
    pub fn disagreements(&self) -> usize {
        self.new + self.known + self.harness_died + self.order_dependent
    }
}

/// Ties the two engines together: renders a batch on both sides and judges it.
pub struct Runner<'a> {
    harness: Option<&'a Harness>,
}

impl<'a> Runner<'a> {
    pub fn new(harness: Option<&'a Harness>) -> Self {
        Self { harness }
    }

    fn now(&self) -> String {
        match self.harness {
            Some(harness) => harness.now.borrow().clone(),
            None => PINNED_NOW.to_string(),
        }
    }

    /// Renders a batch with both engines. Without a harness the .NET side is
    /// `Died` for every case, which is what `--no-dotnet` reports.
    pub fn judge(&self, cases: &[Case]) -> Result<Vec<Judged>, HarnessError> {
        let net = match self.harness {
            Some(harness) => harness.run(cases)?.outcomes,
            None => vec![NetOutcome::Died; cases.len()],
        };
        let now = self.now();

        Ok(cases
            .iter()
            .zip(net)
            .map(|(case, net)| {
                let rust = rustside::render(case, &now);
                let verdict = judge_one(case, &net, &rust);
                Judged {
                    case: case.clone(),
                    net,
                    rust,
                    verdict,
                    minimal: None,
                }
            })
            .collect())
    }

    /// Judges one case on its own, in its own harness invocation.
    fn judge_alone(&self, case: &Case) -> Result<Judged, HarnessError> {
        Ok(self
            .judge(std::slice::from_ref(case))?
            .pop()
            .expect("one case in, one out"))
    }
}

/// Whether a case agrees, and if not, what kind of disagreement it is.
fn judge_one(case: &Case, net: &NetOutcome, rust: &RustOutcome) -> Verdict {
    if matches!(net, NetOutcome::Died) {
        return Verdict::Disagree {
            class: Class::HarnessDied,
            known: None,
        };
    }
    if classify::agrees(case, net, rust) {
        return Verdict::Agree;
    }
    match classify::known_divergence(case, net, rust) {
        Some(known) => Verdict::Disagree {
            class: Class::Known,
            known: Some(known),
        },
        None => Verdict::Disagree {
            class: Class::New,
            known: None,
        },
    }
}

/// Runs a whole campaign over the cases the seed generates.
pub fn run(options: &Options, harness: Option<&Harness>) -> (Summary, Vec<Judged>) {
    let indices: Vec<usize> = match options.only {
        Some(index) => vec![index],
        None => (0..options.count).collect(),
    };
    let cases: Vec<Case> = indices
        .iter()
        .map(|index| gen::generate(options.seed, *index))
        .collect();
    run_cases(&cases, options, harness)
}

/// Runs a campaign over cases somebody else chose — a corpus file, or one case
/// being triaged by hand. Everything downstream of generating is the same, so
/// a corpus replay is judged, confirmed and shrunk exactly as a campaign is.
pub fn run_cases(
    cases: &[Case],
    options: &Options,
    harness: Option<&Harness>,
) -> (Summary, Vec<Judged>) {
    let started = Instant::now();
    let runner = Runner::new(harness);

    let mut summary = Summary {
        cases: cases.len(),
        ..Summary::default()
    };
    let mut findings = Vec::new();
    let batch_size = options.batch_size.max(1);

    for (number, batch) in cases.chunks(batch_size).enumerate() {
        eprintln!(
            "batch {} — cases {}..{} of {}",
            number + 1,
            number * batch_size,
            number * batch_size + batch.len(),
            cases.len()
        );
        let judged = match runner.judge(batch) {
            Ok(judged) => judged,
            Err(error) if error.fatal => {
                eprintln!("difffuzz: {error}");
                break;
            }
            Err(error) => {
                eprintln!("  batch failed: {error}");
                continue;
            }
        };

        for mut judgement in judged {
            if matches!(judgement.rust, RustOutcome::Panic(_)) {
                summary.rust_panics += 1;
            }
            if judgement.verdict.class().is_none() {
                summary.agreements += 1;
                continue;
            }

            // A disagreement inside a batch may be a difference in the
            // rendering or a difference in what .NET was left holding by an
            // earlier case in the same process — `ListFormatter.CollectionIndex`
            // is a static, and a case that fails part-way through an iteration
            // leaves it set for the rest of the run. Rendering the case alone
            // tells the two apart.
            if options.confirm_alone && harness.is_some() && batch.len() > 1 {
                if let Ok(alone) = runner.judge_alone(&judgement.case) {
                    if alone.verdict.class().is_none() {
                        judgement.verdict = Verdict::Disagree {
                            class: Class::OrderDependent,
                            known: None,
                        };
                    } else {
                        judgement.net = alone.net;
                        judgement.rust = alone.rust;
                        judgement.verdict = alone.verdict;
                    }
                }
            }

            match judgement.verdict.class() {
                Some(Class::New) => summary.new += 1,
                Some(Class::Known) => summary.known += 1,
                Some(Class::HarnessDied) => summary.harness_died += 1,
                Some(Class::OrderDependent) => summary.order_dependent += 1,
                None => unreachable!("an agreement was counted above"),
            }

            if judgement.verdict.class() == Some(Class::New) && harness.is_some() {
                judgement.minimal = Some(minimise(&runner, &judgement, options));
            }
            findings.push(judgement);
        }
    }

    summary.dotnet_invocations = harness.map_or(0, |harness| harness.invocations.get());
    summary.seconds = started.elapsed().as_secs_f64();
    (summary, findings)
}

/// Reduces a case while it keeps disagreeing in a way nothing explains.
///
/// Each round renders every candidate in **one** harness invocation and keeps
/// the smallest survivor. The condition a survivor has to meet is "still a new
/// disagreement", not "the same output": requiring identical output would stop
/// the shrink at the first reduction that changes a padding width, and
/// requiring only "still disagrees" would let it wander into a divergence
/// `DESIGN.md` already covers.
pub fn minimise(runner: &Runner<'_>, judged: &Judged, options: &Options) -> Minimal {
    let case = &judged.case;
    let mut current = Minimal {
        case: case.clone(),
        net: judged.net.clone(),
        rust: judged.rust.clone(),
    };

    for round in 0..options.shrink_rounds {
        let candidates = sample(shrink::reductions(&current.case), options.shrink_batch);
        if candidates.is_empty() {
            break;
        }
        // Ids have to be unique inside one case file, or the harness cannot
        // tell the answers apart.
        let numbered: Vec<Case> = candidates
            .into_iter()
            .enumerate()
            .map(|(index, mut candidate)| {
                candidate.id = format!("{}-s{round}-{index}", case.id);
                candidate
            })
            .collect();

        let Ok(judged) = runner.judge(&numbered) else {
            break;
        };
        let best = judged
            .into_iter()
            .filter(|judgement| judgement.verdict.class() == Some(Class::New))
            .map(|judgement| Minimal {
                case: judgement.case,
                net: judgement.net,
                rust: judgement.rust,
            })
            .min_by_key(|minimal| minimal.case.size());

        match best {
            Some(smaller) if smaller.case.size() < current.case.size() => current = smaller,
            _ => break,
        }
    }

    current.case.id = format!("{}-min", case.id);
    current
}

/// Caps a round at `limit` candidates while keeping the whole size range: the
/// list is sorted smallest first, so taking a prefix would only ever try the
/// most aggressive reductions, which are the least likely to survive.
fn sample(candidates: Vec<Case>, limit: usize) -> Vec<Case> {
    let limit = limit.max(1);
    if candidates.len() <= limit {
        return candidates;
    }
    let stride = candidates.len() as f64 / limit as f64;
    (0..limit)
        .map(|index| candidates[(index as f64 * stride) as usize].clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::case::ErrorKind;
    use serde_json::{json, Map};

    fn a_case() -> Case {
        // Written out rather than generated: the classifier reads the template
        // and the arguments, and a case that happens to trip one of its rules
        // would make these tests about the rules instead.
        Case {
            id: "t".into(),
            tree: crate::gen::Template {
                nodes: vec![crate::gen::Node::Literal("{0:N2}".into())],
            },
            args: json!([1]),
            culture: String::new(),
            settings: Map::new(),
        }
    }

    #[test]
    fn a_matching_result_is_an_agreement() {
        let verdict = judge_one(
            &a_case(),
            &NetOutcome::Result("x".into()),
            &RustOutcome::Result("x".into()),
        );
        assert!(verdict.class().is_none());
    }

    #[test]
    fn an_unexplained_difference_is_new() {
        let verdict = judge_one(
            &a_case(),
            &NetOutcome::Result("x".into()),
            &RustOutcome::Result("y".into()),
        );
        assert_eq!(verdict.class(), Some(Class::New));
    }

    #[test]
    fn a_dead_harness_is_its_own_class() {
        let verdict = judge_one(
            &a_case(),
            &NetOutcome::Died,
            &RustOutcome::Error {
                kind: ErrorKind::Parse,
                message: String::new(),
            },
        );
        assert_eq!(verdict.class(), Some(Class::HarnessDied));
    }

    #[test]
    fn sampling_keeps_the_whole_range() {
        let mut cases = Vec::new();
        for length in 1..100 {
            let mut case = a_case();
            case.args = json!("a".repeat(length));
            case.settings = Map::new();
            cases.push(case);
        }
        cases.sort_by_key(Case::size);
        let sampled = sample(cases.clone(), 10);
        assert_eq!(sampled.len(), 10);
        assert_eq!(sampled[0].size(), cases[0].size());
        assert!(sampled[9].size() > sampled[0].size());
    }

    #[test]
    fn a_campaign_without_the_harness_still_runs_every_case() {
        let options = Options {
            seed: 5,
            count: 20,
            batch_size: 7,
            shrink_batch: 8,
            shrink_rounds: 2,
            confirm_alone: false,
            only: None,
        };
        let (summary, findings) = run(&options, None);
        assert_eq!(summary.cases, 20);
        // Every case is `HarnessDied` without a harness, and none is shrunk.
        assert_eq!(summary.harness_died, 20);
        assert_eq!(findings.len(), 20);
        assert!(findings.iter().all(|finding| finding.minimal.is_none()));
    }
}
