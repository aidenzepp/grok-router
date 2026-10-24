//! Splice Grok model blocks inside config text.
//!
//! The edit inserts, replaces, or deletes exact spans. It does not serialize
//! the document back out. After a splice, the text is parsed again and refused
//! when a top-level table, an existing provider id, the intended membership, or
//! the saved default drifts. Refusal returns the caller's original text.
//! [`read_configured`] reports the saved default and configured models and does
//! not change the text. [`router_settings`] reads `[grok_router]` from that
//! same text. A missing table or key keeps [`OPENROUTER_BASE_URL`] and
//! [`BUILTIN_SLUGS`]. [`direct_provider`] reads one
//! `[grok_router.provider.<name>]` table and returns owned strings. It does
//! not write the document. The edit never writes either table back.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};

/// OpenRouter base URL used when `openrouter_base_url` is absent.
pub const OPENROUTER_BASE_URL: &str = "https://openrouter.ai/api/v1";

/// Built-in Grok slugs used when `builtins` is absent. They have no config block.
pub const BUILTIN_SLUGS: [&str; 4] = [
    "grok-4.7",
    "grok-4.7-build-fast",
    "grok-4.6",
    "grok-4.5",
];

/// One configured model and the provider id stored in its block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfiguredModel {
    /// Slug under `[model]`.
    pub slug: String,
    /// Provider id from the block's `model` field.
    pub provider_id: String,
}

/// The saved default and every configured model, in slug order.
///
/// Built-in slugs are not inserted. A default with no block is only
/// [`Self::saved_default`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfiguredInventory {
    /// `[models].default` when that value is a string.
    pub saved_default: Option<String>,
    /// Configured slugs and their provider ids.
    pub models: Vec<ConfiguredModel>,
}

/// Catalog fields the splice reads. The caller supplies them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CatalogModel<'a> {
    /// Provider id, for example `z-ai/glm-5.3`.
    pub id: &'a str,
    /// Display name. Missing or blank falls back to [`Self::id`].
    pub name: Option<&'a str>,
    /// Catalog `context_length`. Missing or non-positive refuses the add.
    pub context_window: Option<i64>,
    /// Whether input modalities include `image`.
    pub sees_image: bool,
}

/// Resolved OpenRouter base and built-in slugs from `[grok_router]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouterSettings {
    /// `openrouter_base_url`, or [`OPENROUTER_BASE_URL`] when that key is absent.
    pub base_url: String,
    /// `builtins`, or [`BUILTIN_SLUGS`] when that key is absent. Empty means none.
    pub builtins: Vec<String>,
}

/// One `[grok_router.provider.<name>]` table, as owned strings.
///
/// Reading it does not rewrite the document, read the environment, or fetch a catalog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectProvider {
    /// `base_url`, kept exactly, including a trailing slash.
    pub base_url: String,
    /// `env_key`. This is the variable name, not its value.
    pub env_key: String,
    /// `catalog` when that key is present. An empty or unverified string stays that string.
    pub catalog: Option<String>,
}

/// Why a text edit was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefuseReason {
    /// The catalog row did not include a provider id.
    EmptyProviderId,
    /// The provider id's last path segment did not yield a slug.
    EmptySlugLeaf {
        /// Provider id that could not be turned into a slug.
        provider_id: String,
    },
    /// The slug does not match the allowed shape.
    InvalidSlug {
        /// Slug that was rejected.
        slug: String,
    },
    /// `context_window` was missing or not greater than zero.
    NonPositiveContext {
        /// Provider id whose context window was rejected.
        provider_id: String,
    },
    /// The provider id is already configured under a different slug.
    DuplicateProvider {
        /// Provider id that is already present.
        provider_id: String,
        /// Slug already bound to that provider id at the resolved base URL.
        existing_slug: String,
    },
    /// The slug is already a configured model or a built-in.
    SlugInUse {
        /// Slug that is already taken.
        slug: String,
    },
    /// Remove was asked for a slug that has no config block.
    NotConfigured {
        /// Slug that is absent from the config.
        slug: String,
    },
    /// Remove was asked for a built-in slug.
    BuiltIn {
        /// Built-in slug that has no block to delete.
        slug: String,
    },
    /// Remove was asked for the saved default.
    IsDefault {
        /// Slug that is `[models].default`.
        slug: String,
    },
    /// `set-default` was asked for a slug that is neither built-in nor configured.
    UnknownSlug {
        /// Slug that cannot become the default.
        slug: String,
    },
    /// The document has more than one `[models]` header.
    MultipleModelsTables,
    /// The document has more than one header for the same model slug.
    DuplicateSlugHeader {
        /// Slug whose header appears more than once.
        slug: String,
    },
    /// The document, or the spliced text, did not parse.
    Parse {
        /// Parser message. The original text is returned separately.
        detail: String,
    },
    /// Re-parse showed a top-level table that the splice dropped.
    DroppedTables {
        /// Table names present before the splice and absent after it.
        tables: Vec<String>,
    },
    /// Re-parse showed an existing model whose provider id changed.
    ChangedProvider {
        /// Slug whose provider id was not preserved.
        slug: String,
    },
    /// Re-parse did not find the model the splice claimed to add.
    AddMissing {
        /// Slug that should have been added.
        slug: String,
    },
    /// Re-parse still found the model the splice claimed to remove.
    RemoveFailed {
        /// Slug that should have been removed.
        slug: String,
    },
    /// Re-parse showed a different saved default than the splice required.
    DefaultMismatch {
        /// Default the splice required, if any.
        expected: Option<String>,
        /// Default the re-parse observed, if any.
        actual: Option<String>,
    },
    /// A present `[grok_router]` key cannot be used.
    InvalidSetting {
        /// `openrouter_base_url`, `builtins`, `base_url`, `env_key`, `catalog`, or a provider name.
        key: String,
        /// Constraint the value failed.
        detail: String,
    },
}

impl fmt::Display for RefuseReason {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyProviderId => write!(formatter, "catalog row has no id"),
            Self::EmptySlugLeaf { provider_id } => write!(
                formatter,
                "cannot derive a slug from {provider_id:?}; pass --as"
            ),
            Self::InvalidSlug { slug } => write!(
                formatter,
                "slug {slug:?} must start with a letter or digit and contain only letters, digits, dots, underscores, and hyphens"
            ),
            Self::NonPositiveContext { provider_id } => {
                write!(formatter, "{provider_id} has no context_length")
            }
            Self::DuplicateProvider {
                provider_id,
                existing_slug,
            } => write!(formatter, "{provider_id} is already {existing_slug}"),
            Self::SlugInUse { slug } => {
                write!(formatter, "slug {slug} is already in use; pass --as")
            }
            Self::NotConfigured { slug } => write!(formatter, "{slug} is not in the config"),
            Self::BuiltIn { slug } => {
                write!(formatter, "{slug} is a built-in and has no config block")
            }
            Self::IsDefault { slug } => write!(formatter, "{slug} is the saved default"),
            Self::UnknownSlug { slug } => write!(
                formatter,
                "{slug} is not a built-in or a configured model"
            ),
            Self::MultipleModelsTables => write!(formatter, "more than one [models] table"),
            Self::DuplicateSlugHeader { slug } => {
                write!(formatter, "{slug} appears more than once")
            }
            Self::Parse { detail } => write!(formatter, "config.toml did not parse: {detail}"),
            Self::DroppedTables { tables } => write!(
                formatter,
                "edit dropped config tables: {}",
                tables.join(", ")
            ),
            Self::ChangedProvider { slug } => {
                write!(formatter, "edit changed the existing model {slug}")
            }
            Self::AddMissing { slug } => write!(formatter, "edit did not add {slug}"),
            Self::RemoveFailed { slug } => write!(formatter, "edit did not remove {slug}"),
            Self::DefaultMismatch { expected, actual } => write!(
                formatter,
                "default is {}, expected {}",
                display_optional(actual),
                display_optional(expected)
            ),
            Self::InvalidSetting { key, detail } => {
                write!(formatter, "{key} is invalid: {detail}")
            }
        }
    }
}

impl std::error::Error for RefuseReason {}

/// A refused text edit, together with the unchanged config.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refused {
    /// Why the edit did not apply.
    pub reason: RefuseReason,
    /// The config text the caller passed in.
    pub text: String,
}

impl fmt::Display for Refused {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.reason)
    }
}

impl std::error::Error for Refused {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.reason)
    }
}

/// Where the previous config bytes are copied before replacement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backup<'a> {
    /// `<cache_dir>/config.toml.prev` for the default config path.
    CacheDir(&'a Path),
    /// Sibling `<file>.prev`, so `config.toml` becomes `config.toml.prev`.
    Sibling,
}

/// Why a file replace did not finish.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplaceKind {
    /// The new bytes contain the caller-supplied secret. Nothing was written.
    SecretInReplacement,
    /// The existing config could not be read. Nothing was written.
    Read,
    /// The backup could not be written. The config was left unchanged.
    Backup,
    /// The temporary file could not be written after the backup.
    Temp,
    /// The temporary file could not be renamed onto the config.
    Rename,
}

/// A file replace that did not leave the new bytes in place.
#[derive(Debug)]
pub struct ReplaceError {
    /// Which step failed.
    pub kind: ReplaceKind,
    /// Path and source error. This string does not include the secret or the file bytes.
    pub detail: String,
}

impl fmt::Display for ReplaceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.detail)
    }
}

impl std::error::Error for ReplaceError {}

/// Which saved default the re-parse must observe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DefaultExpectation<'a> {
    /// The saved default stays whatever it was.
    Unchanged,
    /// The saved default becomes this slug.
    Set(&'a str),
}

/// One header-delimited region of the raw config.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Span {
    /// Index of the header line in the keepends split.
    start: usize,
    /// Index of the next header, or the line count when this is the last header.
    end: usize,
    /// Text inside the brackets, without the brackets.
    name: String,
}

/// Derive `or-` plus the provider id's normalized last path segment.
///
/// # Errors
///
/// Returns an error when the id is empty or its leaf has no alphanumeric characters.
#[must_use = "the derived slug is the caller's only copy of the normalized id"]
pub fn derive_slug(provider_id: &str) -> Result<String, RefuseReason> {
    if provider_id.is_empty() {
        return Err(RefuseReason::EmptyProviderId);
    }
    let leaf = match provider_id.rsplit('/').next() {
        Some(segment) => segment,
        None => provider_id,
    };
    let normalized = normalize_leaf(leaf);
    if normalized.is_empty() {
        return Err(RefuseReason::EmptySlugLeaf {
            provider_id: provider_id.to_owned(),
        });
    }
    let slug = format!("or-{normalized}");
    check_slug(&slug)?;
    Ok(slug)
}

/// Resolve `[grok_router]` from `config` without rewriting it.
///
/// A missing table or a missing key uses [`OPENROUTER_BASE_URL`] and
/// [`BUILTIN_SLUGS`]. `openrouter_base_url` must match `^https?://\S+$` with
/// no trim. `builtins` must be an array of non-empty strings, and an empty
/// array means there are no built-ins. Any other key, including `provider`,
/// is ignored. This function does not call [`direct_provider`].
///
/// # Errors
///
/// Returns [`RefuseReason::InvalidSetting`] when a present key is unusable,
/// or a parse or structural error when the document cannot be read.
#[must_use = "the resolved settings are the caller's only copy of the base and built-ins"]
pub fn router_settings(config: &str) -> Result<RouterSettings, RefuseReason> {
    structural_problem(config)?;
    let root = match config.parse::<toml::Table>() {
        Ok(root) => root,
        Err(error) => {
            return Err(RefuseReason::Parse {
                detail: error.to_string(),
            });
        }
    };
    settings_from_root(&root)
}

