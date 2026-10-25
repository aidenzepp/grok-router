//! Splice Grok model blocks inside config text.
//!
//! The edit inserts, replaces, or deletes exact spans. It does not serialize
//! the document back out. After a splice, the text is parsed again and refused
//! when a top-level table, an existing provider id, the intended membership, or
//! the saved default drifts. Refusal returns the caller's original text.
//! [`read_configured`] reports the saved default and configured models and does
//! not change the text. [`resolve_provider`] reads one
//! `[router.provider.<name>]` record and does not fall back to a compiled
//! `OpenRouter` URL. [`builtins_from`] reads `builtins` from `[router]`.
//! [`init_router_text`] appends a default `OpenRouter` `[router]` table when that
//! key is absent and leaves every earlier byte in place.
//! None of the readers write the document. Model splices do not write `[router]` back.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};

/// Built-in Grok slugs used when `builtins` is absent. They have no config block.
pub const BUILTIN_SLUGS: [&str; 4] = ["grok-4.7", "grok-4.7-build-fast", "grok-4.6", "grok-4.5"];

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

/// One `[router.provider.<name>]` record, as owned strings.
///
/// Blank `catalog` is [`None`]. Any other string is kept, including an unknown id.
/// Reading it does not rewrite the document or select a cartridge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderRecord {
    /// `base_url`, kept exactly, including a trailing slash.
    pub base_url: String,
    /// `env_key`. This is the variable name, not its value.
    pub env_key: String,
    /// `catalog` when that key is a non-empty string.
    pub catalog: Option<String>,
}

/// Which `[router.provider.*]` record [`resolve_provider`] should read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderChoice<'a> {
    /// The `--provider` name, compared exactly. Empty is a missing name, not [`Self::Default`].
    Explicit(&'a str),
    /// The `default_provider` key on `[router]`.
    Default,
}

/// What `init` should do with config text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouterInit {
    /// Append these bytes. Earlier bytes stay. A non-empty file gains one blank line before the block.
    Append(String),
    /// `[router]` is already a table. Do not write.
    AlreadyPresent,
}

/// Why `init` refused to build a router block. The caller's text is not rewritten.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RouterInitError {
    /// The original text, or the text after the block was appended, did not parse.
    #[error("config.toml did not parse: {detail}")]
    Parse {
        /// Parser message. The original text is not rewritten.
        detail: String,
        /// True when the appended text failed. False when the original text failed.
        appended: bool,
    },
    /// A `[router]` key is present and is not a table.
    #[error("[router] is not a table")]
    NotTable,
    /// Re-parse showed a top-level table that was already there and did not compare equal.
    #[error("edit would change existing tables: {}", joined_tables(tables))]
    ChangedTables {
        /// Table names that changed or disappeared.
        tables: Vec<String>,
    },
}

/// Why a text edit was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RefuseReason {
    /// The catalog row did not include a provider id.
    #[error("catalog row has no id")]
    EmptyProviderId,
    /// The provider id's last path segment did not yield a slug.
    #[error("cannot derive a slug from {provider_id:?}; pass --as")]
    EmptySlugLeaf {
        /// Provider id that could not be turned into a slug.
        provider_id: String,
    },
    /// The slug does not match the allowed shape.
    #[error(
        "slug {slug:?} must start with a letter or digit and contain only letters, digits, dots, underscores, and hyphens"
    )]
    InvalidSlug {
        /// Slug that was rejected.
        slug: String,
    },
    /// `context_window` was missing or not greater than zero.
    #[error("{provider_id} has no positive context window")]
    NonPositiveContext {
        /// Provider id whose context window was rejected.
        provider_id: String,
    },
    /// The provider id is already configured under a different slug.
    #[error("{provider_id} is already {existing_slug}")]
    DuplicateProvider {
        /// Provider id that is already present.
        provider_id: String,
        /// Slug already bound to that provider id at the resolved base URL.
        existing_slug: String,
    },
    /// The slug is already a configured model or a built-in.
    #[error("slug {slug} is already in use; pass --as")]
    SlugInUse {
        /// Slug that is already taken.
        slug: String,
    },
    /// Remove was asked for a slug that has no config block.
    #[error("{slug} is not in the config")]
    NotConfigured {
        /// Slug that is absent from the config.
        slug: String,
    },
    /// Remove was asked for a built-in slug.
    #[error("{slug} is a built-in and has no config block")]
    BuiltIn {
        /// Built-in slug that has no block to delete.
        slug: String,
    },
    /// Remove was asked for the saved default.
    #[error("{slug} is the saved default")]
    IsDefault {
        /// Slug that is `[models].default`.
        slug: String,
    },
    /// `set-default` was asked for a slug that is neither built-in nor configured.
    #[error("{slug} is not a built-in or a configured model")]
    UnknownSlug {
        /// Slug that cannot become the default.
        slug: String,
    },
    /// The document has more than one `[models]` header.
    #[error("more than one [models] table")]
    MultipleModelsTables,
    /// The document has more than one header for the same model slug.
    #[error("{slug} appears more than once")]
    DuplicateSlugHeader {
        /// Slug whose header appears more than once.
        slug: String,
    },
    /// The document, or the spliced text, did not parse.
    #[error("config.toml did not parse: {detail}")]
    Parse {
        /// Parser message. The original text is returned separately.
        detail: String,
    },
    /// Re-parse showed a top-level table that the splice dropped.
    #[error("edit dropped config tables: {}", joined_tables(tables))]
    DroppedTables {
        /// Table names present before the splice and absent after it.
        tables: Vec<String>,
    },
    /// Re-parse showed an existing model whose provider id changed.
    #[error("edit changed the existing model {slug}")]
    ChangedProvider {
        /// Slug whose provider id was not preserved.
        slug: String,
    },
    /// Re-parse did not find the model the splice claimed to add.
    #[error("edit did not add {slug}")]
    AddMissing {
        /// Slug that should have been added.
        slug: String,
    },
    /// Re-parse still found the model the splice claimed to remove.
    #[error("edit did not remove {slug}")]
    RemoveFailed {
        /// Slug that should have been removed.
        slug: String,
    },
    /// Re-parse showed a different saved default than the splice required.
    #[error(
        "default is {}, expected {}",
        display_optional(actual),
        display_optional(expected)
    )]
    DefaultMismatch {
        /// Default the splice required, if any.
        expected: Option<String>,
        /// Default the re-parse observed, if any.
        actual: Option<String>,
    },
    /// A present `[grok_router]` key cannot be used.
    #[error("{key} is invalid: {detail}")]
    InvalidSetting {
        /// `builtins`, `base_url`, `env_key`, `catalog`, `default_provider`,
        /// `router`, `provider`, or a provider name.
        key: String,
        /// Constraint the value failed.
        detail: String,
    },
}

impl RefuseReason {
    /// `Snake_case` name of this refusal.
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::EmptyProviderId => "empty_provider_id",
            Self::EmptySlugLeaf { .. } => "empty_slug_leaf",
            Self::InvalidSlug { .. } => "invalid_slug",
            Self::NonPositiveContext { .. } => "non_positive_context",
            Self::DuplicateProvider { .. } => "duplicate_provider",
            Self::SlugInUse { .. } => "slug_in_use",
            Self::NotConfigured { .. } => "not_configured",
            Self::BuiltIn { .. } => "built_in",
            Self::IsDefault { .. } => "is_default",
            Self::UnknownSlug { .. } => "unknown_slug",
            Self::MultipleModelsTables => "multiple_models_tables",
            Self::DuplicateSlugHeader { .. } => "duplicate_slug_header",
            Self::Parse { .. } => "parse",
            Self::DroppedTables { .. } => "dropped_tables",
            Self::ChangedProvider { .. } => "changed_provider",
            Self::AddMissing { .. } => "add_missing",
            Self::RemoveFailed { .. } => "remove_failed",
            Self::DefaultMismatch { .. } => "default_mismatch",
            Self::InvalidSetting { .. } => "invalid_setting",
        }
    }
}

/// Join dropped table names the way the refusal sentence lists them.
fn joined_tables(tables: &[String]) -> String {
    tables.join(", ")
}

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
#[derive(Debug, thiserror::Error)]
#[error("{detail}")]
pub struct ReplaceError {
    /// Which step failed.
    pub kind: ReplaceKind,
    /// Path and source error. This string does not include the secret or the file bytes.
    pub detail: String,
}

impl ReplaceError {
    /// `Snake_case` name of the failed replace step.
    #[must_use]
    pub const fn failure_kind(&self) -> &'static str {
        match self.kind {
            ReplaceKind::SecretInReplacement => "secret_in_replacement",
            ReplaceKind::Read => "read",
            ReplaceKind::Backup => "backup",
            ReplaceKind::Temp => "temp",
            ReplaceKind::Rename => "rename",
        }
    }
}

/// A named environment value blocked the write. The value itself is not stored.
#[derive(Debug, thiserror::Error)]
pub enum SecretError {
    /// The variable is set to bytes that are not Unicode. The file was left unchanged.
    #[error(
        "{name} is not valid Unicode, so the config was not written. Export a Unicode value or unset {name}. The file was left unchanged."
    )]
    NotUnicode {
        /// Variable that could not be read.
        name: String,
    },
    /// The new text contains the variable's value. The file was left unchanged.
    #[error(
        "the new config contains the value of {name}. The file was left unchanged. Remove that value from the text before writing."
    )]
    ValueInConfig {
        /// Variable whose value appeared in the text.
        name: String,
    },
}

impl SecretError {
    /// `Snake_case` name of this secret refusal.
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::NotUnicode { .. } => "not_unicode",
            Self::ValueInConfig { .. } => "value_in_config",
        }
    }
}

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

/// Read one `[router.provider.<name>]` record from `config`.
///
/// [`ProviderChoice::Explicit`] uses that name and does not read `default_provider`.
/// [`ProviderChoice::Default`] reads `default_provider` and refuses when `[router]`
/// is missing, when that key is missing, blank, or not a string, or when the
/// named record is missing. A miss is [`RefuseReason::InvalidSetting`]. It is
/// not a compiled `OpenRouter` URL, and another provider record is not a silent
/// default. `base_url` must match `^https?://\S+$` and `env_key` must match
/// `^[A-Za-z_][A-Za-z0-9_]*$`, both with no trim. Absent or blank `catalog`
/// is [`None`]. Any other string is returned as written. This function does
/// not check a catalog id against a base URL, does not select a cartridge,
/// does not read `[grok_router]`, and does not validate `builtins`.
///
/// # Errors
///
/// Returns [`RefuseReason::InvalidSetting`] when the choice has no record or a
/// present key cannot be used, or a parse or structural error when the
/// document cannot be read. The caller's text is not rewritten.
#[must_use = "the provider record is the caller's only copy of the base, env key, and catalog"]
pub fn resolve_provider(
    config: &str,
    choice: ProviderChoice<'_>,
) -> Result<ProviderRecord, RefuseReason> {
    structural_problem(config)?;
    let root = parsed_root(config)?;
    match choice {
        ProviderChoice::Explicit(name) => explicit_record(&root, name),
        ProviderChoice::Default => default_record(&root),
    }
}

