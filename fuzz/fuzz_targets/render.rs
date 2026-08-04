#![no_main]

use libfuzzer_sys::fuzz_target;
use smartformat::parsing::{Format, FormatItem};
use smartformat::{ErrorAction, SmartFormatter, SmartSettings, Value};
use std::collections::BTreeMap;
use std::sync::OnceLock;

/// A template may legitimately demand an enormous alignment pad — .NET
/// allocates it and so do we (see DESIGN.md, "Non-goals") — so cap it here
/// to keep the fuzzer hunting logic panics instead of by-design allocation.
fn alignment_is_sane(format: &Format) -> bool {
    format.items().iter().all(|item| match item {
        FormatItem::Literal(_) => true,
        FormatItem::Placeholder(p) => {
            p.alignment.unsigned_abs() <= 4096
                && p.format.as_ref().is_none_or(alignment_is_sane)
        }
    })
}

// The full pipeline must never panic: parse leniently, then render against a
// value tree covering every variant. Errors are fine; panics are findings.
fuzz_target!(|template: &str| {
    static SMART: OnceLock<SmartFormatter> = OnceLock::new();
    static ARGS: OnceLock<Value> = OnceLock::new();
    let smart = SMART.get_or_init(|| {
        SmartFormatter::new(SmartSettings {
            parse_error_action: ErrorAction::MaintainTokens,
            format_error_action: ErrorAction::OutputErrorInResult,
            ..SmartSettings::default()
        })
    });
    let args = ARGS.get_or_init(|| {
        Value::List(vec![
            Value::from("text"),
            Value::Int(-42),
            Value::Float(1234.5),
            Value::Bool(true),
            Value::Null,
            Value::Map(BTreeMap::from([
                ("Name".to_owned(), Value::from("Alice")),
                ("Items".to_owned(), Value::List(vec![Value::Int(1), Value::Int(2)])),
            ])),
        ])
    });
    if let Ok(format) = smart.parse(template) {
        if alignment_is_sane(&format) {
            let _ = smart.format_parsed(&format, args);
        }
    }
});
