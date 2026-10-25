//! Command-boundary failures.
//!
//! Domain errors stay in their modules. A command wraps one when that failure
//! becomes the process result. `Display` is the sentence that path already prints.

use std::io;
use std::path::{Path, PathBuf};

use crate::catalog::{CartridgeError, CatalogError, ResolveError, TimeoutError};
use crate::config_edit::{RefuseReason, ReplaceError, SecretError};
use crate::skill::SkillFailure;

/// One command failure. The variant name is the machine-readable kind.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// `--json` or `--jsonl` was combined with `--output text`.
    #[error(
        "{flag} conflicts with --output text, so nothing was written. Drop {flag} or pass --output json."
    )]
    JsonFlagConflicts {
        /// `--json` or `--jsonl`.
        flag: &'static str,
    },
    /// `--quiet` was combined with JSON output.
    #[error(
        "--quiet conflicts with json output, so nothing was written. Drop --quiet or pass --output text."
    )]
    QuietConflictsJson,
    /// The shell name is not one of the five completion targets.
    #[error(
        "unknown shell {shell}. Pass bash, zsh, fish, elvish, or powershell. The config was not read and nothing was written."
    )]
    UnknownShell {
        /// The name that was passed.
        shell: String,
    },
    /// The generated completion script was not UTF-8.
    #[error(
        "the completion script was not valid UTF-8, so nothing was written. Retry completions."
    )]
    CompletionNotUtf8,
    /// A JSON value could not be encoded.
    #[error("could not encode {what} as JSON: {source}. {retry}")]
    JsonEncode {
        /// What was being encoded, as the sentence already names it.
        what: &'static str,
        /// The encoder failure.
        #[source]
        source: serde_json::Error,
        /// The retry sentence that follows the detail.
        retry: &'static str,
    },
    /// Stdout rejected the body.
    #[error("could not write {what}: {source}. Retry when stdout accepts output.")]
    Stdout {
        /// What was being written, as the sentence already names it.
        what: &'static str,
        /// The write failure.
        #[source]
        source: io::Error,
    },
    /// Clap rejected the invocation. Kind stays `usage`.
    #[error("{}", clap_sentence(source))]
    Usage {
        /// The clap error. The printed sentence is its trimmed display.
        #[source]
        source: clap::Error,
    },
    /// `list` was given an empty `--config`.
    #[error(
        "--config is empty, so the models were not listed. Pass --config with the file to read."
    )]
    ListConfigEmpty,
    /// `list` has no home path.
    #[error(
        "HOME is empty and GROK_HOME is unset, so there is no config path. Pass --config with the file to read."
    )]
    ListHomeEmpty,
    /// `list` has no home variable.
    #[error(
        "HOME is not set and GROK_HOME is unset, so there is no config path. Pass --config with the file to read."
    )]
    ListHomeUnset,
    /// `list` was pointed at a file that is not there.
    #[error(
        "could not read config {}. The file is missing, so the models were not listed. Create the file or pass --config with an existing file.",
        path.display()
    )]
    ListConfigMissing {
        /// Path that was not found.
        path: PathBuf,
    },
    /// `list` could not read the config.
    #[error(
        "could not read config {}. {source}. The models were not listed. Pass --config pointing at a readable file.",
        path.display()
    )]
    ListConfigIo {
        /// Path that could not be read.
        path: PathBuf,
        /// The filesystem error.
        #[source]
        source: io::Error,
    },
    /// `list` read bytes that are not UTF-8.
    #[error(
        "config {} is not UTF-8, so the models were not listed. Save it as UTF-8 and pass that --config again.",
        path.display()
    )]
    ListConfigUtf8 {
        /// Path that was not UTF-8.
        path: PathBuf,
    },
    /// `list` could not parse the config text.
    #[error(
        "{reason}. The models were not listed. Fix the config so it parses as TOML, or pass --config with a readable file."
    )]
    ListConfigRefused {
        /// Why the text could not be read.
        #[source]
        reason: RefuseReason,
    },
    /// `list --limit` was not a whole number.
    #[error(
        "--limit {raw} is not a whole number, so nothing was listed. Omit --limit or pass a number greater than zero."
    )]
    LimitNotWhole {
        /// The text that was passed.
        raw: String,
    },
    /// `list --limit 0` would hide every row.
    #[error(
        "--limit 0 would hide every row, so nothing was listed. Omit --limit or pass a number greater than zero."
    )]
    LimitZero,
    /// `show` was given an empty slug.
    #[error("the slug is empty, so the model was not shown. Pass a configured or built-in slug.")]
    ShowSlugEmpty,
    /// `show` was given an empty `--config`.
    #[error("--config is empty, so the model was not shown. Pass --config with the file to read.")]
    ShowConfigEmpty,
    /// `show` has no home path.
    #[error(
        "HOME is empty and GROK_HOME is unset, so there is no config path. Pass --config with the file to read."
    )]
    ShowHomeEmpty,
    /// `show` has no home variable.
    #[error(
        "HOME is not set and GROK_HOME is unset, so there is no config path. Pass --config with the file to read."
    )]
    ShowHomeUnset,
    /// `show` was pointed at a file that is not there.
    #[error(
        "could not read config {}. The file is missing, so the model was not shown. Create the file or pass --config with an existing file.",
        path.display()
    )]
    ShowConfigMissing {
        /// Path that was not found.
        path: PathBuf,
    },
    /// `show` could not read the config.
    #[error(
        "could not read config {}. {source}. The model was not shown. Pass --config pointing at a readable file.",
        path.display()
    )]
    ShowConfigIo {
        /// Path that could not be read.
        path: PathBuf,
        /// The filesystem error.
        #[source]
        source: io::Error,
    },
    /// `show` read bytes that are not UTF-8.
    #[error(
        "config {} is not UTF-8, so the model was not shown. Save it as UTF-8 and pass that --config again.",
        path.display()
    )]
    ShowConfigUtf8 {
        /// Path that was not UTF-8.
        path: PathBuf,
    },
    /// `show` could not use the config text.
    #[error("{}", show_refused_sentence(reason))]
    ShowConfigRefused {
        /// Why the model was not shown.
        #[source]
        reason: RefuseReason,
    },
    /// `set-default` was given an empty `--config`.
    #[error(
        "--config is empty, so the saved default was not changed. Pass --config with the file to edit."
    )]
    SetDefaultConfigEmpty,
    /// `set-default` was not given `--config`.
    #[error(
        "no --config path was given, so the saved default was not changed. Pass --config with the file to edit."
    )]
    SetDefaultConfigOmitted,
    /// `set-default` was given an empty slug.
    #[error(
        "the slug is empty, so the saved default was not changed. Pass a built-in or configured slug."
    )]
    SetDefaultSlugEmpty,
    /// `set-default` could not read the config.
    #[error(
        "could not read config {}. {source}. The saved default was not changed. Pass --config pointing at an existing file.",
        path.display()
    )]
    SetDefaultConfigIo {
        /// Path that could not be read.
        path: PathBuf,
        /// The filesystem error.
        #[source]
        source: io::Error,
    },
    /// `set-default` read bytes that are not UTF-8.
    #[error(
        "config {} is not UTF-8. The saved default was not changed. Save it as UTF-8 and pass that --config again.",
        path.display()
    )]
    SetDefaultConfigUtf8 {
        /// Path that was not UTF-8.
        path: PathBuf,
    },
    /// `set-default` refused the text edit.
    #[error("{}", set_default_edit_sentence(reason))]
    SetDefaultEdit {
        /// Why the edit was refused.
        #[source]
        reason: RefuseReason,
    },
    /// `set-default` could not replace the file.
    #[error(
        "{source}. The saved default was not changed. Retry with a writable --config, or pass --dry-run to preview the slug."
    )]
    SetDefaultReplace {
        /// The replace that failed.
        #[source]
        source: ReplaceError,
    },
    /// `remove` was not confirmed.
    #[error(
        "{slug} was not deleted. Nothing was written. To delete that model block, pass --yes. To preview, pass --dry-run."
    )]
    RemoveNeedsYes {
        /// Slug that was left in place.
        slug: String,
    },
    /// `remove` was given an empty slug.
    #[error("an empty slug was given. Nothing was deleted. Pass the slug of a model block.")]
    RemoveSlugEmpty,
    /// `remove` was given an empty `--config`.
    #[error("--config is empty, so nothing was deleted. Pass --config with the file to edit.")]
    RemoveConfigEmpty,
    /// `remove` was not given `--config`.
    #[error(
        "no --config path was given, so nothing was deleted. Pass --config with the file to edit."
    )]
    RemoveConfigOmitted,
    /// `remove` could not read the config.
    #[error(
        "could not read config {}. {source}. Nothing was deleted. Pass --config pointing at an existing file.",
        path.display()
    )]
    RemoveConfigIo {
        /// Path that could not be read.
        path: PathBuf,
        /// The filesystem error.
        #[source]
        source: io::Error,
    },
    /// `remove` read bytes that are not UTF-8.
    #[error(
        "config {} is not UTF-8. Nothing was deleted. Save it as UTF-8 and pass that --config again.",
        path.display()
    )]
    RemoveConfigUtf8 {
        /// Path that was not UTF-8.
        path: PathBuf,
    },
    /// `remove` refused the text edit.
    #[error("{}", remove_edit_sentence(reason))]
    RemoveEdit {
        /// Why the edit was refused.
        #[source]
        reason: RefuseReason,
    },
    /// `remove` could not replace the file.
    #[error(
        "{source}. The config was not replaced. Retry with a writable --config, or pass --dry-run to preview the deletion."
    )]
    RemoveReplace {
        /// The replace that failed.
        #[source]
        source: ReplaceError,
    },
    /// `edit` was given an empty `--config`.
    #[error("--config is empty, so nothing was written. Pass --config with the file to edit.")]
    EditConfigEmpty,
    /// `edit` was not given `--config`.
    #[error(
        "no --config path was given, so nothing was written. Pass --config with the file to edit."
    )]
    EditConfigOmitted,
    /// `edit` was given an empty slug.
    #[error("the slug is empty, so nothing was written. Pass the slug of a model block.")]
    EditSlugEmpty,
    /// `edit` was given no field to change.
    #[error(
        "no field was given, so nothing was written. Pass a field to change, or --clear a field. The file was left unchanged."
    )]
    EditNoFields,
    /// A boolean edit flag was not `true` or `false`.
    #[error(
        "--{flag} {raw} is not true or false, so nothing was written. Pass true or false. The file was left unchanged."
    )]
    EditBool {
        /// Flag name without the leading dashes.
        flag: &'static str,
        /// The text that was passed.
        raw: String,
    },
    /// `--clear` named a field this command does not edit.
    #[error(
        "--clear {key} is not an editable model field, so nothing was written. The file was left unchanged."
    )]
    EditClearUnknown {
        /// Key that was not recognized.
        key: String,
    },
    /// One key was both set and cleared.
    #[error(
        "{key} was both set and cleared, so nothing was written. Pass the new value or --clear, not both. The file was left unchanged."
    )]
    EditClearAndSet {
        /// Key that was given two actions.
        key: String,
    },
    /// `edit` could not read the config.
    #[error(
        "could not read config {}. {source}. Nothing was written. Pass --config pointing at an existing file.",
        path.display()
    )]
    EditConfigIo {
        /// Path that could not be read.
        path: PathBuf,
        /// The filesystem error.
        #[source]
        source: io::Error,
    },
    /// `edit` read bytes that are not UTF-8.
    #[error(
        "config {} is not UTF-8. Nothing was written. Save it as UTF-8 and pass that --config again.",
        path.display()
    )]
    EditConfigUtf8 {
        /// Path that was not UTF-8.
        path: PathBuf,
    },
    /// `edit` refused the text edit.
    #[error("{}", edit_edit_sentence(reason))]
    EditEdit {
        /// Why the edit was refused.
        #[source]
        reason: RefuseReason,
    },
    /// `edit` could not replace the file.
    #[error(
        "{source}. The config was not replaced. Retry with a writable --config, or pass --dry-run to preview the edit."
    )]
    EditReplace {
        /// The replace that failed.
        #[source]
        source: ReplaceError,
    },
    /// Some direct-add flags were passed and some were not.
    #[error(
        "{present} {present_verb} set, but {missing} {missing_verb} missing. Pass all four of --model, --base-url, --env-key, and --api-backend, or none of them. The file was left unchanged. Nothing was downloaded. Nothing was written."
    )]
    PartialDirect {
        /// Flags that were passed.
        present: String,
        /// `was` or `were`, matching `present`.
        present_verb: &'static str,
        /// Flags that were not passed.
        missing: String,
        /// `was` or `were`, matching `missing`.
        missing_verb: &'static str,
    },
    /// `--name`, `--context-window`, or `--vision` without the four direct flags.
    #[error(
        "{extras} {verb} only used together with --model, --base-url, --env-key, and --api-backend. The file was left unchanged. Nothing was downloaded. Nothing was written."
    )]
    CatalogFlags {
        /// Flags that do not belong on the catalog path.
        extras: String,
        /// `is` or `are`, matching `extras`.
        verb: &'static str,
    },
    /// Catalog selectors were passed to a direct add.
    #[error(
        "{conflicts} cannot be combined with --model, --base-url, --env-key, and --api-backend. The file was left unchanged. Nothing was downloaded. Nothing was written."
    )]
    DirectConflict {
        /// Selectors that conflict with the direct flags.
        conflicts: String,
    },
    /// `--as` was present and empty.
    #[error(
        "--as is empty, so nothing was written. Pass --as with a slug. The file was left unchanged. Nothing was downloaded. Nothing was written."
    )]
    AsEmpty,
    /// A direct add omitted `--as`.
    #[error(
        "--as is required with --model, --base-url, --env-key, and --api-backend. Pass --as with the slug to write. The file was left unchanged. Nothing was downloaded. Nothing was written."
    )]
    AsRequired,
    /// `add` could not replace the file.
    #[error(
        "{source}. The config was not replaced. Retry with a writable --config, or pass --dry-run to preview the edit."
    )]
    AddReplace {
        /// The replace that failed.
        #[source]
        source: ReplaceError,
    },
    /// A catalog match did not select one row. Nothing was written.
    #[error("{source} Nothing was written.")]
    ResolveUnwritten {
        /// The match that failed.
        #[source]
        source: ResolveError,
    },
    /// `add` was given an empty `--config`.
    #[error("--config is empty, so nothing was written. Pass --config with the file to edit.")]
    AddConfigEmpty,
    /// `add` was not given `--config`.
    #[error(
        "no --config path was given, so nothing was written. Pass --config with the file to edit."
    )]
    AddConfigOmitted,
    /// A provider record could not be resolved. Nothing was downloaded or written.
    #[error("{}", provider_resolve_sentence(reason))]
    ProviderResolve {
        /// Why the record was not used.
        #[source]
        reason: RefuseReason,
    },
    /// The provider record has no catalog id.
    #[error(
        "{provider} has no catalog cartridge configured. Nothing was downloaded. Nothing was written. Set catalog to openrouter-models or deepseek-list-models, or pass --provider for a record that already has one."
    )]
    CartridgeMissing {
        /// Provider name used in the sentence.
        provider: String,
        /// The cartridge error the command wrapped.
        #[source]
        source: CartridgeError,
    },
    /// The provider record names an unknown catalog id.
    #[error("{}", cartridge_unknown_sentence(provider, source))]
    CartridgeUnknown {
        /// Provider name used in the sentence.
        provider: String,
        /// The cartridge error the command wrapped.
        #[source]
        source: CartridgeError,
    },
    /// Built-in membership failed before a catalog read.
    #[error("{reason}. The file was left unchanged. Nothing was downloaded. Nothing was written.")]
    AddEarlyRefusal {
        /// Why membership could not be read.
        #[source]
        reason: RefuseReason,
    },
    /// `add` was given an empty query term.
    #[error("an empty query was given. Nothing was written. Pass a provider id or terms.")]
    EmptyQuery,
    /// `add` could not read `--catalog-file`.
    #[error(
        "could not read catalog file {}. {source}. Nothing was written. Pass a readable --catalog-file, or omit it to use the cache.",
        path.display()
    )]
    AddCatalogFile {
        /// Path that could not be read.
        path: PathBuf,
        /// The filesystem error.
        #[source]
        source: io::Error,
    },
    /// A catalog failure stopped `add` before a write.
    #[error("{source} Nothing was written.")]
    CatalogUnwritten {
        /// The catalog error.
        #[source]
        source: CatalogError,
    },
    /// A catalog failure stopped the command before the config was written.
    #[error("{source} The config was not written.")]
    CatalogConfigUnwritten {
        /// The catalog error.
        #[source]
        source: CatalogError,
    },
    /// The cache file landed outside the cache directory.
    #[error(
        "the catalog cache was written to {}, outside {}. The config was not written. Pass --cache-dir.",
        stored.display(),
        cache_dir.display()
    )]
    CacheOutside {
        /// Path that was written.
        stored: PathBuf,
        /// Directory the cache should have stayed inside.
        cache_dir: PathBuf,
    },
    /// The bearer variable is missing or blank.
    #[error(
        "{env_key} is missing, so the catalog was not downloaded. Export {env_key} or pass --catalog-file. The config was not written and an empty cache was not written."
    )]
    MissingBearer {
        /// Variable the record named.
        env_key: String,
        /// The catalog failure, when the download path produced one.
        #[source]
        source: CatalogError,
    },
    /// `add` found a bearer that is not Unicode.
    #[error(
        "{name} is not valid Unicode, so the config was not written. Export a Unicode key or pass --catalog-file."
    )]
    AddEnvKeyNotUnicode {
        /// Variable that could not be read.
        name: String,
    },
    /// `search` found a bearer that is not Unicode.
    #[error(
        "{name} is not valid Unicode, so the catalog was not downloaded. Export a Unicode key or pass --catalog-file."
    )]
    SearchEnvKeyNotUnicode {
        /// Variable that could not be read.
        name: String,
    },
    /// `add` has no home for the default cache directory.
    #[error(
        "HOME is not set, so the catalog cache has no default directory. Nothing was written. Pass --cache-dir or --catalog-file."
    )]
    AddCacheHomeUnset,
    /// `add` has an empty home for the default cache directory.
    #[error(
        "HOME is empty, so the catalog cache has no default directory. Nothing was written. Pass --cache-dir or --catalog-file."
    )]
    AddCacheHomeEmpty,
    /// `add` could not read the config.
    #[error(
        "could not read config {}. {source}. Nothing was written. Pass --config pointing at an existing file.",
        path.display()
    )]
    AddConfigIo {
        /// Path that could not be read.
        path: PathBuf,
        /// The filesystem error.
        #[source]
        source: io::Error,
    },
    /// `add` read bytes that are not UTF-8.
    #[error(
        "config {} is not UTF-8. Nothing was written. Save it as UTF-8 and pass that --config again.",
        path.display()
    )]
    AddConfigUtf8 {
        /// Path that was not UTF-8.
        path: PathBuf,
    },
    /// `add` refused the text edit.
    #[error("{}", add_edit_sentence(reason))]
    AddEdit {
        /// Why the edit was refused.
        #[source]
        reason: RefuseReason,
    },
    /// `search` was given an empty term.
    #[error(
        "an empty search term was given. Remove the empty argument and pass words or an exact model id."
    )]
    SearchEmptyTerm,
    /// `search` could not read `--catalog-file`.
    #[error(
        "could not read catalog file {}. {source}. Nothing was downloaded. The config was not written. Pass a readable --catalog-file, or omit it to use the cache.",
        path.display()
    )]
    SearchCatalogFile {
        /// Path that could not be read.
        path: PathBuf,
        /// The filesystem error.
        #[source]
        source: io::Error,
    },
    /// `--catalog-file` did not parse. Nothing was downloaded and the config was not written.
    #[error("{source} Nothing was downloaded. The config was not written.")]
    SearchCatalogParse {
        /// The catalog error.
        #[source]
        source: CatalogError,
    },
    /// `search` was given an empty `--config`.
    #[error("--config is empty, so the catalog was not read. Pass --config with the file to read.")]
    SearchConfigEmpty,
    /// `search` has no home path.
    #[error(
        "HOME is empty and GROK_HOME is unset, so there is no config path. The catalog was not read. Pass --config."
    )]
    SearchHomeEmpty,
    /// `search` has no home variable.
    #[error(
        "HOME is not set and GROK_HOME is unset, so there is no config path. The catalog was not read. Pass --config."
    )]
    SearchHomeUnset,
    /// `search` could not read the config.
    #[error(
        "could not read config {}. {source}. The catalog was not read. Nothing was downloaded and the cache was not written. Pass --config pointing at a readable file.",
        path.display()
    )]
    SearchConfigIo {
        /// Path that could not be read.
        path: PathBuf,
        /// The filesystem error.
        #[source]
        source: io::Error,
    },
    /// `search` read bytes that are not UTF-8.
    #[error(
        "config {} is not UTF-8, so the catalog was not read. Nothing was downloaded and the cache was not written. Save it as UTF-8.",
        path.display()
    )]
    SearchConfigUtf8 {
        /// Path that was not UTF-8.
        path: PathBuf,
    },
    /// `search` has no home for the default cache directory.
    #[error(
        "HOME is not set, so the catalog cache has no default directory. Pass --cache-dir or --catalog-file."
    )]
    SearchCacheHomeUnset,
    /// `search` has an empty home for the default cache directory.
    #[error(
        "HOME is empty, so the catalog cache has no default directory. Pass --cache-dir or --catalog-file."
    )]
    SearchCacheHomeEmpty,
    /// No catalog row matched the search terms.
    #[error(
        "no models matched {quoted}. 0 matches. The id, name, and description did not contain every term. Try fewer terms or pass --catalog-file with the catalog you meant."
    )]
    SearchEmpty {
        /// Terms as the sentence already quotes them, or `the catalog`.
        quoted: String,
    },
    /// `skill` was not given `--skill-root`.
    #[error(
        "no --skill-root was given, so nothing was written. Pass --skill-root with the directory that should hold the grok-router stub."
    )]
    SkillRootOmitted,
    /// `skill` was given an empty `--skill-root`.
    #[error("--skill-root is empty, so nothing was written. Pass --skill-root with a directory.")]
    SkillRootEmpty,
    /// `init` was given an empty `--config`.
    #[error(
        "--config is empty, so nothing was written. Pass --config with the file to write, or omit --config to use the default path."
    )]
    InitConfigEmpty,
    /// `init` has no home path.
    #[error(
        "HOME is empty and GROK_HOME is unset, so there is no config path. Nothing was written. Pass --config with the file to write."
    )]
    InitHomeEmpty,
    /// `init` has no home variable.
    #[error(
        "HOME is not set and GROK_HOME is unset, so there is no config path. Nothing was written. Pass --config with the file to write."
    )]
    InitHomeUnset,
    /// `init` could not read an existing config.
    #[error(
        "could not read config {}. {source}. Nothing was written. Make that file readable, or pass --config with a readable file.",
        path.display()
    )]
    InitConfigIo {
        /// Path that could not be read.
        path: PathBuf,
        /// The filesystem error.
        #[source]
        source: io::Error,
    },
    /// `init` read bytes that are not UTF-8.
    #[error(
        "config {} is not UTF-8, so nothing was written. The file was left unchanged. Save it as UTF-8 and retry init.",
        path.display()
    )]
    InitConfigUtf8 {
        /// Path that was not UTF-8.
        path: PathBuf,
    },
    /// `init` refused because the config text did not parse.
    #[error("{}", init_parse_sentence(path, *appended, detail))]
    InitConfigParse {
        /// Path that was left unchanged.
        path: PathBuf,
        /// Parser message.
        detail: String,
        /// True when the failure is the text after the block was appended.
        appended: bool,
    },
    /// `init` found a `[router]` key that is not a table.
    #[error(
        "config {} has a [router] key that is not a table, so nothing was written. The file was left unchanged. Make [router] a table or remove that key, then retry init.",
        path.display()
    )]
    InitRouterNotTable {
        /// Path that was left unchanged.
        path: PathBuf,
    },
    /// `init` would change a top-level table that was already in the file.
    #[error("{}", init_changed_sentence(tables))]
    InitChangedTables {
        /// Table names that would change.
        tables: Vec<String>,
    },
    /// `init` could not create or replace the file.
    #[error(
        "{source}. The config was not replaced. Retry with a writable path, or pass --dry-run to preview the write."
    )]
    InitReplace {
        /// The create or replace that failed.
        #[source]
        source: ReplaceError,
    },
    /// A refused config edit, printed as that refusal.
    #[error(transparent)]
    Refused(#[from] RefuseReason),
    /// A file replace, printed as that failure.
    #[error(transparent)]
    Replace(#[from] ReplaceError),
    /// A catalog failure, printed as that failure.
    #[error(transparent)]
    Catalog(#[from] CatalogError),
    /// A catalog match failure, printed as that failure.
    #[error(transparent)]
    Resolve(#[from] ResolveError),
    /// A cartridge failure, printed as that failure.
    #[error(transparent)]
    Cartridge(#[from] CartridgeError),
    /// A skill failure. Kind stays `refused` or `io`.
    #[error(transparent)]
    Skill(#[from] SkillFailure),
    /// A named secret blocked the write.
    #[error(transparent)]
    Secret(#[from] SecretError),
    /// `--timeout` was not a positive whole number of seconds.
    #[error(transparent)]
    Timeout(#[from] TimeoutError),
}

impl Error {
    /// Kind string this path emits.
    ///
    /// JSON `error` and `kind` are this name. A transparent domain error uses
    /// that error's own name. [`SkillFailure`] stays `refused` or `io`.
    #[must_use]
    #[allow(
        clippy::too_many_lines,
        reason = "one arm per error variant keeps a missing kind string a compile error"
    )]
    pub fn kind(&self) -> &'static str {
        match self {
            Self::JsonFlagConflicts { .. } => "json_flag_conflicts",
            Self::QuietConflictsJson => "quiet_conflicts_json",
            Self::UnknownShell { .. } => "unknown_shell",
            Self::CompletionNotUtf8 => "completion_not_utf8",
            Self::JsonEncode { .. } => "json_encode",
            Self::Stdout { .. } => "stdout",
            Self::Usage { .. } => "usage",
            Self::ListConfigEmpty => "list_config_empty",
            Self::ListHomeEmpty => "list_home_empty",
            Self::ListHomeUnset => "list_home_unset",
            Self::ListConfigMissing { .. } => "list_config_missing",
            Self::ListConfigIo { .. } => "list_config_io",
            Self::ListConfigUtf8 { .. } => "list_config_utf8",
            Self::ListConfigRefused { .. } => "list_config_refused",
            Self::LimitNotWhole { .. } => "limit_not_whole",
            Self::LimitZero => "limit_zero",
            Self::ShowSlugEmpty => "show_slug_empty",
            Self::ShowConfigEmpty => "show_config_empty",
            Self::ShowHomeEmpty => "show_home_empty",
            Self::ShowHomeUnset => "show_home_unset",
            Self::ShowConfigMissing { .. } => "show_config_missing",
            Self::ShowConfigIo { .. } => "show_config_io",
            Self::ShowConfigUtf8 { .. } => "show_config_utf8",
            Self::ShowConfigRefused { .. } => "show_config_refused",
            Self::SetDefaultConfigEmpty => "set_default_config_empty",
            Self::SetDefaultConfigOmitted => "set_default_config_omitted",
            Self::SetDefaultSlugEmpty => "set_default_slug_empty",
            Self::SetDefaultConfigIo { .. } => "set_default_config_io",
            Self::SetDefaultConfigUtf8 { .. } => "set_default_config_utf8",
            Self::SetDefaultEdit { .. } => "set_default_edit",
            Self::SetDefaultReplace { .. } => "set_default_replace",
            Self::RemoveNeedsYes { .. } => "remove_needs_yes",
            Self::RemoveSlugEmpty => "remove_slug_empty",
            Self::RemoveConfigEmpty => "remove_config_empty",
            Self::RemoveConfigOmitted => "remove_config_omitted",
            Self::RemoveConfigIo { .. } => "remove_config_io",
            Self::RemoveConfigUtf8 { .. } => "remove_config_utf8",
            Self::RemoveEdit { .. } => "remove_edit",
            Self::RemoveReplace { .. } => "remove_replace",
            Self::EditConfigEmpty => "edit_config_empty",
            Self::EditConfigOmitted => "edit_config_omitted",
            Self::EditSlugEmpty => "edit_slug_empty",
            Self::EditNoFields => "edit_no_fields",
            Self::EditBool { .. } => "edit_bool",
            Self::EditClearUnknown { .. } => "edit_clear_unknown",
            Self::EditClearAndSet { .. } => "edit_clear_and_set",
            Self::EditConfigIo { .. } => "edit_config_io",
            Self::EditConfigUtf8 { .. } => "edit_config_utf8",
            Self::EditEdit { .. } => "edit_edit",
            Self::EditReplace { .. } => "edit_replace",
            Self::PartialDirect { .. } => "partial_direct",
            Self::CatalogFlags { .. } => "catalog_flags",
            Self::DirectConflict { .. } => "direct_conflict",
            Self::AsEmpty => "as_empty",
            Self::AsRequired => "as_required",
            Self::AddReplace { .. } => "add_replace",
            Self::ResolveUnwritten { .. } => "resolve_unwritten",
            Self::AddConfigEmpty => "add_config_empty",
            Self::AddConfigOmitted => "add_config_omitted",
            Self::ProviderResolve { .. } => "provider_resolve",
            Self::CartridgeMissing { .. } => "cartridge_missing",
            Self::CartridgeUnknown { .. } => "cartridge_unknown",
            Self::AddEarlyRefusal { .. } => "add_early_refusal",
            Self::EmptyQuery => "empty_query",
            Self::AddCatalogFile { .. } => "add_catalog_file",
            Self::CatalogUnwritten { .. } => "catalog_unwritten",
            Self::CatalogConfigUnwritten { .. } => "catalog_config_unwritten",
            Self::CacheOutside { .. } => "cache_outside",
            Self::MissingBearer { .. } => "missing_bearer",
            Self::AddEnvKeyNotUnicode { .. } => "add_env_key_not_unicode",
            Self::SearchEnvKeyNotUnicode { .. } => "search_env_key_not_unicode",
            Self::AddCacheHomeUnset => "add_cache_home_unset",
            Self::AddCacheHomeEmpty => "add_cache_home_empty",
            Self::AddConfigIo { .. } => "add_config_io",
            Self::AddConfigUtf8 { .. } => "add_config_utf8",
            Self::AddEdit { .. } => "add_edit",
            Self::SearchEmptyTerm => "search_empty_term",
            Self::SearchCatalogFile { .. } => "search_catalog_file",
            Self::SearchCatalogParse { .. } => "search_catalog_parse",
            Self::SearchConfigEmpty => "search_config_empty",
            Self::SearchHomeEmpty => "search_home_empty",
            Self::SearchHomeUnset => "search_home_unset",
            Self::SearchConfigIo { .. } => "search_config_io",
            Self::SearchConfigUtf8 { .. } => "search_config_utf8",
            Self::SearchCacheHomeUnset => "search_cache_home_unset",
            Self::SearchCacheHomeEmpty => "search_cache_home_empty",
            Self::SearchEmpty { .. } => "search_empty",
            Self::SkillRootOmitted => "skill_root_omitted",
            Self::SkillRootEmpty => "skill_root_empty",
            Self::InitConfigEmpty => "init_config_empty",
            Self::InitHomeEmpty => "init_home_empty",
            Self::InitHomeUnset => "init_home_unset",
            Self::InitConfigIo { .. } => "init_config_io",
            Self::InitConfigUtf8 { .. } => "init_config_utf8",
            Self::InitConfigParse { .. } => "init_config_parse",
            Self::InitRouterNotTable { .. } => "init_router_not_table",
            Self::InitChangedTables { .. } => "init_changed_tables",
            Self::InitReplace { .. } => "init_replace",
            Self::Refused(reason) => reason.kind(),
            Self::Replace(source) => source.failure_kind(),
            Self::Catalog(source) => source.kind(),
            Self::Resolve(source) => source.kind(),
            Self::Cartridge(source) => source.kind(),
            Self::Skill(failure) => failure.kind(),
            Self::Secret(source) => source.kind(),
            Self::Timeout(_) => "timeout",
        }
    }
}

/// `before writing` or `after the router block was appended`.
fn init_parse_when(appended: bool) -> &'static str {
    if appended {
        "after the router block was appended"
    } else {
        "before writing"
    }
}

/// Parse refusal for `init`. Names the path, the consequence, and the next action.
fn init_parse_sentence(path: &Path, appended: bool, detail: &str) -> String {
    format!(
        "config {} did not parse as TOML {when}, so nothing was written. The file was left unchanged. Fix the config so it parses as TOML and retry init. {detail}",
        path.display(),
        when = init_parse_when(appended)
    )
}

/// Changed-table refusal for `init`. Names the tables, the consequence, and the next action.
fn init_changed_sentence(tables: &[String]) -> String {
    format!(
        "init would change existing config tables ({}). Nothing was written. The file was left unchanged. Keep those tables and retry init.",
        tables.join(", ")
    )
}

/// Trim clap's display the way the JSON usage sentence already does.
fn clap_sentence(err: &clap::Error) -> String {
    err.to_string().trim().to_owned()
}

/// `show` refusal, including the next action.
fn show_refused_sentence(reason: &RefuseReason) -> String {
    format!("{reason}. The model was not shown. {}", show_next(reason))
}

/// The next action for a refused `show`.
fn show_next(reason: &RefuseReason) -> &'static str {
    match reason {
        RefuseReason::NotConfigured { .. } => "Pass a slug from list, or a built-in slug.",
        RefuseReason::InvalidSlug { .. } => {
            "Pass a slug that starts with a letter or digit and contains only letters, digits, dots, underscores, and hyphens."
        }
        RefuseReason::DuplicateSlugHeader { .. } => "Leave one header for that slug and retry.",
        RefuseReason::Parse { .. } | RefuseReason::MultipleModelsTables => {
            "Fix the config so it parses as TOML and retry."
        }
        RefuseReason::EmptyProviderId
        | RefuseReason::EmptySlugLeaf { .. }
        | RefuseReason::NonPositiveContext { .. }
        | RefuseReason::DuplicateProvider { .. }
        | RefuseReason::SlugInUse { .. }
        | RefuseReason::BuiltIn { .. }
        | RefuseReason::IsDefault { .. }
        | RefuseReason::UnknownSlug { .. }
        | RefuseReason::DroppedTables { .. }
        | RefuseReason::ChangedProvider { .. }
        | RefuseReason::AddMissing { .. }
        | RefuseReason::RemoveFailed { .. }
        | RefuseReason::DefaultMismatch { .. }
        | RefuseReason::InvalidSetting { .. } => "Fix the config and retry show.",
    }
}

/// `edit` refusal, including the next action.
fn edit_edit_sentence(reason: &RefuseReason) -> String {
    format!(
        "{reason}. The file was left unchanged. {}",
        edit_next(reason)
    )
}

/// The next action for a refused `edit`.
fn edit_next(reason: &RefuseReason) -> &'static str {
    match reason {
        RefuseReason::NotConfigured { .. } => {
            "Pass a slug that appears as a [model.<slug>] block in this file."
        }
        RefuseReason::BuiltIn { .. } => {
            "Pass a configured slug. Built-ins have no config block to edit."
        }
        RefuseReason::InvalidSlug { .. } => {
            "Pass a slug that starts with a letter or digit and contains only letters, digits, dots, underscores, and hyphens."
        }
        RefuseReason::InvalidSetting { .. } => {
            "Fix that field and retry. Pass --dry-run to preview a valid edit."
        }
        RefuseReason::DuplicateSlugHeader { .. } => "Leave one header for that slug and retry.",
        RefuseReason::Parse { .. } | RefuseReason::MultipleModelsTables => {
            "Fix the config so it parses as TOML and retry."
        }
        RefuseReason::EmptyProviderId
        | RefuseReason::EmptySlugLeaf { .. }
        | RefuseReason::NonPositiveContext { .. }
        | RefuseReason::DuplicateProvider { .. }
        | RefuseReason::SlugInUse { .. }
        | RefuseReason::IsDefault { .. }
        | RefuseReason::UnknownSlug { .. }
        | RefuseReason::DroppedTables { .. }
        | RefuseReason::ChangedProvider { .. }
        | RefuseReason::AddMissing { .. }
        | RefuseReason::RemoveFailed { .. }
        | RefuseReason::DefaultMismatch { .. } => {
            "Retry with --dry-run after fixing the config. This command did not replace the file."
        }
    }
}

/// `set-default` refusal, including the next action.
fn set_default_edit_sentence(reason: &RefuseReason) -> String {
    format!(
        "{reason}. The file was left unchanged. {}",
        set_default_next(reason)
    )
}

/// The next action for a refused `set-default`.
fn set_default_next(reason: &RefuseReason) -> &'static str {
    match reason {
        RefuseReason::UnknownSlug { .. } => {
            "Pass a built-in slug (grok-4.7, grok-4.7-build-fast, grok-4.6, grok-4.5) or a slug already in the config."
        }
        RefuseReason::InvalidSlug { .. } => {
            "Pass a slug that starts with a letter or digit and contains only letters, digits, dots, underscores, and hyphens."
        }
        RefuseReason::MultipleModelsTables => "Leave one [models] table in the config and retry.",
        RefuseReason::DuplicateSlugHeader { .. } => "Leave one header for that slug and retry.",
        RefuseReason::Parse { .. } => "Fix the config so it parses as TOML and retry.",
        RefuseReason::EmptyProviderId
        | RefuseReason::EmptySlugLeaf { .. }
        | RefuseReason::NonPositiveContext { .. }
        | RefuseReason::DuplicateProvider { .. }
        | RefuseReason::SlugInUse { .. }
        | RefuseReason::NotConfigured { .. }
        | RefuseReason::BuiltIn { .. }
        | RefuseReason::IsDefault { .. }
        | RefuseReason::DroppedTables { .. }
        | RefuseReason::ChangedProvider { .. }
        | RefuseReason::AddMissing { .. }
        | RefuseReason::RemoveFailed { .. }
        | RefuseReason::DefaultMismatch { .. }
        | RefuseReason::InvalidSetting { .. } => {
            "Retry with --dry-run after fixing the config. This command did not replace the file."
        }
    }
}