/// Read `builtins` from `[router]`.
///
/// A missing `[router]` table, or a missing key, returns [`BUILTIN_SLUGS`].
/// An empty array returns none. The list is not read from a provider record
/// or from `[grok_router]`. This function does not require `default_provider`
/// and does not select a cartridge.
///
/// # Errors
///
/// Returns [`RefuseReason::InvalidSetting`] when `[router]` is not a table or
/// `builtins` is not an array of non-empty strings, or a parse or structural
/// error when the document cannot be read.
#[must_use = "the built-in slugs are the caller's only copy of the resolved list"]
pub fn builtins_from(config: &str) -> Result<Vec<String>, RefuseReason> {
    structural_problem(config)?;
    let root = parsed_root(config)?;
    match root.get("router") {
        None => Ok(compiled_builtins()),
        Some(value) => {
            let Some(table) = value.as_table() else {
                return Err(invalid_setting("router", "[router] must be a table"));
            };
            match table.get("builtins") {
                None => Ok(compiled_builtins()),
                Some(builtins) => builtin_list(builtins),
            }
        }
    }
}

/// Render one new model block, including its trailing newline.
///
/// `base_url` and `env_key` are written on this span only. `env_key` is the
/// variable name the caller already resolved. This function does not read
/// `[grok_router]` and does not invent `OPENROUTER_API_KEY`.
///
/// # Errors
///
/// Returns an error when the slug, provider id, or context window is unusable.
pub fn render_block(
    slug: &str,
    model: CatalogModel<'_>,
    base_url: &str,
    env_key: &str,
) -> Result<String, RefuseReason> {
    require_slug(slug)?;
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
        format!("env_key = {}", toml_basic_string(env_key)),
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

/// Insert one model block into `config`.
///
/// `slug` is the slug the caller already chose. This function calls
/// [`require_slug`] and does not add an `or-` prefix or read `[grok_router]`.
/// The same provider id at `base_url` with the same slug returns `config`
/// unchanged. The same id at `base_url` under a different slug is
/// [`RefuseReason::DuplicateProvider`]. The same id at a different base is not
/// a duplicate. Built-in membership comes from [`builtins_from`].
///
/// # Errors
///
/// Returns [`Refused`] and the original text when the add is refused.
pub fn insert_model(
    config: &str,
    model: CatalogModel<'_>,
    slug: &str,
    base_url: &str,
    env_key: &str,
) -> Result<String, Refused> {
    let builtins = require_builtins(config)?;
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
    if let Err(reason) = require_slug(slug) {
        return Err(refuse(config, reason));
    }
    let matches = provider_slugs(&root, model.id, base_url);
    if let Some(existing) = matches.iter().find(|existing| existing.as_str() != slug) {
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
    if is_builtin(&builtins, slug) || configured_models(&root).contains_key(slug) {
        return Err(refuse(
            config,
            RefuseReason::SlugInUse {
                slug: slug.to_owned(),
            },
        ));
    }
    let block = match render_block(slug, model, base_url, env_key) {
        Ok(block) => block,
        Err(reason) => return Err(refuse(config, reason)),
    };
    let spliced = insert_block(config, &block);
    finish(
        config,
        &root,
        spliced,
        Some(slug),
        None,
        DefaultExpectation::Unchanged,
        Some(model.id),
        None,
    )
}

/// One direct model span. Absent optionals omit their lines.
///
/// `name`, `context_window`, and `vision` are independent of each other.
/// `context_window` is written only when it is greater than zero. `vision`
/// writes `vision = true` and never `vision = false`.
#[derive(Debug, Clone, Copy)]
pub struct DirectBlock<'a> {
    /// Header slug. Not taken from [`Self::model`].
    pub slug: &'a str,
    /// Value of the `model` line.
    pub model: &'a str,
    /// Value of the `base_url` line. Not trimmed.
    pub base_url: &'a str,
    /// Value of the `name` line. `None` omits the line. `Some("")` is refused.
    pub name: Option<&'a str>,
    /// Value of the `api_backend` line. Written as supplied.
    pub api_backend: &'a str,
    /// Value of the `env_key` line. The variable name, not its value.
    pub env_key: &'a str,
    /// Value of the `context_window` line. `None` omits it. Zero and below are refused.
    pub context_window: Option<i64>,
    /// Whether to write `vision = true` after the window, or after `env_key` when the window is omitted.
    pub vision: bool,
}

/// Insert one direct model block into `config`.
///
/// Renders the span itself and splices it with the same block insert the
/// catalog path uses. Does not call [`render_block`] or [`insert_model`], and
/// does not treat two slugs that
/// store the same model id as duplicates. An existing span for `block.slug`
/// that already equals the rendered block returns `config`. A different span
/// for that slug, or a built-in slug from [`builtins_from`], is
/// [`RefuseReason::SlugInUse`]. An absent `[router]` table still uses the
/// compiled built-in slugs.
///
/// # Errors
///
/// Returns [`Refused`] and the original text when the block is unusable, the
/// slug is taken, or the splice would not preserve the document.
pub fn insert_direct(config: &str, block: DirectBlock<'_>) -> Result<String, Refused> {
    let builtins = require_builtins(config)?;
    let root = read_config(config)?;
    let rendered = match render_direct_block(&block) {
        Ok(rendered) => rendered,
        Err(reason) => return Err(refuse(config, reason)),
    };
    if is_builtin(&builtins, block.slug) {
        return Err(refuse(
            config,
            RefuseReason::SlugInUse {
                slug: block.slug.to_owned(),
            },
        ));
    }
    match model_span_text(config, block.slug) {
        Ok(Some(existing)) if existing == rendered => Ok(config.to_owned()),
        Ok(Some(_)) => Err(refuse(
            config,
            RefuseReason::SlugInUse {
                slug: block.slug.to_owned(),
            },
        )),
        Ok(None) => {
            let spliced = insert_block(config, &rendered);
            finish(
                config,
                &root,
                spliced,
                Some(block.slug),
                None,
                DefaultExpectation::Unchanged,
                Some(block.model),
                None,
            )
        }
        Err(reason) => Err(refuse(config, reason)),
    }
}

/// Render one direct block, including its trailing newline and no extra blank line.
///
/// Field order is `model`, `base_url`, `name` when present, `api_backend`,
/// `env_key`, `context_window` when greater than zero, then `vision = true`
/// only when asked. Values are not trimmed. The model id is not copied into
/// the slug or the display name.
///
/// # Errors
///
/// Returns an error when the slug, model id, base URL, env name, display name,
/// api backend, or context window cannot be written as specified.
fn render_direct_block(block: &DirectBlock<'_>) -> Result<String, RefuseReason> {
    require_slug(block.slug)?;
    if block.model.is_empty() {
        return Err(invalid_setting(
            "model",
            "must be non-empty; the value is not trimmed and is not used as the slug",
        ));
    }
    if !is_http_base(block.base_url) {
        return Err(invalid_setting(
            "base_url",
            "must match ^https?://\\S+$ with no trim",
        ));
    }
    if let Some(name) = block.name
        && name.is_empty()
    {
        return Err(invalid_setting(
            "name",
            "must be non-empty when passed; the value is not trimmed and is not copied from the model id",
        ));
    }
    if block.api_backend.is_empty() {
        return Err(invalid_setting(
            "api_backend",
            "must be non-empty; the value is not trimmed",
        ));
    }
    if !is_env_key(block.env_key) {
        return Err(invalid_setting(
            "env_key",
            "must match ^[A-Za-z_][A-Za-z0-9_]*$ with no trim",
        ));
    }
    let context_window = match block.context_window {
        None => None,
        Some(window) if window > 0 => Some(window),
        Some(window) => {
            return Err(invalid_setting(
                "context_window",
                &format!("must be greater than zero, got {window}"),
            ));
        }
    };
    let mut lines = vec![
        model_header(block.slug)?,
        format!("model = {}", toml_basic_string(block.model)),
        format!("base_url = {}", toml_basic_string(block.base_url)),
    ];
    if let Some(name) = block.name {
        lines.push(format!("name = {}", toml_basic_string(name)));
    }
    lines.push(format!(
        "api_backend = {}",
        toml_basic_string(block.api_backend)
    ));
    lines.push(format!("env_key = {}", toml_basic_string(block.env_key)));
    if let Some(window) = context_window {
        lines.push(format!("context_window = {window}"));
    }
    if block.vision {
        lines.push("vision = true".to_owned());
    }
    let mut rendered = lines.join("\n");
    rendered.push('\n');
    Ok(rendered)
}

/// The span [`delete_span`] would remove, without trailing blank lines.
///
/// `None` means the slug has no header. This copies no other bytes and does
/// not call [`delete_span`].
///
/// # Errors
///
/// Returns [`RefuseReason::DuplicateSlugHeader`] when the slug header appears
/// more than once, or a header-decode error from the span walk.
fn model_span_text(text: &str, slug: &str) -> Result<Option<String>, RefuseReason> {
    let lines_ref = split_keepends(text);
    let spans = table_spans(&lines_ref);
    let mut found = Vec::new();
    for span in &spans {
        if let Some(header_slug) = model_slug_from_header(&span.name)?
            && header_slug == slug
        {
            found.push((span.start, span.end));
        }
    }
    let (start, end) = match found.as_slice() {
        [] => return Ok(None),
        [(start, end)] => (*start, *end),
        _ => {
            return Err(RefuseReason::DuplicateSlugHeader {
                slug: slug.to_owned(),
            });
        }
    };
    let mut raw = String::new();
    for (index, line) in lines_ref.iter().enumerate() {
        if index >= start && index < end {
            raw.push_str(line);
        }
    }
    Ok(Some(without_trailing_blank_lines(&raw)))
}

/// Drop blank lines from the end of `text` and keep the last content line's newline.
fn without_trailing_blank_lines(text: &str) -> String {
    let lines = split_keepends(text);
    let mut end = lines.len();
    while end > 0 {
        let blank = lines
            .get(end.saturating_sub(1))
            .is_some_and(|line| line.trim().is_empty());
        if blank {
            end = end.saturating_sub(1);
        } else {
            break;
        }
    }
    let mut kept = String::new();
    for (index, line) in lines.iter().enumerate() {
        if index < end {
            kept.push_str(line);
        }
    }
    kept
}

/// Replace the `default` line inside the single `[models]` table.
///
/// Creates that table only when the header is absent. Model spans are not rewritten.
/// A built-in slug is one [`builtins_from`] returns. This function does not read
/// `default_provider` and does not resolve a cartridge.
///
/// # Errors
///
/// Returns [`Refused`] and the original text when the slug is unknown or the document is unsafe to edit.
pub fn set_default(config: &str, slug: &str) -> Result<String, Refused> {
    let builtins = require_builtins(config)?;
    if let Err(reason) = require_slug(slug) {
        return Err(refuse(config, reason));
    }
    let root = read_config(config)?;
    if !is_builtin(&builtins, slug) && !configured_models(&root).contains_key(slug) {
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
        None,
    )
}

/// Delete one model table span and any child tables under that slug.
///
/// A child header is `[model.<slug>.<rest>]` or `[model."<slug>".<rest>]`.
/// A different slug that merely contains this slug as a prefix is left in place.
///
/// # Errors
///
/// Returns [`Refused`] and the original text for a missing slug, a built-in, the saved default,
/// or a document the splice will not rewrite. Built-in membership comes from
/// [`builtins_from`]. This function does not read `default_provider`.
pub fn remove_model(config: &str, slug: &str) -> Result<String, Refused> {
    let builtins = require_builtins(config)?;
    if let Err(reason) = require_slug(slug) {
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
    if is_builtin(&builtins, slug) {
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
        None,
    )
}

/// Keys `edit` may set or clear. `model` may be set and may not be cleared.
const EDITABLE_FIELDS: [&str; 13] = [
    "model",
    "base_url",
    "name",
    "description",
    "env_key",
    "api_key",
    "api_backend",
    "context_window",
    "max_completion_tokens",
    "reasoning_effort",
    "vision",
    "stream_tool_calls",
    "supports_reasoning_effort",
];

/// Whether `key` is a field [`edit_model`] can set or clear.
#[must_use]
pub fn is_editable_field(key: &str) -> bool {
    EDITABLE_FIELDS.contains(&key)
}

/// One field to set or clear inside an existing model block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldChange {
    /// Bare assignment key, such as `name` or `context_window`.
    pub key: String,
    /// The new value, or [`FieldAction::Clear`].
    pub action: FieldAction,
}

/// What to do with one key inside a model block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldAction {
    /// Write a TOML string. The value is not trimmed.
    SetText(String),
    /// Write a TOML integer.
    SetInt(i64),
    /// Write `true` or `false`.
    SetBool(bool),
    /// Delete the assignment line. `model` cannot be cleared.
    Clear,
}

