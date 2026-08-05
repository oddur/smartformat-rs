//! Compiles every Rust example in `docs/` as a doctest.
//!
//! The items below exist only while `cargo test --doc` collects doctests, so
//! they cost nothing in a normal build. A `rust` block in any of these files
//! is compiled and run; an example that stops matching the API fails the test
//! suite instead of quietly misleading a reader. Prose-only files need no
//! entry here.
//!
//! Blocks that are not meant to run take the usual rustdoc annotations
//! (`ignore`, `no_run`, `text`, `console`).

macro_rules! doc_files {
    ($($name:ident => $path:literal),* $(,)?) => {
        $(
            #[cfg(doctest)]
            #[doc = include_str!($path)]
            pub struct $name;
        )*
    };
}

doc_files! {
    GettingStarted => "../../../docs/tutorials/getting-started.md",
    RunDotnetTemplates => "../../../docs/how-to/run-dotnet-templates.md",
    LocalizeText => "../../../docs/how-to/localize-text.md",
    ChooseErrorBehavior => "../../../docs/how-to/choose-error-behavior.md",
    TestYourTemplates => "../../../docs/how-to/test-your-templates.md",
    AddACulture => "../../../docs/how-to/add-a-culture.md",
    ExtendWithYourOwn => "../../../docs/how-to/extend-with-your-own.md",
    TemplateSyntax => "../../../docs/reference/template-syntax.md",
    Formatters => "../../../docs/reference/formatters.md",
    FormatSpecifiers => "../../../docs/reference/format-specifiers.md",
    SettingsAndFeatures => "../../../docs/reference/settings-and-features.md",
    Cultures => "../../../docs/reference/cultures.md",
    Architecture => "../../../docs/explanation/architecture.md",
}
