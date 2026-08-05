//! Reducing a disagreement to something a person can act on.
//!
//! A two-hundred-character template that disagrees says nothing; `{0:X}` on
//! `-255` says everything. Shrinking is a hill climb over *reductions*: drop an
//! item, shorten a literal, simplify a selector, drop the options, drop a
//! split part, lower the alignment, prune the arguments, drop a setting, fall
//! back to the invariant culture. Every reduction is generated from the syntax
//! tree rather than from the text, so a candidate is still a well-formed
//! template unless it was meant not to be.
//!
//! The search runs in **batches**. A `dotnet run` costs the better part of a
//! second before it renders anything, so asking it one candidate at a time
//! would make shrinking cost more than the campaign that found the case: every
//! candidate of a round goes into one case file and one invocation instead.

use std::collections::HashSet;

use serde_json::{Map, Value as Json};

use crate::case::Case;
use crate::gen::{FormatSpec, Node, Placeholder, Template};

/// What one node becomes: another node, or a run of them — dropping a
/// placeholder in favour of its own first part is the reduction that unwraps
/// nesting, and that one replaces one node with several.
enum Replacement {
    One(Node),
    Many(Vec<Node>),
}

/// Every one-step reduction of a case, smallest first. Duplicates and the case
/// itself are removed; nothing here is guaranteed to still disagree, which is
/// what the batch run decides.
pub fn reductions(case: &Case) -> Vec<Case> {
    let mut out: Vec<Case> = Vec::new();

    for nodes in node_list_reductions(&case.tree.nodes) {
        out.push(Case {
            tree: Template { nodes },
            ..case.clone()
        });
    }
    for args in argument_reductions(&case.args) {
        out.push(Case {
            args,
            ..case.clone()
        });
    }
    for settings in settings_reductions(&case.settings) {
        out.push(Case {
            settings,
            ..case.clone()
        });
    }
    if !case.culture.is_empty() {
        out.push(Case {
            culture: String::new(),
            ..case.clone()
        });
    }

    let mut seen = HashSet::from([fingerprint(case)]);
    out.retain(|candidate| seen.insert(fingerprint(candidate)));
    out.sort_by_key(Case::size);
    out
}

fn fingerprint(case: &Case) -> String {
    format!(
        "{}\u{1}{}\u{1}{}\u{1}{}",
        case.template(),
        case.args,
        case.culture,
        Json::Object(case.settings.clone())
    )
}

// ---------------------------------------------------------------------------
// The template
// ---------------------------------------------------------------------------

fn node_list_reductions(nodes: &[Node]) -> Vec<Vec<Node>> {
    let mut out = Vec::new();
    if nodes.is_empty() {
        return out;
    }

    // Drop one item.
    for index in 0..nodes.len() {
        let mut reduced = nodes.to_vec();
        reduced.remove(index);
        out.push(reduced);
    }
    // Keep only one item: a long run reaches its interesting member in one
    // step rather than in `len - 1` of them.
    if nodes.len() > 2 {
        for node in nodes {
            out.push(vec![node.clone()]);
        }
    }
    // Reduce one item in place.
    for index in 0..nodes.len() {
        for replacement in node_reductions(&nodes[index]) {
            let mut reduced = nodes.to_vec();
            match replacement {
                Replacement::One(node) => reduced[index] = node,
                Replacement::Many(many) => {
                    reduced.splice(index..=index, many);
                }
            }
            out.push(reduced);
        }
    }
    out
}

fn node_reductions(node: &Node) -> Vec<Replacement> {
    match node {
        Node::Literal(text) => literal_reductions(text),
        Node::Placeholder(placeholder) => placeholder_reductions(placeholder),
    }
}

fn literal_reductions(text: &str) -> Vec<Replacement> {
    let mut out = Vec::new();
    let characters: Vec<char> = text.chars().collect();
    if characters.len() > 1 {
        let half: String = characters[..characters.len() / 2].iter().collect();
        out.push(Replacement::One(Node::Literal(half)));
        let first: String = characters[..1].iter().collect();
        out.push(Replacement::One(Node::Literal(first)));
    }
    // An escape that is not what the disagreement is about becomes a plain
    // letter, which takes the escape machinery out of the picture entirely.
    if text.contains('\\') && text != "x" {
        out.push(Replacement::One(Node::Literal("x".into())));
    }
    out
}