/// One model as stored, or a built-in that has no block.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ShownModel {
    /// Slug that was asked for.
    pub slug: String,
    /// True when the slug is a built-in and has no `[model]` block.
    pub builtin: bool,
    /// True when `[models].default` is this slug.
    #[serde(rename = "default")]
    pub is_default: bool,
    /// Exact block text, including child tables. Empty when [`Self::builtin`] is true.
    pub body: String,
}

/// Read one configured model, or a built-in that has no block.
///
/// The body is the original header span plus any child tables. Comments and
/// unrelated keys inside that span stay. This function does not write.
///
/// # Errors
///
/// Returns [`Refused`] when the slug is neither configured nor built-in, or the
/// document cannot be read. The original text is returned with the refusal.
pub fn show_model(config: &str, slug: &str) -> Result<ShownModel, Refused> {
    let builtins = require_builtins(config)?;
    if let Err(reason) = require_slug(slug) {
        return Err(refuse(config, reason));
    }
    let root = read_config(config)?;
    let is_default = saved_default(&root) == Some(slug);
    let configured = configured_models(&root).contains_key(slug);
    if !configured && !is_builtin(&builtins, slug) {
        return Err(refuse(
            config,
            RefuseReason::NotConfigured {
                slug: slug.to_owned(),
            },
        ));
    }
    if !configured {
        return Ok(ShownModel {
            slug: slug.to_owned(),
            builtin: true,
            is_default,
            body: String::new(),
        });
    }
    let located = match locate_model(config, slug) {
        Ok(located) => located,
        Err(reason) => return Err(refuse(config, reason)),
    };
    let mut ranges = vec![located.parent];
    ranges.extend(located.children);
    Ok(ShownModel {
        slug: slug.to_owned(),
        builtin: false,
        is_default,
        body: ranges_text(&located.lines, &ranges),
    })
}

/// Replace or clear fields inside one existing model block.
///
/// Only the parent span is rewritten. Child tables, comments on other lines,
/// and every other table stay. A missing key is appended before the span's
/// trailing blank lines. A key that already has the rendered assignment is left
/// byte-identical. `model` may change the provider id and may not be cleared.
///
/// # Errors
///
/// Returns [`Refused`] and the original text when the slug has no block, a field
/// is unusable, or the splice would not preserve the document.
pub fn edit_model(config: &str, slug: &str, changes: &[FieldChange]) -> Result<String, Refused> {
    if let Err(reason) = validate_changes(changes) {
        return Err(refuse(config, reason));
    }
    let builtins = require_builtins(config)?;
    if let Err(reason) = require_slug(slug) {
        return Err(refuse(config, reason));
    }
    let root = read_config(config)?;
    if is_builtin(&builtins, slug) && !configured_models(&root).contains_key(slug) {
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
    let spliced = match rewrite_model_fields(config, slug, changes) {
        Ok(text) => text,
        Err(reason) => return Err(refuse(config, reason)),
    };
    let edited_provider = changes.iter().find_map(|change| match &change.action {
        FieldAction::SetText(value) if change.key == "model" => Some((slug, value.as_str())),
        _ => None,
    });
    finish(
        config,
        &root,
        spliced,
        None,
        None,
        DefaultExpectation::Unchanged,
        None,
        edited_provider,
    )
}

/// Refuse an empty edit, an unknown key, a cleared `model`, or a value of the wrong shape.
///
/// # Errors
///
/// Returns [`RefuseReason::InvalidSetting`] when a change cannot be written.
fn validate_changes(changes: &[FieldChange]) -> Result<(), RefuseReason> {
    if changes.is_empty() {
        return Err(invalid_setting(
            "edit",
            "pass a field to change, or --clear a field",
        ));
    }
    let mut seen = BTreeSet::new();
    for change in changes {
        if !seen.insert(change.key.as_str()) {
            return Err(invalid_setting(&change.key, "was passed more than once"));
        }
        validate_change(change)?;
    }
    Ok(())
}

/// Refuse one change whose key or value cannot be written.
///
/// # Errors
///
/// Returns [`RefuseReason::InvalidSetting`] for an unknown key, a cleared `model`,
/// or a value that fails that field's constraint.
fn validate_change(change: &FieldChange) -> Result<(), RefuseReason> {
    if !is_editable_field(&change.key) {
        return Err(invalid_setting(
            &change.key,
            "is not an editable model field",
        ));
    }
    if matches!(change.action, FieldAction::Clear) {
        if change.key == "model" {
            return Err(invalid_setting(
                "model",
                "cannot be cleared; pass --model with the provider id",
            ));
        }
        return Ok(());
    }
    match change.key.as_str() {
        "context_window" | "max_completion_tokens" => validate_positive_int(change),
        "vision" | "stream_tool_calls" | "supports_reasoning_effort" => validate_bool(change),
        "base_url" => validate_base_url(change),
        "env_key" => validate_env_key_change(change),
        _ => validate_non_empty_text(change),
    }
}

/// Refuse a window or token limit that is not an integer greater than zero.
///
/// # Errors
///
/// Returns [`RefuseReason::InvalidSetting`] when the action is not [`FieldAction::SetInt`]
/// or the integer is not greater than zero.
fn validate_positive_int(change: &FieldChange) -> Result<(), RefuseReason> {
    match change.action {
        FieldAction::SetInt(value) if value > 0 => Ok(()),
        FieldAction::SetInt(value) => Err(invalid_setting(
            &change.key,
            &format!("must be greater than zero, got {value}"),
        )),
        _ => Err(invalid_setting(&change.key, "must be an integer")),
    }
}

/// Refuse a boolean field written as text or an integer.
///
/// # Errors
///
/// Returns [`RefuseReason::InvalidSetting`] when the action is not [`FieldAction::SetBool`].
fn validate_bool(change: &FieldChange) -> Result<(), RefuseReason> {
    if matches!(change.action, FieldAction::SetBool(_)) {
        Ok(())
    } else {
        Err(invalid_setting(&change.key, "must be true or false"))
    }
}

/// Refuse a base URL that is not `http://` or `https://` plus non-whitespace.
///
/// # Errors
///
/// Returns [`RefuseReason::InvalidSetting`] when the value fails [`is_http_base`].
fn validate_base_url(change: &FieldChange) -> Result<(), RefuseReason> {
    match &change.action {
        FieldAction::SetText(value) if is_http_base(value) => Ok(()),
        FieldAction::SetText(_) => Err(invalid_setting(
            "base_url",
            "must match ^https?://\\S+$ with no trim",
        )),
        _ => Err(invalid_setting("base_url", "must be a string")),
    }
}

/// Refuse an environment variable name that is not a shell identifier.
///
/// # Errors
///
/// Returns [`RefuseReason::InvalidSetting`] when the value fails [`is_env_key`].
fn validate_env_key_change(change: &FieldChange) -> Result<(), RefuseReason> {
    match &change.action {
        FieldAction::SetText(value) if is_env_key(value) => Ok(()),
        FieldAction::SetText(_) => Err(invalid_setting(
            "env_key",
            "must match ^[A-Za-z_][A-Za-z0-9_]*$ with no trim",
        )),
        _ => Err(invalid_setting("env_key", "must be a string")),
    }
}

/// Refuse an empty string on a text field. The value is not trimmed.
///
/// # Errors
///
/// Returns [`RefuseReason::InvalidSetting`] when the text is empty or not text.
fn validate_non_empty_text(change: &FieldChange) -> Result<(), RefuseReason> {
    match &change.action {
        FieldAction::SetText(value) if !value.is_empty() => Ok(()),
        FieldAction::SetText(_) => Err(invalid_setting(
            &change.key,
            "must be non-empty; the value is not trimmed",
        )),
        _ => Err(invalid_setting(&change.key, "must be a string")),
    }
}

/// Splice the parent span. Child tables are not part of that span.
///
/// # Errors
///
/// Returns a refusal when the header is missing or a key cannot be rewritten.
fn rewrite_model_fields(
    config: &str,
    slug: &str,
    changes: &[FieldChange],
) -> Result<String, RefuseReason> {
    let located = locate_model(config, slug)?;
    let (start, end) = located.parent;
    let mut span = Vec::new();
    for index in start..end {
        if let Some(line) = located.lines.get(index) {
            span.push((*line).to_owned());
        }
    }
    apply_field_changes(&mut span, changes)?;
    let mut out = String::new();
    for (index, line) in located.lines.iter().enumerate() {
        if index == start {
            for span_line in &span {
                out.push_str(span_line);
            }
        }
        if index < start || index >= end {
            out.push_str(line);
        }
    }
    Ok(out)
}

/// Apply each change to the parent span. Indexes are found again after each change.
///
/// # Errors
///
/// Returns a refusal when a key is assigned twice, or a clear names a missing key.
fn apply_field_changes(
    span: &mut Vec<String>,
    changes: &[FieldChange],
) -> Result<(), RefuseReason> {
    let mut additions = Vec::new();
    for change in changes {
        let hits = assignment_indexes(span, &change.key);
        match hits.as_slice() {
            [] => match &change.action {
                FieldAction::Clear => {
                    return Err(invalid_setting(&change.key, "is not set on this model"));
                }
                _ => additions.push(render_assignment(change)?),
            },
            [index] if matches!(change.action, FieldAction::Clear) => {
                let _removed = span.remove(*index);
            }
            [index] => replace_assignment(span, *index, change)?,
            _ => {
                return Err(invalid_setting(
                    &change.key,
                    "is assigned more than once in this model block",
                ));
            }
        }
    }
    for addition in additions {
        insert_before_trailing_blanks(span, addition);
    }
    Ok(())
}

/// Replace the assignment at `index` and keep that line's trailing newline.
///
/// # Errors
///
/// Returns a refusal when the replacement cannot be rendered.
fn replace_assignment(
    span: &mut [String],
    index: usize,
    change: &FieldChange,
) -> Result<(), RefuseReason> {
    let ends_with_newline = span.get(index).is_some_and(|line| line.ends_with('\n'));
    let mut rendered = render_assignment(change)?;
    if !ends_with_newline {
        let _newline = rendered.pop();
    }
    if let Some(line) = span.get_mut(index) {
        *line = rendered;
    }
    Ok(())
}

/// Render `key = value` with a trailing newline.
///
/// # Errors
///
/// Returns a refusal when the action is [`FieldAction::Clear`], which has no value to render.
fn render_assignment(change: &FieldChange) -> Result<String, RefuseReason> {
    let value = match &change.action {
        FieldAction::SetText(text) => toml_basic_string(text),
        FieldAction::SetInt(number) => number.to_string(),
        FieldAction::SetBool(true) => "true".to_owned(),
        FieldAction::SetBool(false) => "false".to_owned(),
        FieldAction::Clear => {
            return Err(invalid_setting(&change.key, "has no value to write"));
        }
    };
    Ok(format!("{} = {value}\n", change.key))
}

/// Indexes of bare assignments to `key` after the header line.
fn assignment_indexes(span: &[String], key: &str) -> Vec<usize> {
    let mut hits = Vec::new();
    for (index, line) in span.iter().enumerate() {
        if index > 0 && assigns_key(line, key) {
            hits.push(index);
        }
    }
    hits
}

/// Whether `line` assigns `key` and not a longer key that starts with those characters.
fn assigns_key(line: &str, key: &str) -> bool {
    let Some(rest) = line.trim().strip_prefix(key) else {
        return false;
    };
    rest.trim_start().starts_with('=')
}

/// Insert `addition` after the last content line and before trailing blank lines.
fn insert_before_trailing_blanks(span: &mut Vec<String>, addition: String) {
    let mut end = span.len();
    while end > 1 {
        let blank = span
            .get(end.saturating_sub(1))
            .is_some_and(|line| line.trim().is_empty());
        if !blank {
            break;
        }
        end = end.saturating_sub(1);
    }
    if let Some(previous) = span.get_mut(end.saturating_sub(1))
        && !previous.ends_with('\n')
    {
        previous.push('\n');
    }
    let line = if addition.ends_with('\n') {
        addition
    } else {
        format!("{addition}\n")
    };
    span.insert(end, line);
}

/// Parent span and child spans for one slug. The line slices borrow `text`.
struct LocatedModel<'a> {
    /// Keepends lines of the whole document.
    lines: Vec<&'a str>,
    /// The single `[model.<slug>]` span.
    parent: (usize, usize),
    /// `[model.<slug>.…]` spans, in file order.
    children: Vec<(usize, usize)>,
}

