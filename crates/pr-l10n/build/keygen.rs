//! Port of `PhotoReview.Localization.Generator` (C# baseline d92d6aae): `TrGenerator.cs` (456 lines),
//! `FlatJsonReader.cs` (345), `LocDiagnostics.cs` (39). Reads the English catalog (`assets/languages/en.json`) and
//! plans one constant per key (module `keys`) and one typed accessor per key or plural group (module `tr`), so a
//! wrong key name or a wrong number of arguments is a compile error (docs/adr ADR 0006 point 4).
//!
//! This file has NO dependencies besides `std` because it is compiled twice: by `build.rs` (`mod keygen;` through
//! `#[path]`) and by `tests/key_generator.rs` (`#[path = "../build/keygen.rs"] mod keygen;`). Keep it free of `crate::`
//! paths. Unit W2-04. Contract: `contracts/W2-04.md`.
//!
//! Stub: bodies are `todo!()`; data definitions are complete.

// Stub-only (docs/MULTI-AGENT-PLAN D11): removed by the porter in the commit that removes the last `todo!()`.
#![allow(unused_variables, dead_code)]

/// One `"key": "text"` pair of a flat catalog, with the key's position in the file.
/// C# `JsonEntry`. Offsets are BYTE offsets into the text (C# uses UTF-16 offsets; only diagnostics use them).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonEntry {
    pub key: String,
    pub value: String,
    /// Byte offset of the key's opening quote.
    pub key_start: usize,
    /// Byte length of the key token including its quotes.
    pub key_length: usize,
}

/// C# `FlatJsonException`: the catalog is not valid JSON.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlatJsonError {
    pub message: String,
    /// Byte offset of the problem.
    pub position: usize,
}

/// Maximum nesting skipped inside an underscore key (C# `FlatJsonReader.MaxDepth`; C# compares `depth > 64` with the
/// depth starting at 0 for the value of the key, so 65 nested arrays are still accepted: keep it).
pub const MAX_DEPTH: usize = 64;

/// C# `FlatJsonReader.Read`: a flat JSON object of string values. Keys starting with `_` may hold any JSON value and
/// are skipped. Accepts `//` and `/* */` comments and trailing commas like the runtime parser; a leading U+FEFF is
/// skipped; an unpaired surrogate escape in a key or value is an error (`RejectUnpairedSurrogate`).
pub fn read_flat_json(text: &str) -> Result<Vec<JsonEntry>, FlatJsonError> {
    todo!("see contracts/W2-04.md section 'FlatJsonReader'")
}

/// C# `LocDiagnostics` ids. All are build errors.
pub const PRLOC001_INVALID_JSON: &str = "PRLOC001";
pub const PRLOC002_INVALID_KEY: &str = "PRLOC002";
pub const PRLOC003_IDENTIFIER_COLLISION: &str = "PRLOC003";
pub const PRLOC004_INVALID_PLACEHOLDER: &str = "PRLOC004";
pub const PRLOC005_PLURAL_WITHOUT_OTHER: &str = "PRLOC005";
pub const PRLOC006_DUPLICATE_KEY: &str = "PRLOC006";
pub const PRLOC007_MULTIPLE_CATALOGS: &str = "PRLOC007";

/// One build error. `message` is the C# message text (`LocDiagnostics` format strings with the arguments filled in).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocDiagnostic {
    pub id: &'static str,
    pub message: String,
}

/// An English catalog handed to the generator (C# `AdditionalText` named `en.json`).
#[derive(Debug, Clone)]
pub struct CatalogInput {
    /// Full path, used for ordering (ordinal) and in the PRLOC007 message.
    pub path: String,
    /// `None` = the file could not be read.
    pub content: Option<String>,
}

/// A `TrKeys` constant: C# identifier (PascalCase) and the key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConstEntry {
    /// The C# identifier from `ToIdentifier` (collision detection and golden files use this form).
    pub ident: String,
    pub key: String,
}