/// Read one `[grok_router.provider.<name>]` table from `config`.
///
/// `name` is the table key, compared exactly: it is not trimmed or case-folded.
/// `base_url` must match `^https?://\S+$` with no trim. `env_key` must match
/// `^[A-Za-z_][A-Za-z0-9_]*$`. Absent `catalog` is [`None`]. Any other string,
/// including `""`, is returned as that string. `deepseek-list-models` is
/// refused unless `base_url` is `https://api.deepseek.com` or that URL with
/// one trailing slash. This function does not splice, read the environment,
/// or fetch a catalog.
///
/// # Errors
///
/// Returns [`RefuseReason::InvalidSetting`] when `name` has no table or a
/// present key cannot be used, or a parse or structural error when the
/// document cannot be read. The caller's text is not rewritten.
#[must_use = "the provider table is the caller's only copy of the base, env key, and catalog"]
pub fn direct_provider(config: &str, name: &str) -> Result<DirectProvider, RefuseReason> {
    structural_problem(config)?;
    let root = match config.parse::<toml::Table>() {
        Ok(root) => root,
        Err(error) => {
            return Err(RefuseReason::Parse {
                detail: error.to_string(),
            });
        }
    };
    provider_from_root(&root, name)
}

/// Render one new model block, including its trailing newline.
///
/// `base_url` is written on this span only.
///
/// # Errors
///
/// Returns an error when the slug, provider id, or context window is unusable.
pub fn render_block(
    slug: &str,
    model: CatalogModel<'_>,
    base_url: &str,
) -> Result<String, RefuseReason> {
    check_slug(slug)?;
    if model.id.is_empty() {
        return Err(RefuseReason::EmptyProviderId);
    }
    let Some(context) = model.context_window.filter(|window| *window > 0) else {
        return Err(RefuseReason::NonPositiveContext {
            provider_id: model.id.to_owned(),
        });
    };
    let mut lines = vec![
        model_header(slug)?,
        format!("model = {}", toml_basic_string(model.id)),
        format!("base_url = {}", toml_basic_string(base_url)),
        format!("name = {}", toml_basic_string(display_name(model))),
        "env_key = \"OPENROUTER_API_KEY\"".to_owned(),
        "api_backend = \"chat_completions\"".to_owned(),
        format!("context_window = {context}"),
    ];
    if model.sees_image {
        lines.push("vision = true".to_owned());
    }
    let mut block = lines.join("\n");
    block.push('\n');
    Ok(block)
}

/// Insert one OpenRouter model block into `config`.
///
/// `slug` overrides derivation. `None` derives the slug from the provider id.
/// The same slug, the same provider id, and the same resolved base returns `config` unchanged.
///
/// # Errors
///
/// Returns [`Refused`] and the original text when the add is refused.
pub fn insert_model(
    config: &str,
    model: CatalogModel<'_>,
    slug: Option<&str>,
) -> Result<String, Refused> {
    let settings = require_settings(config)?;
    let root = read_config(config)?;
    if model.id.is_empty() {
        return Err(refuse(config, RefuseReason::EmptyProviderId));
    }
    if model.context_window.is_none_or(|window| window <= 0) {
        return Err(refuse(
            config,
            RefuseReason::NonPositiveContext {
                provider_id: model.id.to_owned(),
            },
        ));
    }
    let resolved = match resolved_slug(model.id, slug) {
        Ok(resolved) => resolved,
        Err(reason) => return Err(refuse(config, reason)),
    };
    let matches = provider_slugs(&root, model.id, &settings.base_url);
    if let Some(existing) = matches.iter().find(|existing| existing.as_str() != resolved) {
        return Err(refuse(
            config,
            RefuseReason::DuplicateProvider {
                provider_id: model.id.to_owned(),
                existing_slug: existing.clone(),
            },
        ));
    }
    if !matches.is_empty() {
        return Ok(config.to_owned());
    }
    if is_builtin(&settings.builtins, &resolved)
        || configured_models(&root).contains_key(&resolved)
    {
        return Err(refuse(
            config,
            RefuseReason::SlugInUse {
                slug: resolved,
            },
        ));
    }
    let block = match render_block(&resolved, model, &settings.base_url) {
        Ok(block) => block,
        Err(reason) => return Err(refuse(config, reason)),
    };
    let spliced = insert_block(config, &block);
    finish(
        config,
        &root,
        spliced,
        Some(resolved.as_str()),
        None,
        DefaultExpectation::Unchanged,
        Some(model.id),
    )
}

/// Replace the `default` line inside the single `[models]` table.
///
/// Creates that table only when the header is absent. Model spans are not rewritten.
///
/// # Errors
///
/// Returns [`Refused`] and the original text when the slug is unknown or the document is unsafe to edit.
pub fn set_default(config: &str, slug: &str) -> Result<String, Refused> {
    let settings = require_settings(config)?;
    if let Err(reason) = check_slug(slug) {
        return Err(refuse(config, reason));
    }
    let root = read_config(config)?;
    if !is_builtin(&settings.builtins, slug) && !configured_models(&root).contains_key(slug) {
        return Err(refuse(
            config,
            RefuseReason::UnknownSlug {
                slug: slug.to_owned(),
            },
        ));
    }
    let spliced = match rewrite_default(config, slug) {
        Ok(text) => text,
        Err(reason) => return Err(refuse(config, reason)),
    };
    finish(
        config,
        &root,
        spliced,
        None,
        None,
        DefaultExpectation::Set(slug),
        None,
    )
}

/// Delete one model table span.
///
/// # Errors
///
/// Returns [`Refused`] and the original text for a missing slug, a built-in, the saved default,
/// or a document the splice will not rewrite.
pub fn remove_model(config: &str, slug: &str) -> Result<String, Refused> {
    let settings = require_settings(config)?;
    if let Err(reason) = check_slug(slug) {
        return Err(refuse(config, reason));
    }
    let root = read_config(config)?;
    if saved_default(&root) == Some(slug) {
        return Err(refuse(
            config,
            RefuseReason::IsDefault {
                slug: slug.to_owned(),
            },
        ));
    }
    if is_builtin(&settings.builtins, slug) {
        return Err(refuse(
            config,
            RefuseReason::BuiltIn {
                slug: slug.to_owned(),
            },
        ));
    }
    if !configured_models(&root).contains_key(slug) {
        return Err(refuse(
            config,
            RefuseReason::NotConfigured {
                slug: slug.to_owned(),
            },
        ));
    }
    let spliced = match delete_span(config, slug) {
        Ok(text) => text,
        Err(reason) => return Err(refuse(config, reason)),
    };
    finish(
        config,
        &root,
        spliced,
        None,
        Some(slug),
        DefaultExpectation::Unchanged,
        None,
    )
}

/// Read the saved default and each configured slug with its provider id.
///
/// Does not read `[grok_router]`. A present or invalid settings table is not
/// an error here.
///
/// # Errors
///
/// Returns [`Refused`] when the text is not a config this crate will read.
#[must_use = "the inventory is the caller's only copy of the parsed models"]
pub fn read_configured(config: &str) -> Result<ConfiguredInventory, Refused> {
    let root = read_config(config)?;
    let saved_default = saved_default(&root).map(str::to_owned);
    let models = configured_models(&root)
        .into_iter()
        .map(|(slug, provider_id)| ConfiguredModel { slug, provider_id })
        .collect();
    Ok(ConfiguredInventory {
        saved_default,
        models,
    })
}

/// Back up `path`, then replace it with `bytes` via a temp file in the same directory.
///
/// An empty `secret` is not searched. A non-empty secret that occurs in `bytes` refuses the
/// replace before any write.
///
/// # Errors
///
/// Returns an error when the bytes contain `secret` or a filesystem step fails.
pub fn replace_file(
    path: &Path,
    backup: Backup<'_>,
    bytes: &[u8],
    secret: &str,
) -> Result<(), ReplaceError> {
    if contains_secret(bytes, secret) {
        return Err(ReplaceError {
            kind: ReplaceKind::SecretInReplacement,
            detail: "replacement contains the caller-supplied secret".to_owned(),
        });
    }
    let original = match std::fs::read(path) {
        Ok(original) => original,
        Err(error) => {
            return Err(ReplaceError {
                kind: ReplaceKind::Read,
                detail: io_detail("read", path, &error),
            });
        }
    };
    let backup_path = match backup {
        Backup::CacheDir(cache_dir) => cache_dir.join("config.toml.prev"),
        Backup::Sibling => sibling_prev(path),
    };
    if let Some(parent) = backup_path.parent() {
        if !parent.as_os_str().is_empty() {
            if let Err(error) = std::fs::create_dir_all(parent) {
                return Err(ReplaceError {
                    kind: ReplaceKind::Backup,
                    detail: io_detail("create backup directory", parent, &error),
                });
            }
        }
    }
    if let Err(error) = std::fs::write(&backup_path, &original) {
        return Err(ReplaceError {
            kind: ReplaceKind::Backup,
            detail: io_detail("write backup", &backup_path, &error),
        });
    }
    let temporary = temp_path(path);
    if let Err(error) = std::fs::write(&temporary, bytes) {
        return Err(ReplaceError {
            kind: ReplaceKind::Temp,
            detail: io_detail("write temporary config", &temporary, &error),
        });
    }
    if let Err(error) = std::fs::rename(&temporary, path) {
        discard_temp(&temporary);
        return Err(ReplaceError {
            kind: ReplaceKind::Rename,
            detail: io_detail("rename temporary config", path, &error),
        });
    }
    Ok(())
}

/// Format an optional slug the way a refusal message names a missing default.
fn display_optional(value: &Option<String>) -> String {
    match value {
        Some(text) => format!("{text:?}"),
        None => "none".to_owned(),
    }
}

/// Attach `reason` to an unchanged copy of `config`.
fn refuse(config: &str, reason: RefuseReason) -> Refused {
    Refused {
        reason,
        text: config.to_owned(),
    }
}

/// Resolve settings, or refuse with the original text before any splice.
fn require_settings(config: &str) -> Result<RouterSettings, Refused> {
    match router_settings(config) {
        Ok(settings) => Ok(settings),
        Err(reason) => Err(refuse(config, reason)),
    }
}

/// Choose the explicit slug or derive one from `provider_id`.
fn resolved_slug(provider_id: &str, slug: Option<&str>) -> Result<String, RefuseReason> {
    match slug {
        Some(explicit) => {
            check_slug(explicit)?;
            Ok(explicit.to_owned())
        }
        None => derive_slug(provider_id),
    }
}