/// Find the parent header and any child headers for `slug`.
///
/// # Errors
///
/// Returns a refusal when the parent header is missing or repeated.
fn locate_model<'a>(text: &'a str, slug: &str) -> Result<LocatedModel<'a>, RefuseReason> {
    let lines = split_keepends(text);
    let spans = table_spans(&lines);
    let mut parents = Vec::new();
    let mut children = Vec::new();
    for span in &spans {
        if let Some(header_slug) = model_slug_from_header(&span.name)? {
            if header_slug == slug {
                parents.push((span.start, span.end));
            }
        } else if is_child_header(&span.name, slug) {
            children.push((span.start, span.end));
        }
    }
    let parent = match parents.as_slice() {
        [(start, end)] => (*start, *end),
        [] => {
            return Err(RefuseReason::NotConfigured {
                slug: slug.to_owned(),
            });
        }
        _ => {
            return Err(RefuseReason::DuplicateSlugHeader {
                slug: slug.to_owned(),
            });
        }
    };
    Ok(LocatedModel {
        lines,
        parent,
        children,
    })
}

/// Whether `header` is a child table of `slug` rather than another model slug.
fn is_child_header(header: &str, slug: &str) -> bool {
    let Some(rest) = header.strip_prefix("model.") else {
        return false;
    };
    let prefix = if is_bare_slug(slug) {
        format!("{slug}.")
    } else {
        format!("{}.", toml_basic_string(slug))
    };
    rest.starts_with(&prefix) && rest.len() > prefix.len()
}

/// Copy every line that does not fall inside `ranges`.
fn delete_ranges(lines: &[&str], ranges: &[(usize, usize)]) -> String {
    let mut kept = String::new();
    for (index, line) in lines.iter().enumerate() {
        let inside = ranges
            .iter()
            .any(|(start, end)| index >= *start && index < *end);
        if !inside {
            kept.push_str(line);
        }
    }
    kept
}

/// Join the named spans in file order and drop blank lines after the last content line.
fn ranges_text(lines: &[&str], ranges: &[(usize, usize)]) -> String {
    let mut ordered = ranges.to_vec();
    ordered.sort_by_key(|(start, _)| *start);
    let mut body = String::new();
    let mut previous_end = None;
    for (start, end) in ordered {
        let separated = previous_end.is_some_and(|previous| start != previous);
        if separated && !body.ends_with("\n\n") {
            if !body.ends_with('\n') {
                body.push('\n');
            }
            body.push('\n');
        }
        for index in start..end {
            if let Some(line) = lines.get(index) {
                body.push_str(line);
            }
        }
        previous_end = Some(end);
    }
    without_trailing_blank_lines(&body)
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

/// The `OpenRouter` `[router]` block `init` writes, including its trailing newline.
///
/// Writes `default_provider` and the openrouter record. Does not write `builtins`.
/// A missing `builtins` key still uses [`BUILTIN_SLUGS`]. This function does not read a file.
#[must_use = "the block is the only copy of the bytes init writes"]
pub fn openrouter_router_block() -> String {
    "\
[router]
default_provider = \"openrouter\"

[router.provider.openrouter]
base_url = \"https://openrouter.ai/api/v1\"
env_key = \"OPENROUTER_API_KEY\"
catalog = \"openrouter-models\"
"
    .to_owned()
}

/// Append the `OpenRouter` router block, or report that `[router]` is already a table.
///
/// A missing `[router]` key appends [`openrouter_router_block`]. Every earlier byte
/// stays. A non-empty file gets one blank line before the block, after a trailing
/// newline when the text did not already end in one. The document is not
/// reserialized. After the append, a top-level table that was already there
/// must still compare equal.
///
/// # Errors
///
/// Returns [`RouterInitError::NotTable`] when `router` is present and not a table,
/// [`RouterInitError::Parse`] when either text does not parse, and
/// [`RouterInitError::ChangedTables`] when an existing top-level table changes.
/// Nothing is written.
#[must_use = "the plan is the only copy of the bytes init would write"]
pub fn init_router_text(config: &str) -> Result<RouterInit, RouterInitError> {
    if config.is_empty() {
        return Ok(RouterInit::Append(openrouter_router_block()));
    }
    let before = match config.parse::<toml::Table>() {
        Ok(root) => root,
        Err(error) => {
            return Err(RouterInitError::Parse {
                detail: error.to_string(),
                appended: false,
            });
        }
    };
    match before.get("router") {
        Some(value) if value.as_table().is_some() => return Ok(RouterInit::AlreadyPresent),
        Some(_) => return Err(RouterInitError::NotTable),
        None => {}
    }
    let spliced = append_block(config, &openrouter_router_block());
    let after = match spliced.parse::<toml::Table>() {
        Ok(root) => root,
        Err(error) => {
            return Err(RouterInitError::Parse {
                detail: error.to_string(),
                appended: true,
            });
        }
    };
    let tables = changed_top_level_tables(&before, &after);
    if !tables.is_empty() {
        return Err(RouterInitError::ChangedTables { tables });
    }
    Ok(RouterInit::Append(spliced))
}

/// Top-level tables in `before` whose parsed value is missing or different in `after`.
fn changed_top_level_tables(before: &toml::Table, after: &toml::Table) -> Vec<String> {
    let mut changed = Vec::new();
    for (key, value) in before {
        if value.as_table().is_none() {
            continue;
        }
        match after.get(key) {
            Some(current) if current == value => {}
            _ => changed.push(key.clone()),
        }
    }
    changed.sort();
    changed
}

/// Refuse when `text` contains the value of a named environment variable.
///
/// Names are every `env_key` string in `text`, plus each name in `also`. An
/// unset name is skipped. An empty value is skipped. A non-Unicode value
/// refuses and names the variable. A non-empty value that occurs in `text`
/// refuses, names the variable, and does not include the value. No file is
/// touched.
///
/// # Errors
///
/// Returns an error when a scanned name is not Unicode or its value occurs in `text`.
pub fn refuse_named_secrets(text: &str, also: &[&str]) -> Result<(), SecretError> {
    let mut names = BTreeSet::new();
    if let Ok(root) = text.parse::<toml::Table>() {
        collect_env_keys(&toml::Value::Table(root), &mut names);
    }
    for name in also {
        if is_env_key(name) {
            let _inserted = names.insert((*name).to_owned());
        }
    }
    for name in names {
        match std::env::var(&name) {
            Err(std::env::VarError::NotUnicode(_)) => {
                return Err(SecretError::NotUnicode { name });
            }
            Ok(value) if !value.is_empty() && text.contains(&value) => {
                return Err(SecretError::ValueInConfig { name });
            }
            Err(std::env::VarError::NotPresent) | Ok(_) => {}
        }
    }
    Ok(())
}

/// Record every `env_key` string in `value`.
fn collect_env_keys(value: &toml::Value, names: &mut BTreeSet<String>) {
    match value {
        toml::Value::Table(table) => {
            for (key, child) in table {
                if key == "env_key"
                    && let Some(name) = child.as_str()
                    && is_env_key(name)
                {
                    let _inserted = names.insert(name.to_owned());
                }
                collect_env_keys(child, names);
            }
        }
        toml::Value::Array(items) => {
            for item in items {
                collect_env_keys(item, names);
            }
        }
        toml::Value::String(_)
        | toml::Value::Integer(_)
        | toml::Value::Float(_)
        | toml::Value::Boolean(_)
        | toml::Value::Datetime(_) => {}
    }
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
    if let Some(parent) = backup_path.parent()
        && !parent.as_os_str().is_empty()
        && let Err(error) = std::fs::create_dir_all(parent)
    {
        return Err(ReplaceError {
            kind: ReplaceKind::Backup,
            detail: io_detail("create backup directory", parent, &error),
        });
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
    write_temp_and_rename(path, bytes)
}

/// Create `path` and any missing parent. Does not write a backup.
///
/// The bytes land via a temp file in the same directory, then a rename, the same
/// way [`replace_file`] finishes a write. A file that already exists is left
/// untouched. An empty `secret` is not searched. A non-empty secret that occurs
/// in `bytes` refuses the create before any write.
///
/// # Errors
///
/// Returns an error when the bytes contain `secret`, `path` already exists, or a
/// filesystem step fails. A failed create does not leave `path` behind.
pub fn create_file(path: &Path, bytes: &[u8], secret: &str) -> Result<(), ReplaceError> {
    if contains_secret(bytes, secret) {
        return Err(ReplaceError {
            kind: ReplaceKind::SecretInReplacement,
            detail: "replacement contains the caller-supplied secret".to_owned(),
        });
    }
    if path.exists() {
        return Err(ReplaceError {
            kind: ReplaceKind::Read,
            detail: format!("{} already exists, so it was not created", path.display()),
        });
    }
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
        && let Err(error) = std::fs::create_dir_all(parent)
    {
        return Err(ReplaceError {
            kind: ReplaceKind::Temp,
            detail: io_detail("create config directory", parent, &error),
        });
    }
    write_temp_and_rename(path, bytes)
}

/// Write `bytes` to a temp file beside `path`, then rename it onto `path`.
///
/// # Errors
///
/// Returns an error when the temp file cannot be written or renamed. A failed
/// rename deletes the temp file.
fn write_temp_and_rename(path: &Path, bytes: &[u8]) -> Result<(), ReplaceError> {
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
#[allow(
    clippy::ref_option,
    reason = "the thiserror format argument is already &Option<String>"
)]
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

/// Resolve built-ins from `[router]`, or refuse with the original text before any splice.
///
/// # Errors
///
/// Returns [`Refused`] when [`builtins_from`] cannot read the list.
fn require_builtins(config: &str) -> Result<Vec<String>, Refused> {
    match builtins_from(config) {
        Ok(builtins) => Ok(builtins),
        Err(reason) => Err(refuse(config, reason)),
    }
}

/// Require the slug shape shared by headers, defaults, and `--as`.
///
/// The slug must start with an ASCII letter or digit. The rest may contain
/// only ASCII letters, digits, dots, underscores, and hyphens.
///
/// # Errors
///
/// Returns [`RefuseReason::InvalidSlug`] when `slug` does not match that grammar.
pub fn require_slug(slug: &str) -> Result<(), RefuseReason> {
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

/// The four compiled slugs, in [`BUILTIN_SLUGS`] order.
fn compiled_builtins() -> Vec<String> {
    BUILTIN_SLUGS.iter().copied().map(str::to_owned).collect()
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
///
/// # Errors
///
/// Returns a refusal when the value is not an array of non-empty strings.
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

/// Parse `config` into a table. The caller already ran [`structural_problem`].
///
/// # Errors
///
/// Returns [`RefuseReason::Parse`] when `config` is not a TOML table.
fn parsed_root(config: &str) -> Result<toml::Table, RefuseReason> {
    match config.parse::<toml::Table>() {
        Ok(root) => Ok(root),
        Err(error) => Err(RefuseReason::Parse {
            detail: error.to_string(),
        }),
    }
}

/// Read the explicit provider name. This does not look at `default_provider`.
///
/// # Errors
///
/// Returns [`RefuseReason::InvalidSetting`] when the name is empty, `[router]`
/// is not a table, or that name has no record.
fn explicit_record(root: &toml::Table, name: &str) -> Result<ProviderRecord, RefuseReason> {
    if name.is_empty() {
        return Err(invalid_setting(
            "provider",
            "name is missing; an empty --provider is not the default",
        ));
    }
    let Some(router) = router_table(root)? else {
        return Err(missing_router_provider(name));
    };
    let Some(table) = provider_table(router, name) else {
        return Err(missing_router_provider(name));
    };
    record_from_table(table)
}

/// Read `default_provider`, then that record. Another record is not a fallback.
///
/// # Errors
///
/// Returns [`RefuseReason::InvalidSetting`] when `[router]` is missing or not
/// a table, `default_provider` is missing, blank, or not a string, or the
/// named record is missing.
fn default_record(root: &toml::Table) -> Result<ProviderRecord, RefuseReason> {
    let Some(router) = router_table(root)? else {
        return Err(invalid_setting(
            "router",
            "[router] is missing; write that table",
        ));
    };
    let name = default_provider_name(router.get("default_provider"))?;
    let Some(table) = provider_table(router, name) else {
        return Err(missing_router_provider(name));
    };
    record_from_table(table)
}

/// `[router]` when it is a table. Absence is [`None`]. A non-table is an error.
///
/// # Errors
///
/// Returns [`RefuseReason::InvalidSetting`] when `router` is present and not a table.
fn router_table(root: &toml::Table) -> Result<Option<&toml::Table>, RefuseReason> {
    match root.get("router") {
        None => Ok(None),
        Some(value) => match value.as_table() {
            Some(table) => Ok(Some(table)),
            None => Err(invalid_setting("router", "[router] must be a table")),
        },
    }
}

/// The table at `provider.<name>` inside an already parsed `[router]`.
fn provider_table<'a>(router: &'a toml::Table, name: &str) -> Option<&'a toml::Table> {
    router
        .get("provider")
        .and_then(toml::Value::as_table)
        .and_then(|providers| providers.get(name))
        .and_then(toml::Value::as_table)
}

/// Refuse `name` because no `[router.provider]` table uses that exact key.
fn missing_router_provider(name: &str) -> RefuseReason {
    invalid_setting(
        name,
        "no [router.provider] table has this name; write that record",
    )
}

/// The `default_provider` string. Blank and non-strings refuse. The text is not trimmed.
///
/// # Errors
///
/// Returns [`RefuseReason::InvalidSetting`] when the key is missing, blank, or not a string.
fn default_provider_name(value: Option<&toml::Value>) -> Result<&str, RefuseReason> {
    let Some(value) = value else {
        return Err(invalid_setting(
            "default_provider",
            "must be a non-empty string when --provider is omitted",
        ));
    };
    let Some(text) = value.as_str() else {
        return Err(invalid_setting(
            "default_provider",
            "must be a non-empty string",
        ));
    };
    if text.is_empty() {
        return Err(invalid_setting(
            "default_provider",
            "must be a non-empty string with no trim, got \"\"",
        ));
    }
    Ok(text)
}

/// Copy `base_url`, `env_key`, and `catalog` from one provider table.
///
/// # Errors
///
/// Returns [`RefuseReason::InvalidSetting`] when `base_url`, `env_key`, or `catalog` cannot be used.
fn record_from_table(table: &toml::Table) -> Result<ProviderRecord, RefuseReason> {
    let base_url = provider_base(table.get("base_url"))?;
    let env_key = provider_env_key(table.get("env_key"))?;
    let catalog = record_catalog(table.get("catalog"))?;
    Ok(ProviderRecord {
        base_url,
        env_key,
        catalog,
    })
}

/// Return `catalog` as written. Blank is absent. This does not match a cartridge id.
///
/// # Errors
///
/// Returns [`RefuseReason::InvalidSetting`] when `catalog` is present and not a string.
fn record_catalog(value: Option<&toml::Value>) -> Result<Option<String>, RefuseReason> {
    let Some(value) = value else {
        return Ok(None);
    };
    let Some(text) = value.as_str() else {
        return Err(invalid_setting("catalog", "must be a string"));
    };
    if text.is_empty() {
        Ok(None)
    } else {
        Ok(Some(text.to_owned()))
    }
}

/// Use the catalog name when it has a non-blank trimmed form, otherwise the provider id.
fn display_name(model: CatalogModel<'_>) -> &str {
    match model.name.map(str::trim) {
        Some(name) if !name.is_empty() => name,
        _ => model.id,
    }
}

/// Header for `slug`. Dotted and other non-bare slugs are quoted so they stay one key.
///
/// # Errors
///
/// Returns a refusal when `slug` is not a legal model header.
fn model_header(slug: &str) -> Result<String, RefuseReason> {
    require_slug(slug)?;
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
        let hex = b"0123456789abcdef"
            .get(usize::from(nibble))
            .copied()
            .unwrap_or(b'0');
        encoded.push(char::from(hex));
    }
}