fn placeholder_reductions(placeholder: &Placeholder) -> Vec<Replacement> {
    let mut out = Vec::new();
    let mut push = |placeholder: Placeholder| {
        out.push(Replacement::One(Node::Placeholder(Box::new(placeholder))));
    };

    // Alignment: gone, then smaller, then positive.
    if let Some(alignment) = placeholder.alignment {
        push(Placeholder {
            alignment: None,
            ..placeholder.clone()
        });
        if alignment.abs() > 1 {
            push(Placeholder {
                alignment: Some(alignment / 2),
                ..placeholder.clone()
            });
        }
        if alignment < 0 {
            push(Placeholder {
                alignment: Some(-alignment),
                ..placeholder.clone()
            });
        }
    }

    // The selector: one segment shorter, then positional, then nameless.
    for selector in selector_reductions(&placeholder.selector) {
        push(Placeholder {
            selector,
            ..placeholder.clone()
        });
    }

    if let Some(format) = &placeholder.format {
        // The whole format section.
        push(Placeholder {
            format: None,
            ..placeholder.clone()
        });
        // The formatter's name, which turns `{0:list:x}` into `{0:x}`.
        if !format.name.is_empty() {
            push(Placeholder {
                format: Some(FormatSpec {
                    name: String::new(),
                    options: None,
                    ..format.clone()
                }),
                ..placeholder.clone()
            });
        }
        // The options.
        if let Some(options) = &format.options {
            push(Placeholder {
                format: Some(FormatSpec {
                    options: None,
                    ..format.clone()
                }),
                ..placeholder.clone()
            });
            let characters: Vec<char> = options.chars().collect();
            if characters.len() > 1 {
                push(Placeholder {
                    format: Some(FormatSpec {
                        options: Some(characters[..characters.len() / 2].iter().collect()),
                        ..format.clone()
                    }),
                    ..placeholder.clone()
                });
            }
        }
        // One split part.
        if format.parts.len() > 1 {
            for index in 0..format.parts.len() {
                let mut parts = format.parts.clone();
                parts.remove(index);
                push(Placeholder {
                    format: Some(FormatSpec {
                        parts,
                        ..format.clone()
                    }),
                    ..placeholder.clone()
                });
            }
        }
        // Inside a part.
        for (index, part) in format.parts.iter().enumerate() {
            for reduced in node_list_reductions(part) {
                let mut parts = format.parts.clone();
                parts[index] = reduced;
                push(Placeholder {
                    format: Some(FormatSpec {
                        parts,
                        ..format.clone()
                    }),
                    ..placeholder.clone()
                });
            }
        }
        // Unwrap: the placeholder becomes what its first part held, which is
        // how a disagreement buried three levels deep climbs to the top.
        if let Some(first) = format.parts.first() {
            if !first.is_empty() {
                out.push(Replacement::Many(first.clone()));
            }
        }
    }

    out
}

fn selector_reductions(selector: &str) -> Vec<String> {
    let mut out = Vec::new();
    if selector.is_empty() {
        return out;
    }
    if let Some((head, _)) = selector.rsplit_once('.') {
        out.push(head.to_string());
    }
    if let Some(index) = selector.find('[') {
        out.push(selector[..index].to_string());
    }
    if selector != "0" {
        out.push("0".to_string());
    }
    out.push(String::new());
    out
}

// ---------------------------------------------------------------------------
// Arguments and settings
// ---------------------------------------------------------------------------

fn argument_reductions(args: &Json) -> Vec<Json> {
    let mut out = Vec::new();
    match args {
        Json::Array(items) => {
            for index in 0..items.len() {
                let mut reduced = items.clone();
                reduced.remove(index);
                out.push(Json::Array(reduced));
            }
            for (index, item) in items.iter().enumerate() {
                for reduced in argument_reductions(item) {
                    let mut items = items.clone();
                    items[index] = reduced;
                    out.push(Json::Array(items));
                }
            }
        }
        Json::Object(entries) => {
            for key in entries.keys() {
                let mut reduced = entries.clone();
                reduced.remove(key);
                out.push(Json::Object(reduced));
            }
            // A marker object is one value, not a map to prune into.
            if !entries.keys().any(|key| key.starts_with('$')) {
                for (key, value) in entries {
                    for reduced in argument_reductions(value) {
                        let mut entries = entries.clone();
                        entries.insert(key.clone(), reduced);
                        out.push(Json::Object(entries));
                    }
                }
            }
        }
        Json::String(text) if !text.is_empty() => {
            let characters: Vec<char> = text.chars().collect();
            out.push(Json::String(
                characters[..characters.len() / 2].iter().collect(),
            ));
        }
        _ => {}
    }
    out
}