/// Require the slug shape shared by headers, defaults, and `--as`.
fn check_slug(slug: &str) -> Result<(), RefuseReason> {
    let mut chars = slug.chars();
    let Some(first) = chars.next() else {
        return Err(RefuseReason::InvalidSlug {
            slug: slug.to_owned(),
        });
    };
    let rest_ok = chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '.' || ch == '-');
    if first.is_ascii_alphanumeric() && rest_ok {
        Ok(())
    } else {
        Err(RefuseReason::InvalidSlug {
            slug: slug.to_owned(),
        })
    }
}

/// Report whether `slug` is a member of the resolved built-in list.
fn is_builtin(builtins: &[String], slug: &str) -> bool {
    builtins.iter().any(|candidate| candidate == slug)
}

/// Settings for a document with no `[grok_router]` table.
fn compiled_settings() -> RouterSettings {
    RouterSettings {
        base_url: OPENROUTER_BASE_URL.to_owned(),
        builtins: compiled_builtins(),
    }
}

/// The four compiled slugs, in [`BUILTIN_SLUGS`] order.
fn compiled_builtins() -> Vec<String> {
    BUILTIN_SLUGS
        .iter()
        .copied()
        .map(str::to_owned)
        .collect()
}

/// Read the base and built-ins from a parsed `[grok_router]` table.
///
/// Keys other than `openrouter_base_url` and `builtins` are ignored.
fn settings_from_root(root: &toml::Table) -> Result<RouterSettings, RefuseReason> {
    let Some(value) = root.get("grok_router") else {
        return Ok(compiled_settings());
    };
    let Some(table) = value.as_table() else {
        return Err(invalid_setting(
            "openrouter_base_url",
            "[grok_router] must be a table",
        ));
    };
    let base_url = match table.get("openrouter_base_url") {
        None => OPENROUTER_BASE_URL.to_owned(),
        Some(value) => http_base(value)?,
    };
    let builtins = match table.get("builtins") {
        None => compiled_builtins(),
        Some(value) => builtin_list(value)?,
    };
    Ok(RouterSettings { base_url, builtins })
}

/// Accept `openrouter_base_url` only when it matches `^https?://\S+$`.
fn http_base(value: &toml::Value) -> Result<String, RefuseReason> {
    let Some(text) = value.as_str() else {
        return Err(invalid_setting(
            "openrouter_base_url",
            "must be a string matching ^https?://\\S+$",
        ));
    };
    if !is_http_base(text) {
        return Err(invalid_setting(
            "openrouter_base_url",
            &format!("must match ^https?://\\S+$ with no trim, got {text:?}"),
        ));
    }
    Ok(text.to_owned())
}

/// Report whether `text` is `http://` or `https://` plus at least one non-whitespace.
fn is_http_base(text: &str) -> bool {
    let rest = if let Some(rest) = text.strip_prefix("https://") {
        rest
    } else if let Some(rest) = text.strip_prefix("http://") {
        rest
    } else {
        return false;
    };
    !rest.is_empty() && rest.chars().all(|character| !character.is_whitespace())
}

/// Accept `builtins` only as an array of non-empty strings. An empty array is none.
fn builtin_list(value: &toml::Value) -> Result<Vec<String>, RefuseReason> {
    let Some(items) = value.as_array() else {
        return Err(invalid_setting(
            "builtins",
            "must be an array of non-empty strings",
        ));
    };
    let mut slugs = Vec::new();
    for item in items {
        let Some(slug) = item.as_str().filter(|slug| !slug.is_empty()) else {
            return Err(invalid_setting(
                "builtins",
                "must be an array of non-empty strings",
            ));
        };
        slugs.push(slug.to_owned());
    }
    Ok(slugs)
}

/// Build an [`RefuseReason::InvalidSetting`] for `key`.
fn invalid_setting(key: &str, detail: &str) -> RefuseReason {
    RefuseReason::InvalidSetting {
        key: key.to_owned(),
        detail: detail.to_owned(),
    }
}

/// Verified catalog value. The provider name does not select it.
const DEEPSEEK_LIST_MODELS: &str = "deepseek-list-models";

/// `base_url` values that may carry [`DEEPSEEK_LIST_MODELS`].
const DEEPSEEK_CATALOG_BASES: [&str; 2] = [
    "https://api.deepseek.com",
    "https://api.deepseek.com/",
];

/// Read `grok_router.provider.<name>` from an already parsed document.
///
/// # Errors
///
/// Returns [`RefuseReason::InvalidSetting`] when `name` has no table or a
/// present key cannot be used.
fn provider_from_root(root: &toml::Table, name: &str) -> Result<DirectProvider, RefuseReason> {
    let Some(table) = named_provider_table(root, name) else {
        return Err(missing_provider(name));
    };
    let base_url = provider_base(table.get("base_url"))?;
    let env_key = provider_env_key(table.get("env_key"))?;
    let catalog = provider_catalog(table.get("catalog"), &base_url)?;
    Ok(DirectProvider {
        base_url,
        env_key,
        catalog,
    })
}

/// The table at `grok_router.provider.<name>`, when that key is a table.
fn named_provider_table<'a>(root: &'a toml::Table, name: &str) -> Option<&'a toml::Table> {
    root.get("grok_router")
        .and_then(toml::Value::as_table)
        .and_then(|grok| grok.get("provider"))
        .and_then(toml::Value::as_table)
        .and_then(|providers| providers.get(name))
        .and_then(toml::Value::as_table)
}

/// Refuse `name` because no provider table uses that exact key.
fn missing_provider(name: &str) -> RefuseReason {
    invalid_setting(name, "no [grok_router.provider] table has this name")
}

/// Accept a provider `base_url` only when it matches `^https?://\S+$`.
///
/// # Errors
///
/// Returns [`RefuseReason::InvalidSetting`] on `base_url` when the value is
/// missing, not a string, or fails the pattern. The string is not trimmed.
fn provider_base(value: Option<&toml::Value>) -> Result<String, RefuseReason> {
    let Some(text) = value.and_then(toml::Value::as_str) else {
        return Err(invalid_setting(
            "base_url",
            "must be a string matching ^https?://\\S+$",
        ));
    };
    if !is_http_base(text) {
        return Err(invalid_setting(
            "base_url",
            &format!("must match ^https?://\\S+$ with no trim, got {text:?}"),
        ));
    }
    Ok(text.to_owned())
}

/// Accept a provider `env_key` only when it matches `^[A-Za-z_][A-Za-z0-9_]*$`.
///
/// # Errors
///
/// Returns [`RefuseReason::InvalidSetting`] on `env_key` when the value is
/// missing, not a string, or fails the pattern. The string is not trimmed.
fn provider_env_key(value: Option<&toml::Value>) -> Result<String, RefuseReason> {
    let Some(text) = value.and_then(toml::Value::as_str) else {
        return Err(invalid_setting(
            "env_key",
            "must be a string matching ^[A-Za-z_][A-Za-z0-9_]*$",
        ));
    };
    if !is_env_key(text) {
        return Err(invalid_setting(
            "env_key",
            &format!("must match ^[A-Za-z_][A-Za-z0-9_]*$ with no trim, got {text:?}"),
        ));
    }
    Ok(text.to_owned())
}

/// Report whether `text` is a shell-style environment variable name.
fn is_env_key(text: &str) -> bool {
    let mut chars = text.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first.is_ascii_alphabetic() || first == '_')
        && chars.all(|character| character.is_ascii_alphanumeric() || character == '_')
}

/// Return `catalog` as written, or refuse the verified value on another base.
///
/// # Errors
///
/// Returns [`RefuseReason::InvalidSetting`] on `catalog` when the value is
/// not a string, or when it is `deepseek-list-models` on any other base.
fn provider_catalog(
    value: Option<&toml::Value>,
    base_url: &str,
) -> Result<Option<String>, RefuseReason> {
    let Some(value) = value else {
        return Ok(None);
    };
    let Some(text) = value.as_str() else {
        return Err(invalid_setting("catalog", "must be a string"));
    };
    if text == DEEPSEEK_LIST_MODELS && !DEEPSEEK_CATALOG_BASES.contains(&base_url) {
        return Err(invalid_setting(
            "catalog",
            "deepseek-list-models requires base_url https://api.deepseek.com or https://api.deepseek.com/",
        ));
    }
    Ok(Some(text.to_owned()))
}

/// Turn every non-alphanumeric run into one hyphen, then trim and lowercase.
fn normalize_leaf(leaf: &str) -> String {
    let mut normalized = String::new();
    let mut pending_hyphen = false;
    let mut started = false;
    for ch in leaf.chars() {
        if ch.is_ascii_alphanumeric() {
            if pending_hyphen && started {
                normalized.push('-');
            }
            pending_hyphen = false;
            started = true;
            normalized.push(ch.to_ascii_lowercase());
        } else if started {
            pending_hyphen = true;
        }
    }
    normalized
}

/// Use the catalog name when it has a non-blank trimmed form, otherwise the provider id.
fn display_name<'a>(model: CatalogModel<'a>) -> &'a str {
    match model.name.map(str::trim) {
        Some(name) if !name.is_empty() => name,
        _ => model.id,
    }
}

/// Header for `slug`. Dotted and other non-bare slugs are quoted so they stay one key.
fn model_header(slug: &str) -> Result<String, RefuseReason> {
    check_slug(slug)?;
    if is_bare_slug(slug) {
        Ok(format!("[model.{slug}]"))
    } else {
        Ok(format!("[model.{}]", toml_basic_string(slug)))
    }
}

/// Report whether `slug` can be a bare TOML key after `model.`.
fn is_bare_slug(slug: &str) -> bool {
    !slug.is_empty()
        && slug
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
}

/// Encode `value` as a TOML basic string, including the surrounding quotes.
fn toml_basic_string(value: &str) -> String {
    let mut encoded = String::from("\"");
    for ch in value.chars() {
        match ch {
            '\\' => encoded.push_str("\\\\"),
            '"' => encoded.push_str("\\\""),
            '\n' => encoded.push_str("\\n"),
            '\r' => encoded.push_str("\\r"),
            '\t' => encoded.push_str("\\t"),
            control if control < ' ' => push_unicode_escape(&mut encoded, control),
            other => encoded.push(other),
        }
    }
    encoded.push('"');
    encoded
}

/// Append a four-digit `\u` escape for one control character.
fn push_unicode_escape(encoded: &mut String, control: char) {
    let code = u32::from(control);
    encoded.push_str("\\u");
    for shift in [12, 8, 4, 0] {
        let nibble = u8::try_from((code >> shift) & 0xF).unwrap_or(0);
        let hex = b"0123456789abcdef".get(usize::from(nibble)).copied().unwrap_or(b'0');
        encoded.push(char::from(hex));
    }
}

/// Decode a TOML basic string used as a model header key.
fn toml_unquote(quoted: &str) -> Result<String, RefuseReason> {
    if quoted.len() < 2 || !quoted.starts_with('"') || !quoted.ends_with('"') {
        return Err(RefuseReason::Parse {
            detail: format!("expected a quoted TOML string, got {quoted:?}"),
        });
    }
    let Some(inner) = quoted.get(1..quoted.len().saturating_sub(1)) else {
        return Err(RefuseReason::Parse {
            detail: format!("expected a quoted TOML string, got {quoted:?}"),
        });
    };
    let mut decoded = String::new();
    let mut chars = inner.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            decoded.push(ch);
            continue;
        }
        let Some(escaped) = chars.next() else {
            return Err(RefuseReason::Parse {
                detail: format!("unsupported escape in {quoted:?}"),
            });
        };
        let next = match escaped {
            '\\' => '\\',
            '"' => '"',
            'n' => '\n',
            't' => '\t',
            'r' => '\r',
            _ => {
                return Err(RefuseReason::Parse {
                    detail: format!("unsupported escape in {quoted:?}"),
                });
            }
        };
        decoded.push(next);
    }
    Ok(decoded)
}

