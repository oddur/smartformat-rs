//! The case a campaign hands to both engines, and what each engine answered.
//!
//! A case is exactly the golden file's case object with its `expected` removed:
//! `tools/goldens` fills that in. Keeping the shape identical means a
//! disagreement can be pasted straight into `goldens/m1.json`'s case table once
//! it has been triaged.

use std::fmt;

use serde_json::{json, Map, Value as Json};

use crate::gen::Template;

/// One rendering to compare. `template`, `args`, `culture` and `settings` are
/// the four inputs both engines read.
#[derive(Clone, Debug)]
pub struct Case {
    pub id: String,
    /// The generated syntax tree, kept alongside the text so the shrinker can
    /// reduce structurally rather than by cutting characters.
    pub tree: Template,
    pub args: Json,
    pub culture: String,
    pub settings: Map<String, Json>,
}

impl Case {
    pub fn template(&self) -> String {
        self.tree.render()
    }

    /// The case object the harness reads: the golden shape without `expected`.
    pub fn to_json(&self) -> Json {
        let mut node = Map::new();
        node.insert("id".into(), json!(self.id));
        node.insert("template".into(), json!(self.template()));
        node.insert("args".into(), self.args.clone());
        node.insert("culture".into(), json!(self.culture));
        if !self.settings.is_empty() {
            node.insert("settings".into(), Json::Object(self.settings.clone()));
        }
        Json::Object(node)
    }

    /// How much there is left to read. The shrinker minimises this, weighting
    /// the template heavily: a short template with a long argument tree is far
    /// easier to act on than the other way round.
    pub fn size(&self) -> usize {
        self.template().chars().count() * 8
            + self.args.to_string().chars().count()
            + self.settings.len() * 16
            + self.culture.len()
    }
}

/// A settings object with nothing in it renders as no `settings` key at all, so
/// a report and a golden case object read the same.
pub fn settings_json(settings: &Map<String, Json>) -> Json {
    if settings.is_empty() {
        Json::Null
    } else {
        Json::Object(settings.clone())
    }
}

/// The document written for the harness.
pub fn cases_document(cases: &[Case]) -> Json {
    json!({ "cases": cases.iter().map(Case::to_json).collect::<Vec<_>>() })
}

/// Reads back a document in that shape — a corpus file, or a report's `case`
/// objects lifted out of it.
///
/// The tree such a case carries is the template as one literal node, because
/// the text is all there is to go on. It renders verbatim, so both engines see
/// exactly what the file says; only the shrinker is weaker on it, and a corpus
/// case has already been shrunk.
pub fn read_cases(document: &Json) -> Result<Vec<Case>, String> {
    let array = match document {
        Json::Array(array) => array,
        Json::Object(object) => match object.get("cases") {
            Some(Json::Array(array)) => array,
            _ => return Err("no \"cases\" array".to_string()),
        },
        _ => return Err("the top level is neither an object nor an array".to_string()),
    };

    array
        .iter()
        .enumerate()
        .map(|(index, node)| read_case(node, index))
        .collect()
}

fn read_case(node: &Json, index: usize) -> Result<Case, String> {
    let object = node
        .as_object()
        .ok_or_else(|| format!("case {index} is not an object"))?;
    let template = object
        .get("template")
        .and_then(Json::as_str)
        .ok_or_else(|| format!("case {index} has no string \"template\""))?;
    let settings = match object.get("settings") {
        None | Some(Json::Null) => Map::new(),
        Some(Json::Object(settings)) => settings.clone(),
        Some(_) => return Err(format!("case {index}: \"settings\" is not an object")),
    };
    Ok(Case {
        id: object
            .get("id")
            .and_then(Json::as_str)
            .map_or_else(|| format!("case-{index}"), str::to_string),
        tree: Template {
            nodes: vec![crate::gen::Node::Literal(template.to_string())],
        },
        args: object.get("args").cloned().unwrap_or_else(|| json!([])),
        culture: object
            .get("culture")
            .and_then(Json::as_str)
            .unwrap_or_default()
            .to_string(),
        settings,
    })
}

/// What SmartFormat.NET answered. `Died` is not an answer: the harness process
/// stopped before it produced one (a stack overflow takes the runtime with it).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NetOutcome {
    Result(String),
    /// The exception type name, as `GetType().Name` writes it.
    Error(String),
    Died,
}

impl fmt::Display for NetOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NetOutcome::Result(text) => write!(f, "{text:?}"),
            NetOutcome::Error(kind) => write!(f, "<{kind}>"),
            NetOutcome::Died => write!(f, "<the harness process died>"),
        }
    }
}

impl NetOutcome {
    pub fn to_json(&self) -> Json {
        match self {
            NetOutcome::Result(text) => json!({ "result": text }),
            NetOutcome::Error(kind) => json!({ "error": kind }),
            NetOutcome::Died => json!({ "died": true }),
        }
    }
}

/// What this port answered. A panic is a finding in its own right, so it is an
/// outcome rather than the end of the campaign.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RustOutcome {
    Result(String),
    Error { kind: ErrorKind, message: String },
    Panic(String),
}

impl fmt::Display for RustOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RustOutcome::Result(text) => write!(f, "{text:?}"),
            RustOutcome::Error { kind, message } => write!(f, "<{kind:?}> {message}"),
            RustOutcome::Panic(message) => write!(f, "<panic> {message}"),
        }
    }
}

impl RustOutcome {
    pub fn to_json(&self) -> Json {
        match self {
            RustOutcome::Result(text) => json!({ "result": text }),
            RustOutcome::Error { kind, message } => {
                json!({ "error": format!("{kind:?}"), "message": message })
            }
            RustOutcome::Panic(message) => json!({ "panic": message }),
        }
    }
}

/// The `Error` variants the golden runner's exception-name table distinguishes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    Parse,
    Escape,
    Format,
    UnsupportedSpec,
    /// Anything else `Error` grows, plus the errors this fuzzer raises itself
    /// (an unknown setting, a culture the table does not carry).
    Other,
}