fn settings_reductions(settings: &Map<String, Json>) -> Vec<Map<String, Json>> {
    settings
        .keys()
        .map(|key| {
            let mut reduced = settings.clone();
            reduced.remove(key);
            reduced
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn case() -> Case {
        Case {
            id: "t".into(),
            tree: Template {
                nodes: vec![
                    Node::Literal("hello ".into()),
                    Node::Placeholder(Box::new(Placeholder {
                        selector: "0.Name".into(),
                        alignment: Some(-8),
                        format: Some(FormatSpec {
                            name: "list".into(),
                            options: Some("abc".into()),
                            parts: vec![
                                vec![Node::Placeholder(Box::new(Placeholder {
                                    selector: String::new(),
                                    alignment: None,
                                    format: None,
                                }))],
                                vec![Node::Literal(", ".into())],
                            ],
                        }),
                    })),
                ],
            },
            args: json!([{ "Name": "abc" }]),
            culture: "de".into(),
            settings: Map::new(),
        }
    }

    #[test]
    fn every_reduction_is_smaller_or_simpler_and_none_repeats() {
        let start = case();
        let candidates = reductions(&start);
        assert!(!candidates.is_empty());
        let mut prints: Vec<String> = candidates.iter().map(fingerprint).collect();
        prints.sort();
        let before = prints.len();
        prints.dedup();
        assert_eq!(before, prints.len(), "reductions repeat themselves");
        assert!(candidates
            .iter()
            .all(|c| fingerprint(c) != fingerprint(&start)));
    }

    #[test]
    fn reductions_reach_the_pieces_that_matter() {
        let templates: Vec<String> = reductions(&case()).iter().map(Case::template).collect();
        // The alignment goes.
        assert!(templates
            .iter()
            .any(|t| t == "hello {0.Name:list(abc):{}|, }"));
        // The options go.
        assert!(templates
            .iter()
            .any(|t| t == "hello {0.Name,-8:list:{}|, }"));
        // The formatter goes.
        assert!(templates.iter().any(|t| t == "hello {0.Name,-8}"));
        // The selector shortens.
        assert!(templates
            .iter()
            .any(|t| t == "hello {0,-8:list(abc):{}|, }"));
        // The literal goes.
        assert!(templates.iter().any(|t| t == "{0.Name,-8:list(abc):{}|, }"));
        // A split part goes.
        assert!(templates
            .iter()
            .any(|t| t == "hello {0.Name,-8:list(abc):{}}"));
    }

    #[test]
    fn the_culture_and_the_settings_shrink_too() {
        let mut start = case();
        start
            .settings
            .insert("formatErrorAction".into(), json!("Ignore"));
        let candidates = reductions(&start);
        assert!(candidates.iter().any(|c| c.culture.is_empty()));
        assert!(candidates.iter().any(|c| c.settings.is_empty()));
    }

    #[test]
    fn nesting_unwraps_in_one_step() {
        let start = Case {
            tree: Template {
                nodes: vec![Node::Placeholder(Box::new(Placeholder {
                    selector: "0".into(),
                    alignment: None,
                    format: Some(FormatSpec {
                        name: "cond".into(),
                        options: None,
                        parts: vec![vec![Node::Literal("deep".into())]],
                    }),
                }))],
            },
            ..case()
        };
        let templates: Vec<String> = reductions(&start).iter().map(Case::template).collect();
        assert!(templates.iter().any(|t| t == "deep"));
    }

    #[test]
    fn arguments_prune() {
        let start = case();
        let candidates = reductions(&start);
        assert!(candidates.iter().any(|c| c.args == json!([])));
        assert!(candidates.iter().any(|c| c.args == json!([{}])));
    }
}