/// Read a model slug from a header body such as `model.or-glm-5-3` or `model."or.vision"`.
fn model_slug_from_header(name: &str) -> Result<Option<String>, RefuseReason> {
    let Some(rest) = name.strip_prefix("model.") else {
        return Ok(None);
    };
    if rest.starts_with('"') {
        let slug = toml_unquote(rest)?;
        check_slug(&slug)?;
        return Ok(Some(slug));
    }
    if is_bare_slug(rest) {
        return Ok(Some(rest.to_owned()));
    }
    Ok(None)
}

/// Reject duplicate `[models]` headers and duplicate model slugs before editing.
fn structural_problem(text: &str) -> Result<(), RefuseReason> {
    let lines = split_keepends(text);
    let spans = table_spans(&lines);
    let mut models_tables = 0usize;
    let mut slug_counts: BTreeMap<String, usize> = BTreeMap::new();
    for span in &spans {
        if span.name == "models" {
            models_tables = models_tables.saturating_add(1);
        }
        if let Some(slug) = model_slug_from_header(&span.name)? {
            let count = slug_counts.entry(slug).or_insert(0);
            *count = count.saturating_add(1);
        }
    }
    if models_tables > 1 {
        return Err(RefuseReason::MultipleModelsTables);
    }
    if let Some((slug, _)) = slug_counts.iter().find(|(_, count)| **count > 1) {
        return Err(RefuseReason::DuplicateSlugHeader { slug: slug.clone() });
    }
    Ok(())
}

/// Parse `config` after the duplicate-header check.
fn read_config(config: &str) -> Result<toml::Table, Refused> {
    if let Err(reason) = structural_problem(config) {
        return Err(refuse(config, reason));
    }
    match config.parse::<toml::Table>() {
        Ok(root) => Ok(root),
        Err(error) => Err(refuse(
            config,
            RefuseReason::Parse {
                detail: error.to_string(),
            },
        )),
    }
}

/// Re-parse `spliced` and keep it only when the edit invariants hold.
fn finish(
    original: &str,
    before: &toml::Table,
    spliced: String,
    added: Option<&str>,
    removed: Option<&str>,
    default: DefaultExpectation<'_>,
    expected_provider: Option<&str>,
) -> Result<String, Refused> {
    if let Err(reason) = structural_problem(&spliced) {
        return Err(refuse(original, reason));
    }
    let after = match spliced.parse::<toml::Table>() {
        Ok(table) => table,
        Err(error) => {
            return Err(refuse(
                original,
                RefuseReason::Parse {
                    detail: error.to_string(),
                },
            ));
        }
    };
    if let Err(reason) = preserve(
        before,
        &after,
        added,
        removed,
        default,
        expected_provider,
    ) {
        return Err(refuse(original, reason));
    }
    Ok(spliced)
}

/// Compare parsed documents after a splice.
fn preserve(
    before: &toml::Table,
    after: &toml::Table,
    added: Option<&str>,
    removed: Option<&str>,
    default: DefaultExpectation<'_>,
    expected_provider: Option<&str>,
) -> Result<(), RefuseReason> {
    let before_models = configured_models(before);
    let after_models = configured_models(after);
    let mut missing = Vec::new();
    for key in top_level_keys(before).difference(&top_level_keys(after)) {
        let removed_last_model = removed.is_some() && key == "model" && after_models.is_empty();
        if removed_last_model {
            continue;
        }
        missing.push(key.clone());
    }
    if !missing.is_empty() {
        return Err(RefuseReason::DroppedTables { tables: missing });
    }
    for (slug, provider) in &before_models {
        if removed == Some(slug.as_str()) {
            continue;
        }
        match after_models.get(slug) {
            Some(current) if current == provider => {}
            _ => {
                return Err(RefuseReason::ChangedProvider {
                    slug: slug.clone(),
                });
            }
        }
    }
    if let Some(slug) = added {
        match after_models.get(slug) {
            Some(provider) if expected_provider.is_none_or(|id| provider == id) => {}
            Some(_) => {
                return Err(RefuseReason::ChangedProvider {
                    slug: slug.to_owned(),
                });
            }
            None => {
                return Err(RefuseReason::AddMissing {
                    slug: slug.to_owned(),
                });
            }
        }
    }
    if let Some(slug) = removed {
        if after_models.contains_key(slug) {
            return Err(RefuseReason::RemoveFailed {
                slug: slug.to_owned(),
            });
        }
    }
    let before_default = saved_default(before);
    let after_default = saved_default(after);
    let expected = match default {
        DefaultExpectation::Unchanged => before_default,
        DefaultExpectation::Set(slug) => Some(slug),
    };
    if after_default != expected {
        return Err(RefuseReason::DefaultMismatch {
            expected: expected.map(str::to_owned),
            actual: after_default.map(str::to_owned),
        });
    }
    Ok(())
}

/// Collect configured `model` slugs and their provider ids.
fn configured_models(root: &toml::Table) -> BTreeMap<String, String> {
    let mut found = BTreeMap::new();
    let Some(model) = root.get("model").and_then(toml::Value::as_table) else {
        return found;
    };
    for (slug, value) in model {
        let Some(entry) = value.as_table() else {
            continue;
        };
        let Some(provider) = entry.get("model").and_then(toml::Value::as_str) else {
            continue;
        };
        let _replaced = found.insert(slug.clone(), provider.to_owned());
    }
    found
}

/// Slugs whose provider id and base URL match `provider_id` and `base_url`.
fn provider_slugs(root: &toml::Table, provider_id: &str, base_url: &str) -> Vec<String> {
    let mut slugs = Vec::new();
    let Some(model) = root.get("model").and_then(toml::Value::as_table) else {
        return slugs;
    };
    for (slug, value) in model {
        let Some(entry) = value.as_table() else {
            continue;
        };
        let Some(id) = entry.get("model").and_then(toml::Value::as_str) else {
            continue;
        };
        let Some(base) = entry.get("base_url").and_then(toml::Value::as_str) else {
            continue;
        };
        if id == provider_id && base == base_url {
            slugs.push(slug.clone());
        }
    }
    slugs
}

/// Read `[models].default` when it is a string.
fn saved_default(root: &toml::Table) -> Option<&str> {
    root.get("models")
        .and_then(toml::Value::as_table)
        .and_then(|models| models.get("default"))
        .and_then(toml::Value::as_str)
}

/// Names of the root table's keys.
fn top_level_keys(root: &toml::Table) -> BTreeSet<String> {
    root.keys().cloned().collect()
}

/// Split `text` on `\n` and keep the separators, matching an edit that must be byte-stable.
fn split_keepends(text: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let bytes = text.as_bytes();
    let mut start = 0usize;
    let mut index = 0usize;
    while index < bytes.len() {
        let is_newline = bytes.get(index).copied() == Some(b'\n');
        if is_newline {
            let end = index.saturating_add(1);
            if let Some(line) = text.get(start..end) {
                lines.push(line);
            }
            start = end;
        }
        index = index.saturating_add(1);
    }
    if start < text.len() {
        if let Some(line) = text.get(start..) {
            lines.push(line);
        }
    }
    lines
}

/// Header spans. Array-of-tables headers and the interior of multiline strings are skipped.
fn table_spans(lines: &[&str]) -> Vec<Span> {
    let mut headers = Vec::new();
    let mut in_basic = false;
    let mut in_literal = false;
    for (index, line) in lines.iter().enumerate() {
        let stripped = line.trim();
        if in_basic {
            if stripped.contains("\"\"\"") {
                in_basic = false;
            }
            continue;
        }
        if in_literal {
            if stripped.contains("'''") {
                in_literal = false;
            }
            continue;
        }
        if stripped.matches("\"\"\"").count() % 2 == 1 {
            in_basic = true;
            continue;
        }
        if stripped.matches("'''").count() % 2 == 1 {
            in_literal = true;
            continue;
        }
        if stripped.starts_with("[[") {
            continue;
        }
        if let Some(name) = header_name(stripped) {
            headers.push((index, name.to_owned()));
        }
    }
    let mut spans = Vec::new();
    for (position, (start, name)) in headers.iter().enumerate() {
        let end = headers
            .get(position.saturating_add(1))
            .map(|(next, _)| *next)
            .unwrap_or(lines.len());
        spans.push(Span {
            start: *start,
            end,
            name: name.clone(),
        });
    }
    spans
}

/// Return the inside of a single `[header]` line.
fn header_name(stripped: &str) -> Option<&str> {
    let rest = stripped.strip_prefix('[')?.strip_suffix(']')?;
    if rest.is_empty() { None } else { Some(rest) }
}

/// Insert `block` immediately before the first `[models]` header.
///
/// A blank line separates the block from the previous line and from `[models]`.
/// When `[models]` is absent, the block is appended after a blank line.
fn insert_block(text: &str, block: &str) -> String {
    let mut block_owned = block.to_owned();
    if !block_owned.ends_with('\n') {
        block_owned.push('\n');
    }
    let lines = split_keepends(text);
    let Some(index) = lines.iter().position(|line| line.trim() == "[models]") else {
        return append_block(text, &block_owned);
    };
    let mut piece = String::new();
    let previous_not_blank = if index == 0 {
        true
    } else {
        lines
            .get(index.saturating_sub(1))
            .is_none_or(|line| !line.trim().is_empty())
    };
    if index == 0 || previous_not_blank {
        piece.push('\n');
    }
    piece.push_str(&block_owned);
    if !block_owned.ends_with("\n\n") {
        piece.push('\n');
    }
    let mut spliced = String::new();
    for (line_index, line) in lines.iter().enumerate() {
        if line_index == index {
            spliced.push_str(&piece);
        }
        spliced.push_str(line);
    }
    spliced
}

/// Append `block` after ensuring one blank line when `text` is non-empty.
fn append_block(text: &str, block: &str) -> String {
    let mut body = text.to_owned();
    if !body.is_empty() && !body.ends_with('\n') {
        body.push('\n');
    }
    if !body.is_empty() && !body.ends_with("\n\n") {
        body.push('\n');
    }
    body.push_str(block);
    body
}

