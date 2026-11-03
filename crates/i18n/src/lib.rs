//! Localization for the game (platform/docs/localization.md).
//!
//! * [`syntax`]: the message syntax (a Fluent subset: named arguments, plural and gender
//!   selects, references);
//! * [`plural`], [`numfmt`], [`locale`]: CLDR plural rules, number and date conventions,
//!   language tags and the fallback chain (requested -> family -> `en`);
//! * [`catalog`]: the bundles of all languages and the [`Translator`] that formats a
//!   message; [`builtin`] compiled-in bundles, [`external`] the player's folder;
//! * [`validate`]: the bundle linter (missing and unused keys, placeholders, plurals);
//! * [`state`]: the active language and the [`t!`] / [`tf!`] lookups used by the code;
//! * [`pseudo`]: the `en-XA` test language.
//!
//! No dependencies, no threads, no platform APIs: this crate builds for wasm32 and Android.

pub mod arg;
pub mod builtin;
pub mod bundle;
pub mod catalog;
pub mod diag;
pub mod external;
pub mod extract;
pub mod locale;
pub mod numfmt;
pub mod parts;
pub mod plural;
pub mod pseudo;
pub mod state;
pub mod syntax;
pub mod tool;
pub mod validate;

pub use arg::Arg;
pub use catalog::{Catalog, LanguageInfo, Origin, Translator};
pub use diag::{Diagnostic, Severity};
pub use state::{
    catalog as active_catalog, content, content_key, content_slug, context, decimal_separator,
    enter_scope, generation, has, language, missing_keys, number, number_int, scope, set_catalog,
    set_language, set_scope, tr, tr_args, tr_dynamic, with_language, with_scope, ScopeGuard,
};