/// `remove` refusal, including the next action.
fn remove_edit_sentence(reason: &RefuseReason) -> String {
    format!(
        "{reason}. The file was left unchanged. {}",
        remove_next(reason)
    )
}

/// The next action for a refused removal.
fn remove_next(reason: &RefuseReason) -> String {
    match reason {
        RefuseReason::IsDefault { .. } => "Set another default, then remove this slug.".to_owned(),
        RefuseReason::BuiltIn { .. } => {
            "Pass a configured slug. Built-ins have no config block to delete.".to_owned()
        }
        RefuseReason::NotConfigured { .. } => {
            "Pass a slug that appears as a [model.<slug>] block in this file.".to_owned()
        }
        RefuseReason::InvalidSlug { .. } => {
            "Pass a slug that starts with a letter or digit and contains only letters, digits, dots, underscores, and hyphens.".to_owned()
        }
        RefuseReason::MultipleModelsTables => {
            "Leave one [models] table in the config and retry.".to_owned()
        }
        RefuseReason::DuplicateSlugHeader { .. } => "Leave one header for that slug and retry.".to_owned(),
        RefuseReason::Parse { .. } => "Fix the config so it parses as TOML and retry.".to_owned(),
        RefuseReason::EmptyProviderId
        | RefuseReason::EmptySlugLeaf { .. }
        | RefuseReason::NonPositiveContext { .. }
        | RefuseReason::DuplicateProvider { .. }
        | RefuseReason::SlugInUse { .. }
        | RefuseReason::UnknownSlug { .. }
        | RefuseReason::DroppedTables { .. }
        | RefuseReason::ChangedProvider { .. }
        | RefuseReason::AddMissing { .. }
        | RefuseReason::RemoveFailed { .. }
        | RefuseReason::DefaultMismatch { .. }
        | RefuseReason::InvalidSetting { .. } => {
            "Retry with --dry-run after fixing the config. This command did not replace the file."
                .to_owned()
        }
    }
}