/// Replace the first `default =` line in `[models]`, or append the table when it is absent.
fn rewrite_default(text: &str, slug: &str) -> Result<String, RefuseReason> {
    let lines_ref = split_keepends(text);
    let spans = table_spans(&lines_ref);
    let mut models_spans = spans.iter().filter(|span| span.name == "models");
    let Some(span) = models_spans.next() else {
        return Ok(append_models_table(text, slug));
    };
    if models_spans.next().is_some() {
        return Err(RefuseReason::MultipleModelsTables);
    }
    let start = span.start;
    let end = span.end;
    let mut lines: Vec<String> = lines_ref.iter().copied().map(str::to_owned).collect();
    let replacement = format!("default = {}\n", toml_basic_string(slug));
    let limit = end.min(lines.len());
    let mut index = start.saturating_add(1);
    while index < limit {
        let is_default = lines.get(index).is_some_and(|line| is_default_assignment(line));
        if is_default {
            if let Some(line) = lines.get_mut(index) {
                *line = replacement;
            }
            return Ok(join_lines(&lines));
        }
        index = index.saturating_add(1);
    }
    lines.insert(start.saturating_add(1), replacement);
    Ok(join_lines(&lines))
}

/// Report whether `line` assigns `[models].default` rather than `default_reasoning_effort`.
fn is_default_assignment(line: &str) -> bool {
    let Some(rest) = line.trim().strip_prefix("default") else {
        return false;
    };
    rest.trim_start().starts_with('=')
}

/// Append a new `[models]` table. The caller has already checked that the header is absent.
fn append_models_table(text: &str, slug: &str) -> String {
    let mut body = text.to_owned();
    if !body.is_empty() && !body.ends_with('\n') {
        body.push('\n');
    }
    body.push('\n');
    body.push_str("[models]\n");
    body.push_str(&format!("default = {}\n", toml_basic_string(slug)));
    body
}

/// Delete the header span for `slug`.
///
/// The span runs from its header through the line before the next header,
/// including blank lines that sit in that span. Bytes outside the span are
/// copied unchanged. Other gaps are not restyled.
fn delete_span(text: &str, slug: &str) -> Result<String, RefuseReason> {
    let lines_ref = split_keepends(text);
    let spans = table_spans(&lines_ref);
    let mut found = Vec::new();
    for span in &spans {
        if let Some(header_slug) = model_slug_from_header(&span.name)? {
            if header_slug == slug {
                found.push((span.start, span.end));
            }
        }
    }
    let (start, end) = match found.as_slice() {
        [] => {
            return Err(RefuseReason::NotConfigured {
                slug: slug.to_owned(),
            });
        }
        [(start, end)] => (*start, *end),
        _ => {
            return Err(RefuseReason::DuplicateSlugHeader {
                slug: slug.to_owned(),
            });
        }
    };
    let mut kept = String::new();
    for (index, line) in lines_ref.iter().enumerate() {
        if index < start || index >= end {
            kept.push_str(line);
        }
    }
    Ok(kept)
}

/// Join keepends lines without adding separators of our own.
fn join_lines(lines: &[String]) -> String {
    let mut joined = String::new();
    for line in lines {
        joined.push_str(line);
    }
    joined
}

/// Report whether `secret` occurs in `bytes`. An empty secret never matches.
fn contains_secret(bytes: &[u8], secret: &str) -> bool {
    let needle = secret.as_bytes();
    if needle.is_empty() || needle.len() > bytes.len() {
        return false;
    }
    bytes.windows(needle.len()).any(|window| window == needle)
}

/// `config.toml` becomes `config.toml.prev` beside the file.
fn sibling_prev(path: &Path) -> PathBuf {
    path_with_suffix(path, ".prev")
}

/// Temporary file used for the same-directory rename.
fn temp_path(path: &Path) -> PathBuf {
    path_with_suffix(path, ".grok-router-tmp")
}

/// Append `suffix` to the file name, preserving the parent directory.
fn path_with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path
        .file_name()
        .map(std::ffi::OsStr::to_os_string)
        .unwrap_or_else(|| std::ffi::OsString::from("config.toml"));
    name.push(suffix);
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.join(name),
        _ => PathBuf::from(name),
    }
}

/// Describe a filesystem failure without including file contents.
fn io_detail(action: &str, path: &Path, error: &std::io::Error) -> String {
    format!("{action} {}: {error}", path.display())
}