/// A `Tr` member.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemberKind {
    /// One key. `key` is the full catalog key.
    Simple { key: String },
    /// A `base.one` / `base.other` group. `base` is the key without the suffix.
    Plural { base: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberEntry {
    /// The C# identifier from `ToIdentifier`.
    pub ident: String,
    pub kind: MemberKind,
    /// Placeholder names in order of first appearance, deduplicated; for a plural group: `other`'s then `one`'s,
    /// without `count`.
    pub params: Vec<String>,
    /// English text(s) for the doc comment: `[other]` or `[one, other]` for a group, `[text]` otherwise.
    pub english: Vec<String>,
}

/// Result of analysing the English catalog (what C# `Execute` computes before it renders).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plan {
    /// Diagnostics in the order C# reports them.
    pub diagnostics: Vec<LocDiagnostic>,
    /// `false` when C# adds no `Tr.g.cs` (no `en.json` at all, unreadable file, invalid JSON).
    pub generated: bool,
    pub consts: Vec<ConstEntry>,
    pub members: Vec<MemberEntry>,
}

/// C# `TrGenerator.Initialize` + `Execute` analysis: picks the first catalog by ordinal path (the others get
/// PRLOC007), reads it, validates keys, claims identifiers, groups plurals. Only files whose name equals `en.json`
/// ignoring case are considered: filter BEFORE calling (the generator does `Path.GetFileName(f.Path)`).
pub fn plan(inputs: &[CatalogInput]) -> Plan {
    todo!("see contracts/W2-04.md section 'TrGenerator.Execute'")
}

/// C# `IsValidKey`: ASCII letters/digits/'.'; first char a letter; no '.' at the end; no empty segment.
pub fn is_valid_key(key: &str) -> bool {
    todo!()
}

/// C# `ToIdentifier`: split on '.', drop empty segments, upper-case each segment's first char (`char.ToUpperInvariant`
/// on an ASCII letter or digit), keep the rest as is. `"status.batchDone"` -> `"StatusBatchDone"`.
pub fn to_identifier(key: &str) -> String {
    todo!()
}

/// C# `TryGetPlaceholders`: names in order of first appearance, deduplicated; `None` when the text is invalid
/// (same grammar as `pr_l10n::template::LocTemplate::try_parse`).
pub fn try_get_placeholders(text: &str) -> Option<Vec<String>> {
    todo!()
}

/// Rust name of a `keys` constant: SCREAMING_SNAKE of the C# identifier (`StatusBatchDone` -> `STATUS_BATCH_DONE`,
/// an `_` before every upper-case letter except the first char). Injective on identifiers.
pub fn const_name(ident: &str) -> String {
    todo!()
}

/// Rust name of a `tr` function: snake_case of the C# identifier by the same rule (`status_batch_done`). A Rust
/// keyword becomes a raw identifier (`r#type`); `self`/`crate`/`super` cannot be raw identifiers: [`plan`] reports
/// PRLOC003 ("a built-in member") for them like it does for the C# reserved names.
pub fn fn_name(ident: &str) -> String {
    todo!()
}

/// Rust parameter name for a placeholder: the placeholder verbatim, `r#`-escaped when it is a Rust keyword, and
/// `name_` for `self`/`Self`/`crate`/`super`.
pub fn param_name(placeholder: &str) -> String {
    todo!()
}

/// Source of module `keys` (`pub const STATUS_BATCH_DONE: &str = "status.batchDone";` per constant, ending with a
/// newline) written to `$OUT_DIR/tr_keys.rs` and included as `pr_l10n::keys`.
pub fn render_keys(plan: &Plan) -> String {
    todo!()
}

/// Source of module `tr` (`pub fn status_batch_done(count: impl Into<LocValue>, ...) -> String`, plural groups as
/// `pub fn test_items(count: i64, ...)`) written to `$OUT_DIR/tr_fns.rs` and included as `pr_l10n::tr`.
pub fn render_tr(plan: &Plan) -> String {
    todo!()
}