/// `add` refusal, including the next action.
fn add_edit_sentence(reason: &RefuseReason) -> String {
    format!(
        "{reason}. The file was left unchanged. {}",
        add_next(reason)
    )
}

/// The next action for a refused add.
fn add_next(reason: &RefuseReason) -> String {
    match reason {
        RefuseReason::EmptyProviderId => "Choose a catalog row that has an id.".to_owned(),
        RefuseReason::EmptySlugLeaf { .. } => "Pass --as with a slug.".to_owned(),
        RefuseReason::InvalidSlug { .. } => {
            "Pass --as with a slug that starts with a letter or digit and contains only letters, digits, dots, underscores, and hyphens.".to_owned()
        }
        RefuseReason::NonPositiveContext { .. } => {
            "Choose a catalog row whose context window is greater than zero.".to_owned()
        }
        RefuseReason::DuplicateProvider { existing_slug, .. } => format!(
            "Keep {existing_slug}, or remove it before adding another slug for that provider."
        ),
        RefuseReason::SlugInUse { .. } => "Pass --as with a slug that is not already used.".to_owned(),
        RefuseReason::UnknownSlug { .. } => {
            "Add the model before passing --default, or choose a built-in or configured slug."
                .to_owned()
        }
        RefuseReason::MultipleModelsTables => "Leave one [models] table in the config and retry.".to_owned(),
        RefuseReason::DuplicateSlugHeader { .. } => "Leave one header for that slug and retry.".to_owned(),
        RefuseReason::Parse { .. } => "Fix the config so it parses as TOML and retry.".to_owned(),
        RefuseReason::NotConfigured { .. }
        | RefuseReason::BuiltIn { .. }
        | RefuseReason::IsDefault { .. }
        | RefuseReason::DroppedTables { .. }
        | RefuseReason::ChangedProvider { .. }
        | RefuseReason::AddMissing { .. }
        | RefuseReason::RemoveFailed { .. }
        | RefuseReason::DefaultMismatch { .. }
        | RefuseReason::InvalidSetting { .. } => {
            "Retry with --dry-run after fixing the config. This command did not replace the file."
                .to_owned()
        }
    }
}