/// Remove a temporary file after a failed rename. Failure here is secondary.
fn discard_temp(path: &Path) {
    if let Err(error) = std::fs::remove_file(path) {
        let _ignored = error.kind();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const COMMENT: &str = "# kept comment\n";
    const MARKETPLACE: &str = "[[marketplace.sources]]\nname = \"local\"\n";
    const FLASH: &str = "\
[model.or-deepseek-flash]
model = \"deepseek/deepseek-v4.1-flash-1017\"
base_url = \"https://openrouter.ai/api/v1\"
name = \"DeepSeek Flash\"
env_key = \"OPENROUTER_API_KEY\"
api_backend = \"chat_completions\"
context_window = 1000000
max_completion_tokens = 65536
";
    const PRO: &str = "\
[model.or-deepseek-pro]
model = \"deepseek/deepseek-v4-pro-0813\"
base_url = \"https://openrouter.ai/api/v1\"
name = \"DeepSeek Pro\"
env_key = \"OPENROUTER_API_KEY\"
api_backend = \"chat_completions\"
context_window = 1000000
";
    const GLM: &str = "\
[model.or-glm-5-3]
model = \"z-ai/glm-5.3\"
base_url = \"https://openrouter.ai/api/v1\"
name = \"GLM 5.3\"
env_key = \"OPENROUTER_API_KEY\"
api_backend = \"chat_completions\"
context_window = 600000
";
    const GLM_FLASH: &str = "\
[model.or-glm-5-3-flash]
model = \"z-ai/glm-5.3-flash\"
base_url = \"https://openrouter.ai/api/v1\"
name = \"GLM 5.3 Flash\"
env_key = \"OPENROUTER_API_KEY\"
api_backend = \"chat_completions\"
context_window = 1000000
";
    const FIXTURE: &str = "\
# kept comment
[[marketplace.sources]]
name = \"local\"

[model.or-deepseek-flash]
model = \"deepseek/deepseek-v4.1-flash-1017\"
base_url = \"https://openrouter.ai/api/v1\"
name = \"DeepSeek Flash\"
env_key = \"OPENROUTER_API_KEY\"
api_backend = \"chat_completions\"
context_window = 1000000
max_completion_tokens = 65536

[model.or-deepseek-pro]
model = \"deepseek/deepseek-v4-pro-0813\"
base_url = \"https://openrouter.ai/api/v1\"
name = \"DeepSeek Pro\"
env_key = \"OPENROUTER_API_KEY\"
api_backend = \"chat_completions\"
context_window = 1000000

[model.or-glm-5-3]
model = \"z-ai/glm-5.3\"
base_url = \"https://openrouter.ai/api/v1\"
name = \"GLM 5.3\"
env_key = \"OPENROUTER_API_KEY\"
api_backend = \"chat_completions\"
context_window = 600000

[model.or-glm-5-3-flash]
model = \"z-ai/glm-5.3-flash\"
base_url = \"https://openrouter.ai/api/v1\"
name = \"GLM 5.3 Flash\"
env_key = \"OPENROUTER_API_KEY\"
api_backend = \"chat_completions\"
context_window = 1000000

[models]
default = \"grok-4.7\"
default_reasoning_effort = \"high\"
";
    const PROVIDERS: &str = "\
[grok_router]
openrouter_base_url = \"https://openrouter.ai/api/v1\"
builtins = [\"grok-4.7\", \"grok-4.7-build-fast\", \"grok-4.6\", \"grok-4.5\"]

[grok_router.provider.deepseek]
base_url = \"https://api.deepseek.com\"
env_key = \"DEEPSEEK_API_KEY\"
catalog = \"deepseek-list-models\"

[grok_router.provider.zai]
base_url = \"https://api.z.ai/api/paas/v4\"
env_key = \"ZAI_API_KEY\"
";

    fn widget(context: Option<i64>, sees_image: bool) -> CatalogModel<'static> {
        CatalogModel {
            id: "acme/widget.v2",
            name: Some("Widget"),
            context_window: context,
            sees_image,
        }
    }

    fn assert_fixture_blocks(text: &str) {
        for block in [COMMENT, MARKETPLACE, FLASH, PRO, GLM, GLM_FLASH] {
            assert!(
                text.contains(block),
                "edit should keep this fixture span byte-identical:\n{block}"
            );
        }
    }

    #[test]
    fn insert_keeps_fixture_blocks_and_writes_catalog_context_only_on_the_new_block() {
        let block = render_block(
            "or-widget-v2",
            widget(Some(1_048_576), false),
            OPENROUTER_BASE_URL,
        )
            .expect("a positive context window should render a block");
        assert_eq!(
            block,
            "\
[model.or-widget-v2]
model = \"acme/widget.v2\"
base_url = \"https://openrouter.ai/api/v1\"
name = \"Widget\"
env_key = \"OPENROUTER_API_KEY\"
api_backend = \"chat_completions\"
context_window = 1048576
"
        );
        assert!(!block.contains("api_key"), "a new block must not store api_key");
        assert!(
            !block.contains("max_completion_tokens"),
            "a new block must not copy max_completion_tokens"
        );
        assert!(
            !block.contains("reasoning_efforts"),
            "a new block must not set reasoning_efforts"
        );
        assert!(!block.contains("vision"), "text-only input must omit vision");

        let updated = insert_model(FIXTURE, widget(Some(1_048_576), false), None)
            .expect("a new provider id should insert one block");
        assert_fixture_blocks(&updated);
        assert!(
            GLM.contains("context_window = 600000"),
            "the fixture's glm block should record the 600000 window"
        );
        assert!(
            updated.contains(&format!("{block}\n[models]")),
            "the new block should sit immediately before [models] with one blank line"
        );
        assert!(
            updated.contains("default_reasoning_effort = \"high\""),
            "insert should leave default_reasoning_effort in place"
        );
    }

    #[test]
    fn same_provider_and_slug_returns_the_original_text() {
        let updated = insert_model(
            FIXTURE,
            CatalogModel {
                id: "z-ai/glm-5.3",
                name: Some("replacement name"),
                context_window: Some(1_048_576),
                sees_image: true,
            },
            Some("or-glm-5-3"),
        )
        .expect("the same slug and provider id should be a no-op");

        assert_eq!(
            updated, FIXTURE,
            "a catalog context_length of 1048576 must not rewrite the 600000 block"
        );
    }

    #[test]
    fn different_slug_for_the_same_provider_returns_the_original_text() {
        let refused = insert_model(
            FIXTURE,
            CatalogModel {
                id: "deepseek/deepseek-v4.1-flash-1017",
                name: Some("DeepSeek Flash"),
                context_window: Some(1_048_576),
                sees_image: false,
            },
            None,
        )
        .expect_err("a derived slug that differs from the configured slug should be refused");

        assert_eq!(refused.text, FIXTURE, "a duplicate provider id should return the original text");
        assert!(
            matches!(
                refused.reason,
                RefuseReason::DuplicateProvider {
                    ref existing_slug,
                    ..
                } if existing_slug == "or-deepseek-flash"
            ),
            "duplicate detection is provider id plus base URL, got {}",
            refused.reason
        );
        assert_eq!(
            derive_slug("deepseek/deepseek-v4.1-flash-1017")
                .expect("this provider id should derive a slug"),
            "or-deepseek-v4-1-flash-1017"
        );
    }

    #[test]
    fn dotted_slug_is_one_quoted_key_and_vision_follows_image_input() {
        let quoted = CatalogModel {
            id: "acme/vision.v1",
            name: Some("Z.ai: GLM \"5.3\""),
            context_window: Some(128_000),
            sees_image: true,
        };
        let updated = insert_model(FIXTURE, quoted, Some("or.vision"))
            .expect("a dotted slug should be inserted as one quoted key");

        assert!(
            updated.contains("[model.\"or.vision\"]\n"),
            "a dotted slug should use a quoted header, got:\n{updated}"
        );
        assert!(
            updated.contains("vision = true\n"),
            "image input should set vision = true"
        );
        assert!(
            !updated.contains("vision = false"),
            "the splice must not write vision = false"
        );
        assert_fixture_blocks(&updated);

        let parsed: toml::Table = updated
            .parse()
            .expect("the spliced config should parse");
        let models = parsed
            .get("model")
            .and_then(toml::Value::as_table)
            .expect("the model table should be present");
        assert!(
            models.get("or").is_none(),
            "a dotted slug must not become nested model.or.vision tables"
        );
        let entry = models
            .get("or.vision")
            .and_then(toml::Value::as_table)
            .expect("or.vision should parse as one key");
        assert_eq!(entry.get("vision").and_then(toml::Value::as_bool), Some(true));
        assert_eq!(
            entry.get("name").and_then(toml::Value::as_str),
            Some("Z.ai: GLM \"5.3\"")
        );
    }

    #[test]
    fn name_falls_back_and_context_must_be_positive() {
        let nameless = insert_model(
            FIXTURE,
            CatalogModel {
                id: "acme/nameless",
                name: None,
                context_window: Some(32),
                sees_image: false,
            },
            None,
        )
        .expect("a missing catalog name should still insert");
        assert!(
            nameless.contains("name = \"acme/nameless\"\n"),
            "a missing name should fall back to the provider id"
        );

        let blank = insert_model(
            &nameless,
            CatalogModel {
                id: "acme/blank",
                name: Some("   "),
                context_window: Some(32),
                sees_image: false,
            },
            None,
        )
        .expect("a blank catalog name should still insert");
        assert!(
            blank.contains("name = \"acme/blank\"\n"),
            "a blank name should fall back to the provider id"
        );

        for context in [None, Some(0), Some(-1)] {
            let refused = insert_model(FIXTURE, widget(context, false), None)
                .expect_err("a non-positive context_window should refuse the add");
            assert_eq!(
                refused.text, FIXTURE,
                "context {context:?} should leave the config unchanged"
            );
            assert!(
                matches!(refused.reason, RefuseReason::NonPositiveContext { .. }),
                "context {context:?} should be a context refusal, got {}",
                refused.reason
            );
        }
    }

    #[test]
    fn derived_slug_uses_the_leaf_and_an_empty_leaf_is_an_error() {
        assert_eq!(
            derive_slug("z-ai/glm-5.3").expect("glm-5.3 should derive a bare slug"),
            "or-glm-5-3"
        );
        for provider_id in ["acme/", "@@@", "/"] {
            let refused = insert_model(
                FIXTURE,
                CatalogModel {
                    id: provider_id,
                    name: Some("Nope"),
                    context_window: Some(8),
                    sees_image: false,
                },
                None,
            )
            .expect_err("an empty leaf should refuse derivation");
            assert_eq!(refused.text, FIXTURE);
            assert!(
                matches!(refused.reason, RefuseReason::EmptySlugLeaf { .. }),
                "{provider_id} should be an empty leaf, got {}",
                refused.reason
            );
        }

        let updated = insert_model(
            FIXTURE,
            CatalogModel {
                id: "@@@",
                name: Some("Custom"),
                context_window: Some(8),
                sees_image: false,
            },
            Some("or-custom"),
        )
        .expect("an explicit slug should bypass an empty leaf");
        assert!(updated.contains("[model.or-custom]\n"));
    }

    #[test]
    fn slug_already_used_by_a_model_or_a_builtin_is_an_error() {
        for slug in ["or-glm-5-3", "grok-4.5"] {
            let refused = insert_model(FIXTURE, widget(Some(8), false), Some(slug))
                .expect_err("a taken slug should be refused");
            assert_eq!(refused.text, FIXTURE, "{slug} should leave the config unchanged");
            assert!(
                matches!(refused.reason, RefuseReason::SlugInUse { .. }),
                "{slug} should be in use, got {}",
                refused.reason
            );
        }
        let refused = insert_model(FIXTURE, widget(Some(8), false), Some("-nope"))
            .expect_err("a slug must start with a letter or digit");
        assert_eq!(refused.text, FIXTURE);
        assert!(matches!(refused.reason, RefuseReason::InvalidSlug { .. }));
    }

    #[test]
    fn insert_puts_one_blank_line_around_the_block() {
        let text = "# c\nkey = 1\n[models]\ndefault = \"grok-4.7\"\n";
        let updated = insert_model(text, widget(Some(1_048_576), false), None)
            .expect("a root key plus [models] should accept an insert");
        assert!(
            updated.contains("key = 1\n\n[model.or-widget-v2]\n"),
            "one blank line should precede the new block, got:\n{updated}"
        );
        assert!(
            updated.contains("context_window = 1048576\n\n[models]\n"),
            "one blank line should precede [models], got:\n{updated}"
        );
    }

    #[test]
    fn absent_models_table_appends_the_block_and_set_default_creates_only_one_header() {
        let (head, _) = FIXTURE
            .split_once("[models]\n")
            .expect("the fixture should contain one [models] table");
        assert!(
            head.ends_with("\n\n"),
            "the fixture should already separate [models] with a blank line"
        );
        let block = render_block(
            "or-widget-v2",
            widget(Some(8), false),
            OPENROUTER_BASE_URL,
        )
            .expect("the widget block should render");
        let appended = insert_model(head, widget(Some(8), false), None)
            .expect("a document without [models] should append the block");
        assert_eq!(appended, format!("{head}{block}"));
        assert!(
            !appended.contains("[models]"),
            "append should not invent a [models] table"
        );

        let with_default = set_default(head, "grok-4.6")
            .expect("set-default should create [models] when the header is absent");
        assert!(with_default.starts_with(head));
        assert_eq!(
            with_default
                .lines()
                .filter(|line| line.trim() == "[models]")
                .count(),
            1,
            "set-default should create one [models] header, got:\n{with_default}"
        );
        assert!(with_default.contains("default = \"grok-4.6\"\n"));
        assert_fixture_blocks(head);
        assert_fixture_blocks(&with_default);
    }

    #[test]
    fn set_default_replaces_only_the_default_line() {
        let (prefix, _) = FIXTURE
            .split_once("[models]\n")
            .expect("the fixture should contain [models]");
        for slug in ["grok-4.6", "or-glm-5-3"] {
            let updated = set_default(FIXTURE, slug)
                .expect("a built-in or configured slug should become the default");
            let (updated_prefix, rest) = updated.split_once("[models]\n").expect(
                "set-default should keep the existing [models] header",
            );
            assert_eq!(
                updated_prefix, prefix,
                "set-default should leave every byte before [models] unchanged"
            );
            assert!(
                rest.contains("default_reasoning_effort = \"high\""),
                "default_reasoning_effort should stay"
            );
            assert!(
                rest.contains(&format!("default = \"{slug}\"\n")),
                "the default line should be replaced with {slug}, got:\n{rest}"
            );
            assert_eq!(
                updated
                    .lines()
                    .filter(|line| line.trim() == "[models]")
                    .count(),
                1
            );
        }

        let refused = set_default(FIXTURE, "not-a-model")
            .expect_err("an unknown slug should be refused");
        assert_eq!(refused.text, FIXTURE);
        assert!(matches!(refused.reason, RefuseReason::UnknownSlug { .. }));
    }

    #[test]
    fn remove_deletes_only_that_table_span() {
        let inserted = insert_model(FIXTURE, widget(Some(8), false), None)
            .expect("insert should succeed before the round trip");
        let restored = remove_model(&inserted, "or-widget-v2")
            .expect("removing the inserted slug should succeed");
        assert_eq!(
            restored, FIXTURE,
            "removing the inserted span should restore the fixture bytes"
        );

        let removed = remove_model(FIXTURE, "or-deepseek-pro")
            .expect("a configured non-default slug should be removable");
        assert!(
            !removed.contains("[model.or-deepseek-pro]"),
            "remove should delete that header"
        );
        for block in [COMMENT, MARKETPLACE, FLASH, GLM, GLM_FLASH] {
            assert!(
                removed.contains(block),
                "remove should leave this span byte-identical:\n{block}"
            );
        }
        assert!(removed.contains("default = \"grok-4.7\"\n"));
        assert!(removed.contains("default_reasoning_effort = \"high\""));
        assert!(
            removed.contains("max_completion_tokens = 65536"),
            "remove should not rewrite the flash block"
        );
    }

    #[test]
    fn remove_of_the_default_builtin_or_missing_slug_returns_the_original_text() {
        let default_refused = remove_model(FIXTURE, "grok-4.7")
            .expect_err("the saved default should be refused");
        assert_eq!(default_refused.text, FIXTURE);
        assert!(matches!(
            default_refused.reason,
            RefuseReason::IsDefault { .. }
        ));

        let builtin = remove_model(FIXTURE, "grok-4.6")
            .expect_err("a built-in should be refused");
        assert_eq!(builtin.text, FIXTURE);
        assert!(matches!(builtin.reason, RefuseReason::BuiltIn { .. }));

        let missing = remove_model(FIXTURE, "or-missing")
            .expect_err("a missing slug should be refused");
        assert_eq!(missing.text, FIXTURE);
        assert!(matches!(missing.reason, RefuseReason::NotConfigured { .. }));

        let switched = set_default(FIXTURE, "or-glm-5-3")
            .expect("a configured slug should become the default");
        let refused = remove_model(&switched, "or-glm-5-3")
            .expect_err("remove of the saved default should be refused");
        assert_eq!(refused.text, switched);
        assert!(matches!(refused.reason, RefuseReason::IsDefault { .. }));
    }

    #[test]
    fn remove_of_the_only_model_drops_that_table() {
        let only = "\
# kept comment
[model.or-only]
model = \"acme/only\"
base_url = \"https://openrouter.ai/api/v1\"
name = \"Only\"

[models]
default = \"grok-4.7\"
";
        let updated = remove_model(only, "or-only")
            .expect("removing the only configured model should delete its table");
        assert!(!updated.contains("[model.or-only]"));
        assert!(updated.contains("# kept comment\n"));
        assert!(updated.contains("default = \"grok-4.7\"\n"));
    }

    #[test]
    fn duplicate_models_or_slug_headers_are_errors() {
        let duplicate_models = format!("{FIXTURE}\n[models]\ndefault = \"grok-4.5\"\n");
        let models = set_default(&duplicate_models, "grok-4.6")
            .expect_err("two [models] tables should be refused");
        assert_eq!(models.text, duplicate_models);
        assert!(matches!(
            models.reason,
            RefuseReason::MultipleModelsTables
        ));

        let duplicate_slug = "\
[model.or-glm-5-3]
model = \"z-ai/glm-5.3\"

[model.or-glm-5-3]
model = \"z-ai/glm-5.3\"

[models]
default = \"grok-4.7\"
";
        let slug = remove_model(duplicate_slug, "or-glm-5-3")
            .expect_err("two headers for one slug should be refused");
        assert_eq!(slug.text, duplicate_slug);
        assert!(matches!(
            slug.reason,
            RefuseReason::DuplicateSlugHeader { .. }
        ));

        let malformed = "[\n";
        let parsed = insert_model(malformed, widget(Some(8), false), None)
            .expect_err("malformed toml should be refused");
        assert_eq!(parsed.text, malformed);
        assert!(matches!(parsed.reason, RefuseReason::Parse { .. }));
    }

    #[test]
    fn missing_settings_table_uses_the_compiled_base_and_env_key() {
        let updated = insert_model(FIXTURE, widget(Some(8), false), None)
            .expect("a document with no settings table should still insert");
        assert!(
            updated.contains("base_url = \"https://openrouter.ai/api/v1\"\n"),
            "the new block should use the compiled base, got:\n{updated}"
        );
        assert!(
            updated.contains("env_key = \"OPENROUTER_API_KEY\"\n"),
            "the new block should keep the OpenRouter env key, got:\n{updated}"
        );
        assert!(
            !updated.contains("[grok_router]"),
            "insert must not write [grok_router], got:\n{updated}"
        );
        assert!(
            !updated.contains("model_provider"),
            "the splice must not write model_provider"
        );
        assert!(!updated.contains("api_key"), "the splice must not write api_key");
        assert_fixture_blocks(&updated);
    }

    #[test]
    fn custom_base_is_written_on_the_new_block_only() {
        let base = "https://example.invalid/v1/";
        let config = with_settings(&format!("openrouter_base_url = \"{base}\""));
        let updated = insert_model(&config, widget(Some(8), false), None)
            .expect("a custom base should still insert one block");
        let block = "\
[model.or-widget-v2]
model = \"acme/widget.v2\"
base_url = \"https://example.invalid/v1/\"
name = \"Widget\"
env_key = \"OPENROUTER_API_KEY\"
api_backend = \"chat_completions\"
context_window = 8
";
        assert!(
            updated.contains(block),
            "the new block should keep the trailing slash, got:\n{updated}"
        );
        assert_eq!(
            updated
                .matches("\nbase_url = \"https://example.invalid/v1/\"\n")
                .count(),
            1,
            "the resolved base should appear on the new block only, got:\n{updated}"
        );
        assert_eq!(
            updated.matches("[grok_router]").count(),
            1,
            "the edit must not insert another [grok_router]"
        );
        assert!(
            updated.contains("openrouter_base_url = \"https://example.invalid/v1/\""),
            "the settings line should stay byte-identical"
        );
        assert_fixture_blocks(&updated);
        assert!(
            !updated.contains("model_provider"),
            "the splice must not write model_provider"
        );
        assert!(!updated.contains("api_key"), "the splice must not write api_key");
    }

    #[test]
    fn duplicate_detection_uses_the_resolved_base() {
        let base = "https://example.invalid/v1";
        let settings = format!("[grok_router]\nopenrouter_base_url = \"{base}\"\n\n");
        let same = format!(
            "{settings}[model.or-glm-5-3]\nmodel = \"z-ai/glm-5.3\"\n"
        );
        let same = format!("{same}base_url = \"{base}\"\nname = \"GLM 5.3\"\n");
        let unchanged = insert_model(
            &same,
            CatalogModel {
                id: "z-ai/glm-5.3",
                name: Some("replacement name"),
                context_window: Some(9),
                sees_image: true,
            },
            Some("or-glm-5-3"),
        )
        .expect("the same provider id and resolved base should be a no-op");
        assert_eq!(
            unchanged, same,
            "the same provider id at the resolved base should return the original text"
        );

        let different = format!("{settings}{GLM}");
        let updated = insert_model(
            &different,
            CatalogModel {
                id: "z-ai/glm-5.3",
                name: Some("GLM 5.3"),
                context_window: Some(8),
                sees_image: false,
            },
            Some("or-glm-elsewhere"),
        )
        .expect("the same provider id at a different base should not be a duplicate");
        assert!(
            updated.contains("[model.or-glm-elsewhere]\n"),
            "a different base should insert a second block, got:\n{updated}"
        );
        assert!(
            updated.contains(&format!("base_url = \"{base}\"\n")),
            "the new block should use the resolved base, got:\n{updated}"
        );
        assert!(
            updated.contains(GLM),
            "the existing model span should stay byte-identical"
        );
        assert_eq!(
            updated.matches("[grok_router]").count(),
            1,
            "duplicate detection must not insert [grok_router]"
        );
    }

    #[test]
    fn missing_builtins_key_keeps_the_four_compiled_slugs() {
        let config = with_settings("openrouter_base_url = \"https://example.invalid/v1\"");
        let (prefix, _) = config
            .split_once("[models]\n")
            .expect("the fixture should contain [models]");
        for slug in BUILTIN_SLUGS {
            let updated = set_default(&config, slug)
                .expect("a compiled slug should stay a built-in when builtins is absent");
            let (updated_prefix, rest) = updated
                .split_once("[models]\n")
                .expect("set-default should keep the [models] header");
            assert_eq!(
                updated_prefix, prefix,
                "set-default of {slug} should leave every earlier byte unchanged"
            );
            assert!(
                rest.contains(&format!("default = \"{slug}\"\n")),
                "{slug} should become the default, got:\n{rest}"
            );
            assert!(
                !rest.contains("[model."),
                "set-default must not add a model table, got:\n{rest}"
            );
        }

        for slug in ["grok-4.5", "grok-4.6", "grok-4.7-build-fast"] {
            let removed = remove_model(&config, slug)
                .expect_err("a compiled slug should still be a built-in");
            assert_eq!(removed.text, config, "{slug} should leave the text unchanged");
            assert!(
                matches!(removed.reason, RefuseReason::BuiltIn { .. }),
                "{slug} should be a built-in, got {}",
                removed.reason
            );
        }
    }

    #[test]
    fn builtins_list_replaces_the_compiled_slugs_and_adds_no_model_table() {
        let config = "\
[grok_router]
builtins = [\"grok-4.8\"]

[model.or-glm-5-3]
model = \"z-ai/glm-5.3\"
base_url = \"https://openrouter.ai/api/v1\"
name = \"GLM 5.3\"

[models]
default = \"or-glm-5-3\"
";
        let model = "\
[model.or-glm-5-3]
model = \"z-ai/glm-5.3\"
base_url = \"https://openrouter.ai/api/v1\"
name = \"GLM 5.3\"
";
        let updated = set_default(config, "grok-4.8")
            .expect("grok-4.8 should be a built-in when the list says so");
        assert!(updated.contains("default = \"grok-4.8\"\n"));
        assert!(
            updated.contains(model),
            "set-default should leave the existing model span byte-identical"
        );
        assert_eq!(
            updated.matches("[model.").count(),
            1,
            "the built-in list must not add a [model.*] table, got:\n{updated}"
        );

        let not_builtin = set_default(config, "grok-4.7")
            .expect_err("grok-4.7 should not be a built-in when it is absent from the list");
        assert_eq!(not_builtin.text, config);
        assert!(
            matches!(not_builtin.reason, RefuseReason::UnknownSlug { .. }),
            "grok-4.7 should be unknown, got {}",
            not_builtin.reason
        );

        let builtin = remove_model(config, "grok-4.8")
            .expect_err("grok-4.8 should be refused as a built-in");
        assert_eq!(builtin.text, config);
        assert!(
            matches!(builtin.reason, RefuseReason::BuiltIn { .. }),
            "grok-4.8 should be a built-in, got {}",
            builtin.reason
        );

        let ordinary = remove_model(config, "grok-4.7")
            .expect_err("grok-4.7 should not be treated as a built-in");
        assert_eq!(ordinary.text, config);
        assert!(
            matches!(ordinary.reason, RefuseReason::NotConfigured { .. }),
            "grok-4.7 should be missing, not built-in, got {}",
            ordinary.reason
        );

        let in_use = insert_model(config, widget(Some(8), false), Some("grok-4.8"))
            .expect_err("a listed built-in slug should be in use");
        assert_eq!(in_use.text, config);
        assert!(
            matches!(in_use.reason, RefuseReason::SlugInUse { .. }),
            "grok-4.8 should be in use, got {}",
            in_use.reason
        );
    }

    #[test]
    fn empty_builtins_means_there_are_no_builtins() {
        let config = "\
[grok_router]
builtins = []

[model.or-glm-5-3]
model = \"z-ai/glm-5.3\"
base_url = \"https://openrouter.ai/api/v1\"
name = \"GLM 5.3\"

[models]
default = \"or-glm-5-3\"
";
        for slug in BUILTIN_SLUGS {
            let defaulted = set_default(config, slug)
                .expect_err("an empty builtins array should not keep the compiled slug");
            assert_eq!(defaulted.text, config, "{slug} should leave the text unchanged");
            assert!(
                matches!(defaulted.reason, RefuseReason::UnknownSlug { .. }),
                "{slug} should be unknown, got {}",
                defaulted.reason
            );
            let removed = remove_model(config, slug)
                .expect_err("an empty builtins array should not treat the slug as built-in");
            assert_eq!(removed.text, config, "{slug} should leave the text unchanged");
            assert!(
                matches!(removed.reason, RefuseReason::NotConfigured { .. }),
                "{slug} should be missing, not built-in, got {}",
                removed.reason
            );
        }

        let updated = set_default(config, "or-glm-5-3")
            .expect("a configured slug should still be accepted");
        assert_eq!(
            updated.matches("[model.").count(),
            1,
            "set-default must not add a model table, got:\n{updated}"
        );
        assert!(updated.contains("default = \"or-glm-5-3\"\n"));
    }

    #[test]
    fn invalid_settings_refuse_the_edit_and_return_the_original_text() {
        let cases = [
            ("openrouter_base_url = \"\"", "openrouter_base_url"),
            ("openrouter_base_url = \"ftp://nope\"", "openrouter_base_url"),
            (
                "openrouter_base_url = \"https://ex ample.invalid/v1\"",
                "openrouter_base_url",
            ),
            (
                "openrouter_base_url = \"https://example.invalid/v1 \"",
                "openrouter_base_url",
            ),
            ("openrouter_base_url = \"http://\"", "openrouter_base_url"),
            ("builtins = \"grok-4.8\"", "builtins"),
            ("builtins = [\"\"]", "builtins"),
            ("builtins = [1]", "builtins"),
            ("builtins = [\"grok-4.8\", \"\"]", "builtins"),
        ];
        for (body, key) in cases {
            assert_invalid_setting(&with_settings(body), key);
        }
    }

    #[test]
    fn read_configured_ignores_an_invalid_grok_router_table() {
        let config = with_settings("openrouter_base_url = \"\"\nbuiltins = [\"\"]");
        let inventory = read_configured(&config)
            .expect("an invalid settings table should not fail the model reader");
        assert_eq!(inventory.saved_default.as_deref(), Some("grok-4.7"));
        assert!(
            inventory
                .models
                .iter()
                .any(|model| model.slug == "or-glm-5-3"),
            "the reader should still report configured models"
        );
        let refused = insert_model(&config, widget(Some(8), false), None)
            .expect_err("the same text should still refuse an edit");
        assert_eq!(refused.text, config, "the refused edit should return the original text");
    }

    /// Prefix `body` as a `[grok_router]` table in front of [`FIXTURE`].
    fn with_settings(body: &str) -> String {
        format!("[grok_router]\n{body}\n\n{FIXTURE}")
    }

    /// Refuse `config` from insert, set-default, and remove, and name `key`.
    fn assert_invalid_setting(config: &str, key: &str) {
        let inserted = insert_model(config, widget(Some(8), false), None)
            .expect_err("insert should refuse an invalid setting");
        assert_eq!(
            inserted.text, config,
            "insert should return the original text for {key}"
        );
        assert_setting(&inserted.reason, key);

        let defaulted = set_default(config, "grok-4.6")
            .expect_err("set-default should refuse an invalid setting");
        assert_eq!(
            defaulted.text, config,
            "set-default should return the original text for {key}"
        );
        assert_setting(&defaulted.reason, key);

        let removed = remove_model(config, "or-deepseek-pro")
            .expect_err("remove should refuse an invalid setting");
        assert_eq!(
            removed.text, config,
            "remove should return the original text for {key}"
        );
        assert_setting(&removed.reason, key);
    }

    /// The refusal names `key` and the display includes that key and the detail.
    fn assert_setting(reason: &RefuseReason, key: &str) {
        match reason {
            RefuseReason::InvalidSetting { key: actual, detail } => {
                assert_eq!(
                    actual, key,
                    "the refusal should name {key}, detail: {detail}"
                );
                assert!(!detail.is_empty(), "{key} should include a detail");
                let rendered = reason.to_string();
                assert!(
                    rendered.contains(key),
                    "display should name {key}, got {rendered}"
                );
                assert!(
                    rendered.contains(detail),
                    "display should include the detail, got {rendered}"
                );
            }
            other => panic!("{key} should be an invalid setting, got {other}"),
        }
    }

    #[test]
    fn router_settings_ignores_provider_tables() {
        assert_compiled(FIXTURE, "a missing [grok_router] table");
        assert_compiled(
            "[grok_router]\n",
            "a [grok_router] table with no provider key",
        );
        assert_compiled(PROVIDERS, "a present provider table");
        assert_compiled(
            "\
[grok_router.provider.deepseek]
base_url = \"https://api.deepseek.com \"
env_key = \"sk-secret\"
",
            "a provider table with an illegal base_url",
        );
    }

    #[test]
    fn direct_provider_reads_the_deepseek_table() {
        let provider = direct_provider(PROVIDERS, "deepseek")
            .expect("the deepseek table should be readable");
        assert_eq!(provider.base_url, "https://api.deepseek.com");
        assert_eq!(provider.env_key, "DEEPSEEK_API_KEY");
        assert_eq!(
            provider.catalog.as_deref(),
            Some("deepseek-list-models"),
            "the verified catalog string should be returned as written"
        );
    }

    #[test]
    fn direct_provider_reads_the_zai_table() {
        let provider =
            direct_provider(PROVIDERS, "zai").expect("the zai table should be readable");
        assert_eq!(provider.base_url, "https://api.z.ai/api/paas/v4");
        assert_eq!(provider.env_key, "ZAI_API_KEY");
        assert_eq!(provider.catalog, None, "an absent catalog key is not an error");
    }

    #[test]
    fn direct_provider_refuses_a_missing_name_and_leaves_the_text_unchanged() {
        let original = PROVIDERS.to_owned();
        for name in ["missing", "Deepseek", "deepseek ", " deepseek"] {
            let refused = direct_provider(&original, name)
                .expect_err("a name that is not an exact table key should be refused");
            assert_eq!(
                original, PROVIDERS,
                "{name:?} should leave the config text unchanged"
            );
            assert_setting(&refused, name);
        }
        let bare = FIXTURE.to_owned();
        let missing = direct_provider(&bare, "deepseek")
            .expect_err("a config with no provider table should refuse the name");
        assert_eq!(bare, FIXTURE, "a missing table should leave the config text unchanged");
        assert_setting(&missing, "deepseek");
    }

    #[test]
    fn direct_provider_rejects_a_base_url_that_fails_the_http_rule() {
        let spaced = one_provider(
            "deepseek",
            "base_url = \"https://api.deepseek.com \"\nenv_key = \"DEEPSEEK_API_KEY\"\n",
        );
        let spaced_error = direct_provider(&spaced, "deepseek")
            .expect_err("a trailing space in base_url should not be trimmed");
        assert_setting(&spaced_error, "base_url");

        let number = one_provider("deepseek", "base_url = 1\nenv_key = \"DEEPSEEK_API_KEY\"\n");
        let number_error = direct_provider(&number, "deepseek")
            .expect_err("a non-string base_url should be refused");
        assert_setting(&number_error, "base_url");

        let slash = one_provider(
            "deepseek",
            "\
base_url = \"https://api.deepseek.com/\"
env_key = \"DEEPSEEK_API_KEY\"
catalog = \"deepseek-list-models\"
",
        );
        let provider = direct_provider(&slash, "deepseek")
            .expect("a trailing slash should stay part of the base");
        assert_eq!(provider.base_url, "https://api.deepseek.com/");
        assert_eq!(provider.catalog.as_deref(), Some("deepseek-list-models"));
    }

    #[test]
    fn direct_provider_rejects_an_env_key_outside_the_shell_name_rule() {
        for key in ["sk-secret", "1KEY"] {
            let config = one_provider(
                "deepseek",
                &format!("base_url = \"https://api.deepseek.com\"\nenv_key = \"{key}\"\n"),
            );
            let error = direct_provider(&config, "deepseek")
                .expect_err("an env_key outside the shell name rule should be refused");
            assert_setting(&error, "env_key");
            let rendered = error.to_string();
            assert!(
                rendered.starts_with("env_key is invalid:"),
                "display should keep the invalid-setting shape, got {rendered}"
            );
        }
    }

    #[test]
    fn direct_provider_refuses_the_deepseek_catalog_on_another_base() {
        let config = one_provider(
            "zai",
            "\
base_url = \"https://api.z.ai/api/paas/v4\"
env_key = \"ZAI_API_KEY\"
catalog = \"deepseek-list-models\"
",
        );
        let error = direct_provider(&config, "zai").expect_err(
            "deepseek-list-models on a base other than the two legal strings should be refused",
        );
        assert_setting(&error, "catalog");
    }

    #[test]
    fn direct_provider_returns_catalog_strings_that_are_not_a_verified_document() {
        let empty = one_provider(
            "zai",
            "\
base_url = \"https://api.z.ai/api/paas/v4\"
env_key = \"ZAI_API_KEY\"
catalog = \"\"
",
        );
        let empty_provider = direct_provider(&empty, "zai")
            .expect("an empty catalog string should be returned, not refused");
        assert_eq!(empty_provider.catalog.as_deref(), Some(""));

        let other = one_provider(
            "zai",
            "\
base_url = \"https://api.z.ai/api/paas/v4\"
env_key = \"ZAI_API_KEY\"
catalog = \"not-a-document\"
",
        );
        let other_provider = direct_provider(&other, "zai")
            .expect("an unverified catalog string should be returned, not refused");
        assert_eq!(other_provider.catalog.as_deref(), Some("not-a-document"));
    }

    /// Require `config` to resolve to the compiled base and the four built-in slugs.
    fn assert_compiled(config: &str, situation: &str) {
        let settings = router_settings(config)
            .unwrap_or_else(|error| panic!("{situation} should resolve, got {error}"));
        assert_eq!(
            settings.base_url, OPENROUTER_BASE_URL,
            "{situation} should keep the compiled base"
        );
        assert_eq!(
            settings.builtins,
            BUILTIN_SLUGS.map(str::to_owned).to_vec(),
            "{situation} should keep the four compiled slugs"
        );
    }

    /// One provider table whose header key is the bare name `name`.
    fn one_provider(name: &str, body: &str) -> String {
        format!("[grok_router.provider.{name}]\n{body}")
    }

    #[test]
    fn replace_file_backs_up_then_writes_bytes_that_omit_the_secret() {
        let tree = TempTree::new("backup");
        let config = tree.path().join("config.toml");
        let secret = "sk-live-should-not-be-written";
        std::fs::write(&config, FIXTURE).expect("the temp config should be writable");
        let updated = set_default(FIXTURE, "grok-4.6")
            .expect("the replacement bytes should be a successful set-default");
        assert!(
            !updated.contains(secret),
            "the replacement fixture must not already contain the secret"
        );

        replace_file(&config, Backup::Sibling, updated.as_bytes(), secret)
            .expect("a secret-free replace should write the sibling backup and the new bytes");
        let sibling = tree.path().join("config.toml.prev");
        assert_eq!(
            std::fs::read(&sibling).expect("the sibling backup should exist"),
            FIXTURE.as_bytes(),
            "the sibling backup should be the previous bytes"
        );
        assert!(
            !tree.path().join("config.prev").exists(),
            "the backup name should keep the original suffix and append .prev"
        );
        let written = std::fs::read(&config).expect("the replaced config should exist");
        assert_eq!(written, updated.as_bytes());
        assert!(
            !written.windows(secret.len()).any(|window| window == secret.as_bytes()),
            "the written bytes should not contain the caller-supplied secret"
        );

        std::fs::write(&config, FIXTURE).expect("the config should reset for the cache-dir backup");
        let cache = tree.path().join("cache");
        replace_file(&config, Backup::CacheDir(&cache), updated.as_bytes(), secret)
            .expect("the default-config backup should go under the cache dir");
        assert_eq!(
            std::fs::read(cache.join("config.toml.prev"))
                .expect("cache-dir backup should exist"),
            FIXTURE.as_bytes()
        );
    }

    #[test]
    fn replace_file_refuses_bytes_that_contain_the_secret() {
        let tree = TempTree::new("secret");
        let config = tree.path().join("custom.toml");
        let secret = "sk-live-should-not-be-written";
        std::fs::write(&config, FIXTURE).expect("the temp config should be writable");
        let poisoned = format!("{FIXTURE}{secret}");
        let refused = replace_file(&config, Backup::Sibling, poisoned.as_bytes(), secret)
            .expect_err("bytes that contain the secret should be refused");
        assert!(matches!(
            refused.kind,
            ReplaceKind::SecretInReplacement
        ));
        assert_eq!(
            std::fs::read(&config).expect("the original file should still be readable"),
            FIXTURE.as_bytes(),
            "a secret refusal should not write the config"
        );
        assert!(
            !tree.path().join("custom.toml.prev").exists(),
            "a secret refusal should not write the backup"
        );
    }

    /// Isolated directory for file-replace tests. Cleanup is best-effort.
    struct TempTree {
        path: PathBuf,
    }

    impl TempTree {
        fn new(label: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "grok-router-config-edit-{}-{label}",
                std::process::id()
            ));
            std::fs::create_dir_all(&path)
                .expect("creating an isolated config directory should succeed");
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TempTree {
        fn drop(&mut self) {
            if let Err(error) = std::fs::remove_dir_all(&self.path) {
                let _ignored = error.kind();
            }
        }
    }
}
