#![no_main]

use libfuzzer_sys::fuzz_target;
use smartformat::SmartFormatter;
use std::sync::OnceLock;

// The parser must never panic, whatever the template. Errors are fine.
fuzz_target!(|template: &str| {
    static SMART: OnceLock<SmartFormatter> = OnceLock::new();
    let smart = SMART.get_or_init(SmartFormatter::default);
    let _ = smart.parse(template);
});