/// Decode a TOML basic string used as a model header key.
///
/// # Errors
///
/// Returns a refusal when `quoted` is not a TOML basic string.
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
///
/// A quoted key with another segment after it, such as
/// `model."or.vision".extra_headers`, is a child table and returns [`None`].
///
/// # Errors
///
/// Returns a refusal when a `model.` header key starts with `"` and the basic string
/// never closes, or the closed string is not a legal slug.
fn model_slug_from_header(name: &str) -> Result<Option<String>, RefuseReason> {
    let Some(rest) = name.strip_prefix("model.") else {
        return Ok(None);
    };
    if let Some(quoted) = leading_basic_string(rest) {
        if rest.len() != quoted.len() {
            return Ok(None);
        }
        let slug = toml_unquote(quoted)?;
        require_slug(&slug)?;
        return Ok(Some(slug));
    }
    if rest.starts_with('"') {
        return Err(RefuseReason::Parse {
            detail: format!("expected a quoted TOML string, got {rest:?}"),
        });
    }
    if is_bare_slug(rest) {
        return Ok(Some(rest.to_owned()));
    }
    Ok(None)
}

/// The leading TOML basic string in `text`, including its quotes, when the quote closes.
///
/// Escaped quotes do not end the string. An unclosed quote returns [`None`].
fn leading_basic_string(text: &str) -> Option<&str> {
    if !text.starts_with('"') {
        return None;
    }
    let mut escaped = false;
    for (index, ch) in text.char_indices().skip(1) {
        if escaped {
            escaped = false;
            continue;
        }
        match ch {
            '\\' => escaped = true,
            '"' => return text.get(..=index),
            _ => {}
        }
    }
    None
}

/// Reject duplicate `[models]` headers and duplicate model slugs before editing.
///
/// # Errors
///
/// Returns a refusal when `[models]` or one model slug is declared twice.
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
///
/// # Errors
///
/// Returns [`Refused`] when the text has a duplicate header or is not a TOML table.
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
///
/// `edited_provider` is the one existing slug whose `model` id may change, and the id it must have.
///
/// # Errors
///
/// Returns [`Refused`] when the spliced text does not re-parse or breaks an edit invariant.
#[allow(
    clippy::too_many_arguments,
    reason = "the splice contract names each preserved fact separately"
)]
fn finish(
    original: &str,
    before: &toml::Table,
    spliced: String,
    added: Option<&str>,
    removed: Option<&str>,
    default: DefaultExpectation<'_>,
    expected_provider: Option<&str>,
    edited_provider: Option<(&str, &str)>,
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
        edited_provider,
    ) {
        return Err(refuse(original, reason));
    }
    Ok(spliced)
}