/// Provider-record refusal. Some invalid settings tell the operator to write the record.
fn provider_resolve_sentence(reason: &RefuseReason) -> String {
    let preserved = "The file was left unchanged. Nothing was downloaded. Nothing was written.";
    let must_write = match reason {
        RefuseReason::InvalidSetting { key, detail }
            if key == "router"
                || key == "default_provider"
                || key == "provider"
                || detail.contains("[router]") =>
        {
            " The operator must write that record."
        }
        RefuseReason::EmptyProviderId
        | RefuseReason::EmptySlugLeaf { .. }
        | RefuseReason::InvalidSlug { .. }
        | RefuseReason::NonPositiveContext { .. }
        | RefuseReason::DuplicateProvider { .. }
        | RefuseReason::SlugInUse { .. }
        | RefuseReason::NotConfigured { .. }
        | RefuseReason::BuiltIn { .. }
        | RefuseReason::IsDefault { .. }
        | RefuseReason::UnknownSlug { .. }
        | RefuseReason::MultipleModelsTables
        | RefuseReason::DuplicateSlugHeader { .. }
        | RefuseReason::Parse { .. }
        | RefuseReason::DroppedTables { .. }
        | RefuseReason::ChangedProvider { .. }
        | RefuseReason::AddMissing { .. }
        | RefuseReason::RemoveFailed { .. }
        | RefuseReason::DefaultMismatch { .. }
        | RefuseReason::InvalidSetting { .. } => "",
    };
    format!("{reason}.{must_write} {preserved}")
}

/// Unknown-cartridge sentence. The id comes from the source when it is unknown.
fn cartridge_unknown_sentence(provider: &str, source: &CartridgeError) -> String {
    let id = match source {
        CartridgeError::Unknown { id } => id.as_str(),
        CartridgeError::Missing => "",
    };
    format!(
        "catalog {id} on {provider} is not a cartridge, so no catalog cartridge is configured. Nothing was downloaded. Nothing was written. Set catalog to openrouter-models or deepseek-list-models, or pass --provider for a record that already has one."
    )
}