/// Compare parsed documents after a splice.
///
/// # Errors
///
/// Returns a refusal when the splice dropped a table, changed a provider id, or missed the edit.
#[allow(
    clippy::too_many_arguments,
    reason = "the splice contract names each preserved fact separately"
)]
fn preserve(
    before: &toml::Table,
    after: &toml::Table,
    added: Option<&str>,
    removed: Option<&str>,
    default: DefaultExpectation<'_>,
    expected_provider: Option<&str>,
    edited_provider: Option<(&str, &str)>,
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
            Some(current)
                if edited_provider
                    .is_some_and(|(edited, next)| edited == slug.as_str() && current == next) => {}
            _ => {
                return Err(RefuseReason::ChangedProvider { slug: slug.clone() });
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
    if let Some(slug) = removed
        && after_models.contains_key(slug)
    {
        return Err(RefuseReason::RemoveFailed {
            slug: slug.to_owned(),
        });
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
    if start < text.len()
        && let Some(line) = text.get(start..)
    {
        lines.push(line);
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
            .map_or(lines.len(), |(next, _)| *next);
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
///
/// # Errors
///
/// Returns a refusal when `[models]` cannot be updated for `slug`.
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
        let is_default = lines
            .get(index)
            .is_some_and(|line| is_default_assignment(line));
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
    let default_line = format!("default = {}\n", toml_basic_string(slug));
    body.push_str(&default_line);
    body
}

/// Delete the header span for `slug` and every child table under that slug.
///
/// The parent span runs from its header through the line before the next header.
/// A child header is removed the same way. Bytes outside those spans are copied
/// unchanged.
///
/// # Errors
///
/// Returns a refusal when `slug` has no single model header to delete.
fn delete_span(text: &str, slug: &str) -> Result<String, RefuseReason> {
    let located = locate_model(text, slug)?;
    let mut ranges = vec![located.parent];
    ranges.extend(located.children);
    Ok(delete_ranges(&located.lines, &ranges))
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
    let mut name = path.file_name().map_or_else(
        || std::ffi::OsString::from("config.toml"),
        std::ffi::OsStr::to_os_string,
    );
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
    use crate::catalog::CatalogCartridge;

    const FIXTURE_BASE: &str = "https://openrouter.ai/api/v1";

    /// Splice with the `OpenRouter` base and env name. The slug is already chosen.
    fn open_insert(config: &str, model: CatalogModel<'_>, slug: &str) -> Result<String, Refused> {
        insert_model(config, model, slug, FIXTURE_BASE, "OPENROUTER_API_KEY")
    }

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
            FIXTURE_BASE,
            "OPENROUTER_API_KEY",
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
        assert!(
            !block.contains("api_key"),
            "a new block must not store api_key"
        );
        assert!(
            !block.contains("max_completion_tokens"),
            "a new block must not copy max_completion_tokens"
        );
        assert!(
            !block.contains("reasoning_efforts"),
            "a new block must not set reasoning_efforts"
        );
        assert!(
            !block.contains("vision"),
            "text-only input must omit vision"
        );

        let updated = open_insert(FIXTURE, widget(Some(1_048_576), false), "or-widget-v2")
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
        let updated = open_insert(
            FIXTURE,
            CatalogModel {
                id: "z-ai/glm-5.3",
                name: Some("replacement name"),
                context_window: Some(1_048_576),
                sees_image: true,
            },
            "or-glm-5-3",
        )
        .expect("the same slug and provider id should be a no-op");

        assert_eq!(
            updated, FIXTURE,
            "a catalog context_length of 1048576 must not rewrite the 600000 block"
        );
    }

    #[test]
    fn different_slug_for_the_same_provider_returns_the_original_text() {
        let refused = open_insert(
            FIXTURE,
            CatalogModel {
                id: "deepseek/deepseek-v4.1-flash-1017",
                name: Some("DeepSeek Flash"),
                context_window: Some(1_048_576),
                sees_image: false,
            },
            "or-deepseek-v4-1-flash-1017",
        )
        .expect_err("a derived slug that differs from the configured slug should be refused");

        assert_eq!(
            refused.text, FIXTURE,
            "a duplicate provider id should return the original text"
        );
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
            crate::catalog::cartridge(Some("openrouter-models"))
                .expect("openrouter-models should select a cartridge")
                .slug("deepseek/deepseek-v4.1-flash-1017", None)
                .expect("this provider id should derive a slug on the OpenRouter cartridge"),
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
        let updated = open_insert(FIXTURE, quoted, "or.vision")
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

        let parsed: toml::Table = updated.parse().expect("the spliced config should parse");
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
        assert_eq!(
            entry.get("vision").and_then(toml::Value::as_bool),
            Some(true)
        );
        assert_eq!(
            entry.get("name").and_then(toml::Value::as_str),
            Some("Z.ai: GLM \"5.3\"")
        );
    }

    #[test]
    fn name_falls_back_and_context_must_be_positive() {
        let nameless = open_insert(
            FIXTURE,
            CatalogModel {
                id: "acme/nameless",
                name: None,
                context_window: Some(32),
                sees_image: false,
            },
            "or-nameless",
        )
        .expect("a missing catalog name should still insert");
        assert!(
            nameless.contains("name = \"acme/nameless\"\n"),
            "a missing name should fall back to the provider id"
        );

        let blank = open_insert(
            &nameless,
            CatalogModel {
                id: "acme/blank",
                name: Some("   "),
                context_window: Some(32),
                sees_image: false,
            },
            "or-blank",
        )
        .expect("a blank catalog name should still insert");
        assert!(
            blank.contains("name = \"acme/blank\"\n"),
            "a blank name should fall back to the provider id"
        );

        for context in [None, Some(0), Some(-1)] {
            let refused = open_insert(FIXTURE, widget(context, false), "or-widget-v2")
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
    fn explicit_slug_inserts_when_the_provider_id_has_an_empty_leaf() {
        let updated = open_insert(
            FIXTURE,
            CatalogModel {
                id: "@@@",
                name: Some("Custom"),
                context_window: Some(8),
                sees_image: false,
            },
            "or-custom",
        )
        .expect("an explicit slug should bypass an empty leaf");
        assert!(updated.contains("[model.or-custom]\n"));
    }

    #[test]
    fn slug_already_used_by_a_model_or_a_builtin_is_an_error() {
        for slug in ["or-glm-5-3", "grok-4.5"] {
            let refused = open_insert(FIXTURE, widget(Some(8), false), slug)
                .expect_err("a taken slug should be refused");
            assert_eq!(
                refused.text, FIXTURE,
                "{slug} should leave the config unchanged"
            );
            assert!(
                matches!(refused.reason, RefuseReason::SlugInUse { .. }),
                "{slug} should be in use, got {}",
                refused.reason
            );
        }
        let refused = open_insert(FIXTURE, widget(Some(8), false), "-nope")
            .expect_err("a slug must start with a letter or digit");
        assert_eq!(refused.text, FIXTURE);
        assert!(matches!(refused.reason, RefuseReason::InvalidSlug { .. }));
    }

    #[test]
    fn insert_puts_one_blank_line_around_the_block() {
        let text = "# c\nkey = 1\n[models]\ndefault = \"grok-4.7\"\n";
        let updated = open_insert(text, widget(Some(1_048_576), false), "or-widget-v2")
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
            FIXTURE_BASE,
            "OPENROUTER_API_KEY",
        )
        .expect("the widget block should render");
        let appended = open_insert(head, widget(Some(8), false), "or-widget-v2")
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
            let (updated_prefix, rest) = updated
                .split_once("[models]\n")
                .expect("set-default should keep the existing [models] header");
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

        let refused =
            set_default(FIXTURE, "not-a-model").expect_err("an unknown slug should be refused");
        assert_eq!(refused.text, FIXTURE);
        assert!(matches!(refused.reason, RefuseReason::UnknownSlug { .. }));
    }

    #[test]
    fn remove_deletes_only_that_table_span() {
        let inserted = open_insert(FIXTURE, widget(Some(8), false), "or-widget-v2")
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
        let default_refused =
            remove_model(FIXTURE, "grok-4.7").expect_err("the saved default should be refused");
        assert_eq!(default_refused.text, FIXTURE);
        assert!(matches!(
            default_refused.reason,
            RefuseReason::IsDefault { .. }
        ));

        let builtin = remove_model(FIXTURE, "grok-4.6").expect_err("a built-in should be refused");
        assert_eq!(builtin.text, FIXTURE);
        assert!(matches!(builtin.reason, RefuseReason::BuiltIn { .. }));

        let missing =
            remove_model(FIXTURE, "or-missing").expect_err("a missing slug should be refused");
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
        assert!(matches!(models.reason, RefuseReason::MultipleModelsTables));

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
        let parsed = open_insert(malformed, widget(Some(8), false), "or-widget-v2")
            .expect_err("malformed toml should be refused");
        assert_eq!(parsed.text, malformed);
        assert!(matches!(parsed.reason, RefuseReason::Parse { .. }));
    }

    #[test]
    fn insert_writes_the_caller_base_and_env_and_does_not_read_router_settings() {
        let base = "https://caller.example/v1";
        let env_key = "CALLER_API_KEY";
        let updated = insert_model(
            FIXTURE,
            widget(Some(8), false),
            "or-widget-v2",
            base,
            env_key,
        )
        .expect("a document with no settings table should still insert the caller record");
        let block = "\
[model.or-widget-v2]
model = \"acme/widget.v2\"
base_url = \"https://caller.example/v1\"
name = \"Widget\"
env_key = \"CALLER_API_KEY\"
api_backend = \"chat_completions\"
context_window = 8
";
        assert!(
            updated.contains(block),
            "the new block should use the caller base and env name, not a compiled fallback, got:\n{updated}"
        );
        assert!(
            !updated.contains("[grok_router]"),
            "insert must not write [grok_router], got:\n{updated}"
        );
        assert!(
            !updated.contains("\n[router]\n"),
            "insert must not write [router], got:\n{updated}"
        );
        assert!(
            !updated.contains("model_provider"),
            "the splice must not write model_provider"
        );
        assert!(
            !updated.contains("api_key"),
            "the splice must not write api_key"
        );
        assert_fixture_blocks(&updated);
    }

    #[test]
    fn caller_base_is_written_on_the_new_block_only() {
        let base = "https://caller.example/v1/";
        let config = with_settings("openrouter_base_url = \"https://decoy.example/v1\"");
        let updated = insert_model(
            &config,
            widget(Some(8), false),
            "or-widget-v2",
            base,
            "OPENROUTER_API_KEY",
        )
        .expect("the caller base should still insert one block");
        let block = "\
[model.or-widget-v2]
model = \"acme/widget.v2\"
base_url = \"https://caller.example/v1/\"
name = \"Widget\"
env_key = \"OPENROUTER_API_KEY\"
api_backend = \"chat_completions\"
context_window = 8
";
        assert!(
            updated.contains(block),
            "the new block should keep the trailing slash from the caller, got:\n{updated}"
        );
        assert_eq!(
            updated
                .matches("\nbase_url = \"https://caller.example/v1/\"\n")
                .count(),
            1,
            "the caller base should appear on the new block only, got:\n{updated}"
        );
        assert!(
            !updated.contains("\nbase_url = \"https://decoy.example/v1\"\n"),
            "insert must not copy openrouter_base_url onto a model span, got:\n{updated}"
        );
        assert_eq!(
            updated.matches("[grok_router]").count(),
            1,
            "the edit must not insert another [grok_router]"
        );
        assert!(
            updated.contains("openrouter_base_url = \"https://decoy.example/v1\""),
            "the settings line should stay byte-identical"
        );
        assert_fixture_blocks(&updated);
        assert!(
            !updated.contains("model_provider"),
            "the splice must not write model_provider"
        );
        assert!(
            !updated.contains("api_key"),
            "the splice must not write api_key"
        );
    }

    #[test]
    fn duplicate_detection_uses_the_passed_base() {
        let base = "https://example.invalid/v1";
        let settings = "[grok_router]\nopenrouter_base_url = \"https://openrouter.ai/api/v1\"\n\n";
        let same = format!("{settings}[model.or-glm-5-3]\nmodel = \"z-ai/glm-5.3\"\n");
        let same = format!("{same}base_url = \"{base}\"\nname = \"GLM 5.3\"\n");
        let model = CatalogModel {
            id: "z-ai/glm-5.3",
            name: Some("replacement name"),
            context_window: Some(9),
            sees_image: true,
        };
        let unchanged = insert_model(&same, model, "or-glm-5-3", base, "OPENROUTER_API_KEY")
            .expect("the same provider id and passed base should be a no-op");
        assert_eq!(
            unchanged, same,
            "the same provider id at the passed base should return the original text"
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
            "or-glm-elsewhere",
            base,
            "OPENROUTER_API_KEY",
        )
        .expect("the same provider id at a different base should not be a duplicate");
        assert!(
            updated.contains("[model.or-glm-elsewhere]\n"),
            "a different base should insert a second block, got:\n{updated}"
        );
        assert!(
            updated.contains(&format!("base_url = \"{base}\"\n")),
            "the new block should use the passed base, not openrouter_base_url, got:\n{updated}"
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
            assert_eq!(
                removed.text, config,
                "{slug} should leave the text unchanged"
            );
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
[router]
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

        let builtin =
            remove_model(config, "grok-4.8").expect_err("grok-4.8 should be refused as a built-in");
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

        let in_use = open_insert(config, widget(Some(8), false), "grok-4.8")
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
[router]
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
            assert_eq!(
                defaulted.text, config,
                "{slug} should leave the text unchanged"
            );
            assert!(
                matches!(defaulted.reason, RefuseReason::UnknownSlug { .. }),
                "{slug} should be unknown, got {}",
                defaulted.reason
            );
            let removed = remove_model(config, slug)
                .expect_err("an empty builtins array should not treat the slug as built-in");
            assert_eq!(
                removed.text, config,
                "{slug} should leave the text unchanged"
            );
            assert!(
                matches!(removed.reason, RefuseReason::NotConfigured { .. }),
                "{slug} should be missing, not built-in, got {}",
                removed.reason
            );
        }

        let updated =
            set_default(config, "or-glm-5-3").expect("a configured slug should still be accepted");
        assert_eq!(
            updated.matches("[model.").count(),
            1,
            "set-default must not add a model table, got:\n{updated}"
        );
        assert!(updated.contains("default = \"or-glm-5-3\"\n"));

        let inserted = open_insert(config, widget(Some(8), false), "grok-4.7")
            .expect("an empty builtins array should not reserve a compiled slug");
        assert!(
            inserted.contains("[model.\"grok-4.7\"]\n"),
            "insert should accept a compiled slug when [router] builtins is empty, got:\n{inserted}"
        );
    }

    #[test]
    fn invalid_grok_router_settings_do_not_block_insert() {
        let cases = [
            "openrouter_base_url = \"\"",
            "openrouter_base_url = \"ftp://nope\"",
            "openrouter_base_url = \"https://ex ample.invalid/v1\"",
            "openrouter_base_url = \"https://example.invalid/v1 \"",
            "openrouter_base_url = \"http://\"",
            "builtins = \"grok-4.8\"",
            "builtins = [\"\"]",
            "builtins = [1]",
            "builtins = [\"grok-4.8\", \"\"]",
        ];
        for body in cases {
            let config = with_settings(body);
            let inserted = insert_model(
                &config,
                widget(Some(8), false),
                "or-widget-v2",
                FIXTURE_BASE,
                "OPENROUTER_API_KEY",
            )
            .expect("insert should ignore an invalid [grok_router] table");
            assert!(
                inserted.contains(body),
                "insert should leave the invalid [grok_router] line in place for {body}, got:\n{inserted}"
            );
            assert!(
                inserted.contains("[model.or-widget-v2]\n"),
                "insert should still splice the caller slug for {body}, got:\n{inserted}"
            );
            assert_fixture_blocks(&inserted);
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
        let inserted = insert_model(
            &config,
            widget(Some(8), false),
            "or-widget-v2",
            FIXTURE_BASE,
            "OPENROUTER_API_KEY",
        )
        .expect("an invalid [grok_router] table should not refuse insert");
        assert!(
            inserted.contains("openrouter_base_url = \"\""),
            "insert should leave the invalid settings line in place, got:\n{inserted}"
        );
        assert!(
            inserted.contains("[model.or-widget-v2]\n"),
            "insert should splice the caller slug, got:\n{inserted}"
        );
    }

    /// Prefix `body` as a `[grok_router]` table in front of [`FIXTURE`].
    fn with_settings(body: &str) -> String {
        format!("[grok_router]\n{body}\n\n{FIXTURE}")
    }

    /// Refuse `config` from insert, set-default, and remove, and name `key`.
    fn assert_invalid_setting(config: &str, key: &str) {
        let inserted = open_insert(config, widget(Some(8), false), "or-widget-v2")
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
            RefuseReason::InvalidSetting {
                key: actual,
                detail,
            } => {
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
    fn explicit_choice_ignores_default_provider() {
        let record_body = "\
[router.provider.deepseek]
base_url = \"https://api.deepseek.com\"
env_key = \"DEEPSEEK_API_KEY\"
catalog = \"deepseek-list-models\"
";
        let configs = [
            format!("[router]\n\n{record_body}"),
            format!("[router]\ndefault_provider = \"\"\n\n{record_body}"),
            format!("[router]\ndefault_provider = 1\n\n{record_body}"),
            format!("[router]\ndefault_provider = \"openrouter\"\n\n{record_body}"),
        ];
        for config in &configs {
            let record = resolve_provider(config, ProviderChoice::Explicit("deepseek")).expect(
                "an explicit deepseek record should resolve without reading default_provider",
            );
            assert_eq!(record.base_url, "https://api.deepseek.com");
            assert_eq!(record.env_key, "DEEPSEEK_API_KEY");
            assert_eq!(
                record.catalog.as_deref(),
                Some("deepseek-list-models"),
                "the catalog string should be returned as written"
            );
        }
    }

    #[test]
    fn default_choice_refuses_without_another_record_or_the_compiled_base() {
        let deepseek = "\
[router.provider.deepseek]
base_url = \"https://api.deepseek.com\"
env_key = \"DEEPSEEK_API_KEY\"
";
        let cases = [
            (String::new(), "router"),
            ("[router]\n".to_owned(), "default_provider"),
            (
                format!("[router]\ndefault_provider = \"\"\n\n{deepseek}"),
                "default_provider",
            ),
            (
                format!("[router]\ndefault_provider = 1\n\n{deepseek}"),
                "default_provider",
            ),
            (
                format!("[router]\ndefault_provider = \"missing\"\n\n{deepseek}"),
                "missing",
            ),
            (
                format!("[router]\ndefault_provider = \" deepseek\"\n\n{deepseek}"),
                " deepseek",
            ),
        ];
        for (config, key) in cases {
            let error = match resolve_provider(&config, ProviderChoice::Default) {
                Ok(record) => {
                    panic!(
                        "default resolution should refuse {key}, got base {}",
                        record.base_url
                    )
                }
                Err(error) => error,
            };
            assert_setting(&error, key);
            let rendered = error.to_string();
            assert!(
                !rendered.contains(FIXTURE_BASE),
                "a default miss must not name the compiled base, got {rendered}"
            );
            assert!(
                !rendered.contains("https://api.deepseek.com"),
                "a default miss must not use the other provider record, got {rendered}"
            );
        }
    }

    #[test]
    fn empty_explicit_name_is_not_the_default() {
        let config = "\
[router]
default_provider = \"deepseek\"

[router.provider.deepseek]
base_url = \"https://api.deepseek.com\"
env_key = \"DEEPSEEK_API_KEY\"
";
        let error = resolve_provider(config, ProviderChoice::Explicit(""))
            .expect_err("an empty explicit name should be refused");
        assert_setting(&error, "provider");
        let rendered = error.to_string();
        assert!(
            !rendered.contains("api.deepseek.com"),
            "an empty name must not resolve the default record, got {rendered}"
        );
        let chosen = resolve_provider(config, ProviderChoice::Default)
            .expect("the same file should still resolve the default record");
        assert_eq!(chosen.base_url, "https://api.deepseek.com");
    }

    #[test]
    fn grok_router_table_is_not_a_router_record() {
        let tables = [
            "\
[grok_router.provider.deepseek]
base_url = \"https://api.deepseek.com\"
env_key = \"DEEPSEEK_API_KEY\"
",
            "\
[grok_router]
openrouter_base_url = \"https://openrouter.ai/api/v1\"

[grok_router.provider.deepseek]
base_url = \"https://api.deepseek.com\"
env_key = \"DEEPSEEK_API_KEY\"
",
        ];
        for config in tables {
            let explicit = resolve_provider(config, ProviderChoice::Explicit("deepseek"))
                .expect_err("an explicit name should miss when only [grok_router] exists");
            assert_setting(&explicit, "deepseek");
            let defaulted = resolve_provider(config, ProviderChoice::Default)
                .expect_err("the default choice should miss when [router] is absent");
            assert_setting(&defaulted, "router");
            for error in [&explicit, &defaulted] {
                let rendered = error.to_string();
                assert!(
                    !rendered.contains(FIXTURE_BASE),
                    "a miss must not name the compiled base, got {rendered}"
                );
                assert!(
                    !rendered.contains("https://api.deepseek.com"),
                    "a miss must not return the old provider base, got {rendered}"
                );
            }
        }
    }

    #[test]
    fn record_fields_keep_an_untrimmed_base_and_env_key() {
        let slash = "\
[router.provider.deepseek]
base_url = \"https://api.deepseek.com/\"
env_key = \"DEEPSEEK_API_KEY\"
";
        let record = resolve_provider(slash, ProviderChoice::Explicit("deepseek"))
            .expect("a trailing slash should stay part of the base");
        assert_eq!(record.base_url, "https://api.deepseek.com/");
        assert_eq!(record.env_key, "DEEPSEEK_API_KEY");

        let spaced = "\
[router.provider.deepseek]
base_url = \"https://api.deepseek.com \"
env_key = \"DEEPSEEK_API_KEY\"
";
        let spaced_error = resolve_provider(spaced, ProviderChoice::Explicit("deepseek"))
            .expect_err("a trailing space in base_url should not be trimmed");
        assert_setting(&spaced_error, "base_url");

        let spaced_key = "\
[router.provider.deepseek]
base_url = \"https://api.deepseek.com\"
env_key = \"DEEPSEEK_API_KEY \"
";
        let key_error = resolve_provider(spaced_key, ProviderChoice::Explicit("deepseek"))
            .expect_err("a trailing space in env_key should not be trimmed");
        assert_setting(&key_error, "env_key");
    }

    #[test]
    fn catalog_string_is_returned_as_written() {
        let other_base = "\
[router.provider.zai]
base_url = \"https://api.z.ai/api/paas/v4\"
env_key = \"ZAI_API_KEY\"
catalog = \"deepseek-list-models\"
";
        let other = resolve_provider(other_base, ProviderChoice::Explicit("zai"))
            .expect("deepseek-list-models on another base should be returned, not checked");
        assert_eq!(other.catalog.as_deref(), Some("deepseek-list-models"));

        let unknown = "\
[router.provider.zai]
base_url = \"https://api.z.ai/api/paas/v4\"
env_key = \"ZAI_API_KEY\"
catalog = \"not-a-cartridge\"
";
        let unknown_record = resolve_provider(unknown, ProviderChoice::Explicit("zai"))
            .expect("an unknown catalog id should be returned without selecting a cartridge");
        assert_eq!(unknown_record.catalog.as_deref(), Some("not-a-cartridge"));

        let blank = "\
[router.provider.zai]
base_url = \"https://api.z.ai/api/paas/v4\"
env_key = \"ZAI_API_KEY\"
catalog = \"\"
";
        let blank_record = resolve_provider(blank, ProviderChoice::Explicit("zai"))
            .expect("a blank catalog should be absent, not an error");
        assert_eq!(blank_record.catalog, None);

        let absent = "\
[router.provider.zai]
base_url = \"https://api.z.ai/api/paas/v4\"
env_key = \"ZAI_API_KEY\"
";
        let absent_record = resolve_provider(absent, ProviderChoice::Explicit("zai"))
            .expect("a missing catalog key should be absent");
        assert_eq!(absent_record.catalog, None);
    }

    #[test]
    fn non_string_record_fields_are_invalid_settings() {
        let cases = [
            ("base_url = 1\nenv_key = \"DEEPSEEK_API_KEY\"\n", "base_url"),
            (
                "base_url = \"https://api.deepseek.com\"\nenv_key = 1\n",
                "env_key",
            ),
            (
                "\
base_url = \"https://api.deepseek.com\"
env_key = \"DEEPSEEK_API_KEY\"
catalog = 1
",
                "catalog",
            ),
        ];
        for (body, key) in cases {
            let config = format!("[router.provider.deepseek]\n{body}");
            let error = match resolve_provider(&config, ProviderChoice::Explicit("deepseek")) {
                Ok(record) => panic!(
                    "a non-string {key} should be refused, got {}",
                    record.base_url
                ),
                Err(error) => error,
            };
            assert_setting(&error, key);
        }
    }

    #[test]
    fn grok_router_builtins_do_not_decide_membership() {
        let config = "\
[grok_router]
builtins = [\"grok-4.8\"]

[models]
default = \"or-glm-5-3\"
";
        let listed = set_default(config, "grok-4.8")
            .expect_err("[grok_router] builtins should not make grok-4.8 a built-in");
        assert_eq!(listed.text, config);
        assert!(
            matches!(listed.reason, RefuseReason::UnknownSlug { .. }),
            "grok-4.8 should be unknown, got {}",
            listed.reason
        );
        let compiled = set_default(config, "grok-4.7")
            .expect("an absent [router] table should keep the compiled built-ins");
        assert!(compiled.contains("default = \"grok-4.7\"\n"));
    }

    #[test]
    fn builtins_on_a_provider_record_are_not_membership() {
        let config = "\
[router]
default_provider = \"\"

[router.provider.deepseek]
base_url = \"not-a-url\"
env_key = \"nope\"
catalog = \"not-a-cartridge\"
builtins = [\"grok-4.8\"]
";
        let builtins = builtins_from(config)
            .expect("builtins on a provider record should not replace the compiled list");
        assert_eq!(builtins, BUILTIN_SLUGS.map(str::to_owned).to_vec());
        let updated = set_default(config, "grok-4.7")
            .expect("set-default should not require default_provider or a valid record");
        assert!(updated.contains("default = \"grok-4.7\"\n"));
        assert!(
            updated.contains("base_url = \"not-a-url\""),
            "set-default should leave the provider record byte-identical"
        );
        let removed = remove_model(config, "grok-4.8")
            .expect_err("grok-4.8 on the provider record should not be a built-in");
        assert_eq!(removed.text, config);
        assert!(
            matches!(removed.reason, RefuseReason::NotConfigured { .. }),
            "grok-4.8 should be missing, not built-in, got {}",
            removed.reason
        );
    }

    #[test]
    fn ill_typed_router_refuses_the_three_edits_and_returns_the_original_text() {
        let not_a_table = format!("router = \"nope\"\n\n{FIXTURE}");
        assert_invalid_setting(&not_a_table, "router");
        for body in [
            "builtins = \"grok-4.8\"",
            "builtins = [\"\"]",
            "builtins = [1]",
            "builtins = [\"grok-4.8\", \"\"]",
        ] {
            let config = format!("[router]\n{body}\n\n{FIXTURE}");
            assert_invalid_setting(&config, "builtins");
        }
    }

    #[test]
    fn resolve_provider_does_not_validate_builtins() {
        let config = "\
[router]
builtins = \"nope\"
default_provider = 1

[router.provider.deepseek]
base_url = \"https://api.deepseek.com\"
env_key = \"DEEPSEEK_API_KEY\"
catalog = \"not-a-cartridge\"
";
        let record = resolve_provider(config, ProviderChoice::Explicit("deepseek"))
            .expect("resolve_provider should ignore builtins and default_provider");
        assert_eq!(record.base_url, "https://api.deepseek.com");
        assert_eq!(record.catalog.as_deref(), Some("not-a-cartridge"));
        let builtins =
            builtins_from(config).expect_err("the same builtins value should refuse membership");
        assert_setting(&builtins, "builtins");
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
            !written
                .windows(secret.len())
                .any(|window| window == secret.as_bytes()),
            "the written bytes should not contain the caller-supplied secret"
        );

        std::fs::write(&config, FIXTURE).expect("the config should reset for the cache-dir backup");
        let cache = tree.path().join("cache");
        replace_file(
            &config,
            Backup::CacheDir(&cache),
            updated.as_bytes(),
            secret,
        )
        .expect("the default-config backup should go under the cache dir");
        assert_eq!(
            std::fs::read(cache.join("config.toml.prev")).expect("cache-dir backup should exist"),
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
        assert!(matches!(refused.kind, ReplaceKind::SecretInReplacement));
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

    fn example_direct(window: Option<i64>, vision: bool) -> DirectBlock<'static> {
        DirectBlock {
            slug: "example-model",
            model: "example-model",
            base_url: "https://example.test/v1",
            name: None,
            api_backend: "messages",
            env_key: "EXAMPLE_API_KEY",
            context_window: window,
            vision,
        }
    }

    #[test]
    fn direct_window_and_vision_follow_the_flags() {
        let original = "# kept\n\n[models]\ndefault = \"grok-4.7\"\n";
        let omitted = insert_direct(original, example_direct(None, false))
            .expect("a direct add without window or vision should insert one span");
        let omitted_block = "\
[model.example-model]
model = \"example-model\"
base_url = \"https://example.test/v1\"
api_backend = \"messages\"
env_key = \"EXAMPLE_API_KEY\"
";
        assert_eq!(
            omitted,
            format!("# kept\n\n{omitted_block}\n[models]\ndefault = \"grok-4.7\"\n"),
            "absent window and vision should omit both lines and leave the other bytes"
        );
        assert!(
            !omitted.contains("context_window") && !omitted.contains("vision"),
            "the omitted block should not mention a window or vision; config: {omitted}"
        );
        assert_eq!(
            model_span_text(&omitted, "example-model").expect("the new span should be readable"),
            Some(omitted_block.to_owned()),
            "the stored span, without the separator blank line, should equal the rendered block"
        );

        for window in [0, -1] {
            let refused = insert_direct(original, example_direct(Some(window), false))
                .expect_err("a non-positive context_window should refuse");
            assert_eq!(
                refused.text, original,
                "context_window {window} should return the original text"
            );
            assert!(
                matches!(
                    refused.reason,
                    RefuseReason::InvalidSetting { ref key, .. } if key == "context_window"
                ),
                "context_window {window} should be the refusal, got {}",
                refused.reason
            );
        }

        let windowed = insert_direct(original, example_direct(Some(4096), false))
            .expect("a positive context_window should be written");
        assert!(
            windowed.contains("context_window = 4096\n"),
            "a positive window should be written as that integer; config: {windowed}"
        );
        assert!(
            !windowed.contains("vision"),
            "a window without --vision should not write vision; config: {windowed}"
        );

        let vision_only = insert_direct(original, example_direct(None, true))
            .expect("vision without a window should be written");
        assert!(
            vision_only.contains("env_key = \"EXAMPLE_API_KEY\"\nvision = true\n"),
            "vision without a window should follow env_key; config: {vision_only}"
        );
        assert!(
            !vision_only.contains("context_window"),
            "vision without a window should omit context_window; config: {vision_only}"
        );
        assert_eq!(
            vision_only.matches("vision").count(),
            1,
            "vision should be written once; config: {vision_only}"
        );
        assert!(
            !vision_only.contains("vision = false"),
            "vision should not be written as false; config: {vision_only}"
        );

        let both = insert_direct(original, example_direct(Some(4096), true))
            .expect("a window and vision should both be written");
        assert!(
            both.contains("context_window = 4096\nvision = true\n"),
            "vision should follow context_window when that line exists; config: {both}"
        );
        assert!(
            !both.contains("vision = false"),
            "vision true should not also write vision false; config: {both}"
        );

        let again = insert_direct(&omitted, example_direct(None, false))
            .expect("the same direct block should be a no-op");
        assert_eq!(
            again, omitted,
            "an equal span should return the original text"
        );

        let builtin = insert_direct(original, {
            let mut block = example_direct(None, false);
            block.slug = "grok-4.7";
            block
        })
        .expect_err("a compiled built-in slug should be in use when builtins is absent");
        assert_eq!(
            builtin.text, original,
            "a built-in slug should return the original text"
        );
        assert!(
            matches!(builtin.reason, RefuseReason::SlugInUse { .. }),
            "a built-in slug should be SlugInUse, got {}",
            builtin.reason
        );
    }

    #[test]
    fn openrouter_router_block_omits_builtins() {
        let block = openrouter_router_block();
        assert_eq!(
            block,
            "\
[router]
default_provider = \"openrouter\"

[router.provider.openrouter]
base_url = \"https://openrouter.ai/api/v1\"
env_key = \"OPENROUTER_API_KEY\"
catalog = \"openrouter-models\"
",
            "init should write the provider record and not a builtins key"
        );
        assert!(
            !block.contains("builtins"),
            "init must not freeze the compiled slug list, got:\n{block}"
        );
        assert_eq!(
            BUILTIN_SLUGS,
            ["grok-4.7", "grok-4.7-build-fast", "grok-4.6", "grok-4.5"],
            "a missing builtins key still falls back to these four compiled slugs"
        );
    }

    #[test]
    fn init_router_text_appends_without_reserializing_and_keeps_models() {
        let original = "# kept comment\n[models]\ndefault = \"grok-4.5\"";
        let text = match init_router_text(original)
            .expect("a file with no router key should accept the block")
        {
            RouterInit::Append(text) => text,
            RouterInit::AlreadyPresent => {
                panic!("a file with no [router] key should not count as already present")
            }
        };
        assert_eq!(
            text,
            format!("{original}\n\n{}", openrouter_router_block()),
            "a file that does not end in a newline should end its last line, then leave one blank line before [router]"
        );
        let already_separated = format!("{original}\n\n");
        let separated = match init_router_text(&already_separated)
            .expect("a file that already ends with a blank line should accept the block")
        {
            RouterInit::Append(text) => text,
            RouterInit::AlreadyPresent => {
                panic!("a trailing blank line is not an existing [router] table")
            }
        };
        assert_eq!(
            separated,
            format!("{already_separated}{}", openrouter_router_block()),
            "a file that already ends with a blank line should not gain a second one"
        );
        let before = original
            .parse::<toml::Table>()
            .expect("the original comment and models table should parse");
        let after = text
            .parse::<toml::Table>()
            .expect("the appended text should parse");
        assert_eq!(
            before.get("models"),
            after.get("models"),
            "appending [router] should leave the [models] table equal"
        );
        let again = init_router_text(&text)
            .expect("a second plan should see the table the first plan added");
        assert_eq!(
            again,
            RouterInit::AlreadyPresent,
            "a second init should not build another block"
        );
    }

    #[test]
    fn init_router_text_keeps_an_existing_router_table_including_a_dotted_key() {
        let header = "[router]\ndefault_provider = \"deepseek\"\n";
        assert_eq!(
            init_router_text(header).expect("a [router] table should be left alone"),
            RouterInit::AlreadyPresent
        );
        let dotted = "router.default_provider = \"openrouter\"\n";
        assert_eq!(
            init_router_text(dotted).expect("a dotted router key is already a table"),
            RouterInit::AlreadyPresent
        );
        assert_eq!(
            init_router_text("").expect("an empty file should become the block"),
            RouterInit::Append(openrouter_router_block())
        );
    }

    #[test]
    fn init_router_text_refuses_a_router_key_that_is_not_a_table() {
        let error = init_router_text("router = \"nope\"\n")
            .expect_err("a string router key should refuse before any rewrite");
        assert!(
            matches!(error, RouterInitError::NotTable),
            "a non-table router key should be NotTable, got {error}"
        );
    }

    #[test]
    fn create_file_writes_a_new_path_without_a_backup() {
        let tree = TempTree::new("create");
        let config = tree.path().join("nested").join("config.toml");
        let bytes = b"created\n";
        create_file(&config, bytes, "").expect("a missing file should be created");
        assert_eq!(
            std::fs::read(&config).expect("the new file should be readable"),
            bytes,
            "create_file should leave the bytes it was given"
        );
        assert!(
            !config.with_file_name("config.toml.prev").exists(),
            "a new file has nothing to back up"
        );
    }

    #[test]
    fn remove_model_deletes_a_quoted_slug_and_its_child_table() {
        let config = "\
[model.\"zai-glm-5.3\"]
model = \"glm-5.3\"
base_url = \"https://example.test/v1\"
name = \"GLM\"
env_key = \"ZAI_API_KEY\"
context_window = 600000

[model.\"zai-glm-5.3\".extra_headers]
anthropic-version = \"2023-06-01\"

[model.hub-default]
model = \"default\"
base_url = \"http://hub.example/v1\"
name = \"Hub\"
env_key = \"VIVI_PROXY_TOKEN\"
context_window = 131072

[models]
default = \"grok-4.7\"
";
        let edited =
            remove_model(config, "zai-glm-5.3").expect("the quoted slug should be removed");
        assert!(
            !edited.contains("zai-glm-5.3"),
            "the quoted slug and its child table should be gone; config: {edited}"
        );
        assert!(
            edited.contains("[model.hub-default]"),
            "the next model should stay; config: {edited}"
        );
        assert!(
            edited.contains("default = \"grok-4.7\""),
            "the saved default should stay; config: {edited}"
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
