//! Catalog rows, matching, cache decisions, and two catalog cartridges.
//!
//! Search reads. It never writes the config. The only write helper here stores
//! non-empty catalog bytes under a caller-supplied cache directory.
//! Cache freshness is an age the caller passes in. Nothing in this module
//! reads the clock.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde_json::Value;

use crate::config_edit::{CatalogModel, RefuseReason, derive_slug, require_slug};

/// Default OpenRouter models URL. A derived URL equal to this caches as `models.json`.
pub const MODELS_URL: &str = "https://openrouter.ai/api/v1/models";

/// Cache file name for [`MODELS_URL`] only. Any other models URL uses [`cache_file_for`].
pub const CACHE_FILE_NAME: &str = "models.json";

/// A cache younger than this duration is the answer, unless refresh was asked for.
pub const CACHE_FRESH: Duration = Duration::from_secs(60 * 60);

/// Largest response body accepted from the models endpoint.
const CATALOG_BODY_LIMIT: u64 = 32 * 1024 * 1024;

/// One catalog row, parsed once so later edits do not parse the JSON again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogRow {
    id: String,
    name: String,
    description: String,
    context_length: Option<i64>,
    sees_image: bool,
}

impl CatalogRow {
    /// Provider id, for example `z-ai/glm-5.3`.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Catalog display name, or an empty string when the document omitted it.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Name shown to a person. A blank catalog name falls back to the id.
    #[must_use]
    pub fn display_name(&self) -> &str {
        if self.name.is_empty() {
            &self.id
        } else {
            &self.name
        }
    }

    /// Catalog description, or an empty string when the document omitted it.
    #[must_use]
    pub fn description(&self) -> &str {
        &self.description
    }

    /// Catalog `context_length`, including non-positive values.
    ///
    /// `None` means the field was missing or JSON null. The add path refuses
    /// a missing or non-positive window; this type does not hide that fact.
    #[must_use]
    pub fn context_length(&self) -> Option<i64> {
        self.context_length
    }

    /// Whether `architecture.input_modalities` contains `image`.
    #[must_use]
    pub fn sees_image(&self) -> bool {
        self.sees_image
    }

    /// Borrowed fields for [`CatalogModel`].
    ///
    /// A blank name becomes `None` so the splice can apply its own fallback.
    #[must_use]
    pub fn as_catalog_model(&self) -> CatalogModel<'_> {
        CatalogModel {
            id: &self.id,
            name: (!self.name.is_empty()).then_some(self.name.as_str()),
            context_window: self.context_length,
            sees_image: self.sees_image,
        }
    }

    /// Whether every term appears, case-insensitively, in the id, name, or description.
    fn matches_all(&self, terms: &[&str]) -> bool {
        terms.iter().all(|term| self.matches_term(term))
    }

    /// Whether one term appears in the id, the name, or the description.
    fn matches_term(&self, term: &str) -> bool {
        contains_ignore_case(&self.id, term)
            || contains_ignore_case(&self.name, term)
            || contains_ignore_case(&self.description, term)
    }
}

/// Why a catalog load, match, or cache write stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CatalogError {
    /// `OPENROUTER_API_KEY` was missing or blank. Nothing was downloaded or cached.
    MissingKey,
    /// The models URL was empty or only whitespace. No request was built.
    EmptyUrl,
    /// The bytes were not JSON.
    Json {
        /// Parser detail.
        detail: String,
    },
    /// The JSON was not an OpenRouter models document.
    Shape {
        /// What was missing.
        detail: String,
    },
    /// One `data` element could not be turned into a row.
    Row {
        /// Index in the `data` array.
        index: usize,
        /// What was wrong with that element.
        detail: String,
    },
    /// The download returned no bytes. The cache was not written.
    EmptyBody,
    /// The download failed. The cache was not written.
    Http {
        /// Failure detail, with the API key removed.
        detail: String,
    },
    /// The cache file could not be read or written.
    CacheIo {
        /// Path that failed.
        path: String,
        /// Filesystem detail.
        detail: String,
    },
    /// A fresh cache file was not valid catalog JSON.
    CacheInvalid {
        /// Cache file that failed to parse.
        path: String,
        /// Parse detail.
        detail: String,
    },
    /// A caller asked to store an empty body. No file was written.
    EmptyCacheWrite,
    /// Direct-catalog bytes were not JSON. This is not an OpenRouter document error.
    DirectJson {
        /// Parser detail.
        detail: String,
    },
    /// Direct-catalog JSON was not a `data` array of model objects.
    DirectShape {
        /// What was missing.
        detail: String,
    },
}

impl CatalogError {
    /// A filesystem failure against `path`.
    fn cache_io(path: &Path, err: &io::Error) -> Self {
        Self::CacheIo {
            path: path.display().to_string(),
            detail: err.to_string(),
        }
    }
}

impl fmt::Display for CatalogError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingKey => write!(
                formatter,
                "OPENROUTER_API_KEY is missing, so the catalog was not downloaded. Export OPENROUTER_API_KEY or pass --catalog-file. The config was not written and an empty cache was not written."
            ),
            Self::EmptyUrl => write!(
                formatter,
                "the models URL is empty or only whitespace, so no catalog request was built. Pass a non-blank models URL. Nothing was downloaded and the cache was not written."
            ),
            Self::Json { detail } => write!(
                formatter,
                "the catalog JSON could not be parsed: {detail}. Pass an OpenRouter /api/v1/models document via --catalog-file, or pass --refresh."
            ),
            Self::Shape { detail } => write!(
                formatter,
                "{detail}. Pass an OpenRouter /api/v1/models document via --catalog-file."
            ),
            Self::Row { index, detail } => write!(
                formatter,
                "catalog data[{index}] {detail}. Fix that element or remove it from the file."
            ),
            Self::EmptyBody => write!(
                formatter,
                "the catalog download was empty. 0 bytes. Nothing was cached. Check OPENROUTER_API_KEY or pass --catalog-file."
            ),
            Self::Http { detail } => write!(
                formatter,
                "the catalog download failed: {detail}. Nothing was cached. Check the network and OPENROUTER_API_KEY, or pass --catalog-file."
            ),
            Self::CacheIo { path, detail } => write!(
                formatter,
                "the catalog cache at {path} could not be used: {detail}. Pass --cache-dir or --catalog-file."
            ),
            Self::CacheInvalid { path, detail } => write!(
                formatter,
                "the cached catalog at {path} is not valid OpenRouter JSON: {detail}. Pass --refresh to download it again."
            ),
            Self::EmptyCacheWrite => write!(
                formatter,
                "refusing to write an empty catalog cache. 0 bytes. Pass --catalog-file or retry the download."
            ),
            Self::DirectJson { detail } => write!(
                formatter,
                "the catalog JSON could not be parsed: {detail}. Pass a deepseek-list-models document. Nothing was written."
            ),
            Self::DirectShape { detail } => write!(
                formatter,
                "{detail}. Pass a deepseek-list-models document. Nothing was written."
            ),
        }
    }
}

impl std::error::Error for CatalogError {}

/// GET request for the models catalog. Building it does not send it.
#[derive(Clone, PartialEq, Eq)]
pub struct CatalogRequest {
    api_key: String,
    url: String,
}

impl CatalogRequest {
    /// HTTP method. Always `GET`.
    #[must_use]
    pub fn method(&self) -> &'static str {
        "GET"
    }

    /// Models URL this request would GET.
    #[must_use]
    pub fn url(&self) -> &str {
        &self.url
    }

    /// `Authorization` header value.
    #[must_use]
    pub fn authorization(&self) -> String {
        format!("Bearer {}", self.api_key)
    }
}

impl fmt::Debug for CatalogRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CatalogRequest")
            .field("method", &self.method())
            .field("url", &self.url())
            .field("authorization", &"Bearer <redacted>")
            .finish()
    }
}

/// Whether cached bytes are fresh enough to be the answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheChoice {
    /// The age is inside one hour and refresh was not requested.
    Use,
    /// The age is one hour or older, or refresh was requested.
    Fetch,
}

/// A fresh parsed cache, or a request the caller still has to send.
#[derive(Debug)]
pub enum CachedOrFetch {
    /// Rows from a cache younger than one hour.
    Cached(Vec<CatalogRow>),
    /// GET the caller should perform. Write the body only after it is non-empty.
    Fetch(CatalogRequest),
}

/// Zero matches, or more than one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveError {
    /// No row contained every term.
    None {
        /// Terms that matched nothing.
        terms: Vec<String>,
    },
    /// More than one row matched.
    Many {
        /// How many rows matched.
        count: usize,
        /// Terms that were too broad, or an id that appeared more than once.
        terms: Vec<String>,
        /// The single term was an exact id and the catalog listed it more than once.
        exact_id: bool,
    },
}

impl ResolveError {
    /// How many rows matched.
    #[must_use]
    pub fn count(&self) -> usize {
        match self {
            Self::None { .. } => 0,
            Self::Many { count, .. } => *count,
        }
    }
}

impl fmt::Display for ResolveError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None { terms } => write!(
                formatter,
                "no catalog row matched {}. 0 matches. Try fewer terms or pass --catalog-file with the catalog you meant.",
                display_terms(terms)
            ),
            Self::Many {
                count,
                terms,
                exact_id: true,
            } => write!(
                formatter,
                "{count} catalog rows matched the exact id {}. {count} matches. Pass more terms to tell them apart.",
                display_terms(terms)
            ),
            Self::Many {
                count,
                terms,
                exact_id: false,
            } => write!(
                formatter,
                "{count} catalog rows matched {}. {count} matches. Pass the exact provider id.",
                display_terms(terms)
            ),
        }
    }
}

impl std::error::Error for ResolveError {}

/// Build the models GET. A blank key or a blank URL does not build a request.
///
/// # Errors
///
/// Returns [`CatalogError::MissingKey`] when `api_key` is empty or only whitespace.
/// Returns [`CatalogError::EmptyUrl`] when `models_url` is empty or only whitespace.
#[must_use = "the request is not sent until fetch_catalog_bytes"]
pub fn catalog_request(api_key: &str, models_url: &str) -> Result<CatalogRequest, CatalogError> {
    if api_key.is_empty() || api_key.chars().all(char::is_whitespace) {
        return Err(CatalogError::MissingKey);
    }
    if models_url.is_empty() || models_url.chars().all(char::is_whitespace) {
        return Err(CatalogError::EmptyUrl);
    }
    Ok(CatalogRequest {
        api_key: api_key.to_owned(),
        url: models_url.to_owned(),
    })
}

/// Decide whether a cache of this age is the answer.
///
/// An age of exactly one hour is not inside the fresh window, so it is fetched.
/// `refresh` fetches even when the age is inside the window.
#[must_use]
pub fn cache_choice(age: Duration, refresh: bool) -> CacheChoice {
    if refresh || age >= CACHE_FRESH {
        CacheChoice::Fetch
    } else {
        CacheChoice::Use
    }
}

/// Parse an OpenRouter `/api/v1/models` document.
///
/// # Errors
///
/// Returns a [`CatalogError`] when the bytes are not a `data` array of model objects.
pub fn parse_catalog(bytes: &[u8]) -> Result<Vec<CatalogRow>, CatalogError> {
    let value: Value = serde_json::from_slice(bytes).map_err(|err| CatalogError::Json {
        detail: err.to_string(),
    })?;
    let Some(data) = value.get("data").and_then(Value::as_array) else {
        return Err(CatalogError::Shape {
            detail: "the catalog JSON has no data array".to_owned(),
        });
    };
    let mut rows = Vec::with_capacity(data.len());
    for (index, item) in data.iter().enumerate() {
        rows.push(parse_row(index, item)?);
    }
    Ok(rows)
}

/// One catalog cartridge. Variant methods dispatch on `self` and do not match a catalog id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cartridge {
    /// `openrouter-models`. [`CatalogCartridge::parse`] calls [`parse_catalog`].
    OpenRouter,
    /// `deepseek-list-models`. Reads `id`, `name`, `context_window`, and `input_modalities`.
    DeepSeek,
}

/// Why [`cartridge`] did not return a cartridge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CartridgeError {
    /// The catalog id was absent or empty.
    Missing,
    /// `id` is not a known cartridge. OpenRouter was not selected.
    Unknown {
        /// Catalog id that was refused.
        id: String,
    },
}

impl fmt::Display for CartridgeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing => write!(
                formatter,
                "the catalog id is missing. Pass openrouter-models or deepseek-list-models. No cartridge was selected and nothing was written."
            ),
            Self::Unknown { id } => write!(
                formatter,
                "catalog id {id:?} is not a cartridge, so openrouter-models was not selected. Pass openrouter-models or deepseek-list-models. Nothing was written."
            ),
        }
    }
}

impl std::error::Error for CartridgeError {}

/// Parse catalog bytes and choose a config slug.
///
/// `slug`'s second argument is `--as`. OpenRouter derives `or-` plus the leaf
/// when it is absent. DeepSeek uses the model id itself and does not invent a leaf.
pub trait CatalogCartridge {
    /// Parse `bytes` into rows.
    ///
    /// # Errors
    ///
    /// Returns a [`CatalogError`] when `bytes` are not this cartridge's document.
    fn parse(&self, bytes: &[u8]) -> Result<Vec<CatalogRow>, CatalogError>;

    /// Choose the config slug for `model_id`.
    ///
    /// # Errors
    ///
    /// Returns [`RefuseReason`] when `model_id` or `explicit` is not a usable slug.
    fn slug(&self, model_id: &str, explicit: Option<&str>) -> Result<String, RefuseReason>;
}

impl CatalogCartridge for Cartridge {
    fn parse(&self, bytes: &[u8]) -> Result<Vec<CatalogRow>, CatalogError> {
        match self {
            Self::OpenRouter => parse_catalog(bytes),
            Self::DeepSeek => parse_direct(bytes),
        }
    }

    fn slug(&self, model_id: &str, explicit: Option<&str>) -> Result<String, RefuseReason> {
        match self {
            Self::OpenRouter => openrouter_slug(model_id, explicit),
            Self::DeepSeek => deepseek_slug(model_id, explicit),
        }
    }
}

/// Select the cartridge named by `catalog_id`.
///
/// `None` and `Some("")` are missing. Any other string is unknown and does not
/// select OpenRouter. This is the only match on the catalog id strings.
///
/// # Errors
///
/// Returns [`CartridgeError::Missing`] or [`CartridgeError::Unknown`].
#[must_use = "the cartridge is the caller's only copy of the selected parser"]
pub fn cartridge(catalog_id: Option<&str>) -> Result<Cartridge, CartridgeError> {
    match catalog_id {
        None | Some("") => Err(CartridgeError::Missing),
        Some("openrouter-models") => Ok(Cartridge::OpenRouter),
        Some("deepseek-list-models") => Ok(Cartridge::DeepSeek),
        Some(id) => Err(CartridgeError::Unknown { id: id.to_owned() }),
    }
}

/// OpenRouter slug: derive `or-` plus the leaf, or require `--as` as given.
fn openrouter_slug(model_id: &str, explicit: Option<&str>) -> Result<String, RefuseReason> {
    match explicit {
        Some(slug) => {
            require_slug(slug)?;
            Ok(slug.to_owned())
        }
        None => derive_slug(model_id),
    }
}

/// DeepSeek slug: the id itself, or `--as` when present. Never a derived leaf.
fn deepseek_slug(model_id: &str, explicit: Option<&str>) -> Result<String, RefuseReason> {
    let slug = explicit.unwrap_or(model_id);
    require_slug(slug)?;
    Ok(slug.to_owned())
}

/// Rows in which every term appears in the id, the name, or the description.
///
/// Matching is case-insensitive. An empty term list returns every row.
/// Each term is tested on its own; a term does not have to sit in all three fields.
#[must_use]
pub fn matching<'a>(rows: &'a [CatalogRow], terms: &[&str]) -> Vec<&'a CatalogRow> {
    rows.iter()
        .filter(|row| row.matches_all(terms))
        .collect()
}

/// One exact id, or one unique term match.
///
/// Exact id comparison is case-insensitive and wins over a broader term match.
/// Zero matches and more than one match are errors.
///
/// # Errors
///
/// Returns [`ResolveError::None`] or [`ResolveError::Many`].
pub fn resolve<'a>(rows: &'a [CatalogRow], terms: &[&str]) -> Result<&'a CatalogRow, ResolveError> {
    if let [term] = terms {
        let exact = exact_ids(rows, term);
        match exact.as_slice() {
            [row] => return Ok(*row),
            [] => {}
            many => {
                return Err(ResolveError::Many {
                    count: many.len(),
                    terms: owned_terms(terms),
                    exact_id: true,
                });
            }
        }
    }
    let matched = matching(rows, terms);
    match matched.as_slice() {
        [row] => Ok(*row),
        many => Err(if many.is_empty() {
            ResolveError::None {
                terms: owned_terms(terms),
            }
        } else {
            ResolveError::Many {
                count: many.len(),
                terms: owned_terms(terms),
                exact_id: false,
            }
        }),
    }
}

/// Read a fresh cache for `base_url`, or build the GET that should replace it.
///
/// The path is [`cache_file_for`] of [`models_url`]. Does not write, does not
/// call the network, and does not read the clock. `now` is the caller's instant.
/// A cache whose modification time is in the future is not used.
///
/// # Errors
///
/// Returns [`CatalogError::MissingKey`] when a download is required and `api_key` is blank.
/// Returns [`CatalogError::CacheInvalid`] when a fresh cache is not catalog JSON.
/// Returns [`CatalogError::CacheIo`] when an existing cache file cannot be read.
pub fn prepare_cached_load(
    cache_dir: &Path,
    now: SystemTime,
    refresh: bool,
    api_key: &str,
    base_url: &str,
) -> Result<CachedOrFetch, CatalogError> {
    match fresh_bytes_or_fetch(cache_dir, now, refresh, api_key, base_url)? {
        FreshBytesOrFetch::Fresh { path, bytes } => parse_catalog(&bytes)
            .map(CachedOrFetch::Cached)
            .map_err(|err| CatalogError::CacheInvalid {
                path: path.display().to_string(),
                detail: err.to_string(),
            }),
        FreshBytesOrFetch::Fetch(request) => Ok(CachedOrFetch::Fetch(request)),
    }
}

/// Fresh cache bytes, or a request the caller still has to send.
///
/// The bytes are not parsed.
#[derive(Debug)]
pub enum CachedBodyOrFetch {
    /// Bytes from a cache younger than one hour.
    Cached(Vec<u8>),
    /// GET the caller should perform. Write the body only after it is non-empty.
    Fetch(CatalogRequest),
}

/// Read a fresh cache body for `base_url`, or build the GET that should replace it.
///
/// The path is [`cache_file_for`] of [`models_url`]. Does not parse, does not
/// write, does not call the network, and does not read the clock. `now` is
/// the caller's instant. A cache whose modification time is in the future is
/// not used. `base_url` is a base URL, not a provider name or a catalog id.
///
/// # Errors
///
/// Returns [`CatalogError::MissingKey`] when a download is required and `api_key` is blank.
/// Returns [`CatalogError::CacheIo`] when an existing cache file cannot be read.
pub fn prepare_cached_body(
    cache_dir: &Path,
    now: SystemTime,
    refresh: bool,
    api_key: &str,
    base_url: &str,
) -> Result<CachedBodyOrFetch, CatalogError> {
    match fresh_bytes_or_fetch(cache_dir, now, refresh, api_key, base_url)? {
        FreshBytesOrFetch::Fresh { bytes, .. } => Ok(CachedBodyOrFetch::Cached(bytes)),
        FreshBytesOrFetch::Fetch(request) => Ok(CachedBodyOrFetch::Fetch(request)),
    }
}

/// A fresh non-empty cache body, or the GET that should replace it.
enum FreshBytesOrFetch {
    /// Bytes at `path`, younger than one hour. Not parsed.
    Fresh { path: PathBuf, bytes: Vec<u8> },
    /// GET the caller should perform.
    Fetch(CatalogRequest),
}

/// Load a fresh cache body for `base_url`, or build its models GET.
///
/// Does not parse. An empty cache and a cache at least one hour old both fall
/// through to the request. `refresh` skips the cache.
fn fresh_bytes_or_fetch(
    cache_dir: &Path,
    now: SystemTime,
    refresh: bool,
    api_key: &str,
    base_url: &str,
) -> Result<FreshBytesOrFetch, CatalogError> {
    let url = models_url(base_url);
    let path = cache_file_for(cache_dir, &url);
    if !refresh {
        if let Some((bytes, age)) = read_cache_age(&path, now)? {
            if !bytes.is_empty() && cache_choice(age, false) == CacheChoice::Use {
                return Ok(FreshBytesOrFetch::Fresh { path, bytes });
            }
        }
    }
    catalog_request(api_key, &url).map(FreshBytesOrFetch::Fetch)
}

/// Download the catalog body. Does not write the cache or the config.
///
/// # Errors
///
/// Returns [`CatalogError::Http`] or [`CatalogError::EmptyBody`] without writing a file.
pub fn fetch_catalog_bytes(request: &CatalogRequest) -> Result<Vec<u8>, CatalogError> {
    // A non-default models URL is sent. Only a non-GET is refused.
    if request.method() != "GET" {
        return Err(CatalogError::Http {
            detail: "refusing to send a catalog request that is not GET".to_owned(),
        });
    }
    let authorization = request.authorization();
    let mut response = ureq::get(request.url())
        .header("Authorization", &authorization)
        .call()
        .map_err(|err| CatalogError::Http {
            detail: redact(&err.to_string(), request),
        })?;
    let bytes = response
        .body_mut()
        .with_config()
        .limit(CATALOG_BODY_LIMIT)
        .read_to_vec()
        .map_err(|err| CatalogError::Http {
            detail: redact(&err.to_string(), request),
        })?;
    if bytes.is_empty() {
        return Err(CatalogError::EmptyBody);
    }
    Ok(bytes)
}

/// Models URL for a catalog base.
///
/// Removes one trailing `/` when `base_url` ends with one, then appends
/// `/models`. Does not trim and does not remove more than one slash.
/// `https://example.invalid/v1` and `https://example.invalid/v1/` both
/// become `https://example.invalid/v1/models`.
#[must_use]
pub fn models_url(base_url: &str) -> String {
    let without_one_slash = base_url.strip_suffix('/').unwrap_or(base_url);
    format!("{without_one_slash}/models")
}

/// Path of `models.json` inside `cache_dir`.
///
/// Correct only for the derived default models URL. Other URLs use [`cache_file_for`].
#[must_use]
pub fn cache_file(cache_dir: &Path) -> PathBuf {
    cache_dir.join(CACHE_FILE_NAME)
}

/// Cache path for one models URL.
///
/// [`MODELS_URL`] uses [`cache_file`]. Any other URL uses
/// `<cache_dir>/models-<hex>.json`. `<hex>` is the lowercase hex of the URL's
/// UTF-8 bytes, two digits per byte, with no separators. That file name is
/// never `models.json`.
#[must_use]
pub fn cache_file_for(cache_dir: &Path, models_url: &str) -> PathBuf {
    cache_dir.join(cache_file_name(models_url))
}

/// File name for `models_url`. Never `models.json` unless `models_url` is [`MODELS_URL`].
fn cache_file_name(models_url: &str) -> String {
    if models_url == MODELS_URL {
        CACHE_FILE_NAME.to_owned()
    } else {
        format!("models-{}.json", encode_hex(models_url.as_bytes()))
    }
}

/// Lowercase hex of `bytes`, two digits per byte, no separators.
fn encode_hex(bytes: &[u8]) -> String {
    let mut encoded = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        encoded.push_str(&format!("{byte:02x}"));
    }
    encoded
}

/// Store catalog bytes for `base_url` under `cache_dir`.
///
/// The path is [`cache_file_for`] of [`models_url`]. Empty bytes are refused
/// and no file is created. A non-empty body is written to a temporary file in
/// `cache_dir` that is not named `models.json`, then renamed into place.
///
/// # Errors
///
/// Returns [`CatalogError::EmptyCacheWrite`] or [`CatalogError::CacheIo`].
pub fn write_cache(cache_dir: &Path, base_url: &str, bytes: &[u8]) -> Result<PathBuf, CatalogError> {
    if bytes.is_empty() {
        return Err(CatalogError::EmptyCacheWrite);
    }
    fs::create_dir_all(cache_dir).map_err(|err| CatalogError::cache_io(cache_dir, &err))?;
    let url = models_url(base_url);
    let destination = cache_file_for(cache_dir, &url);
    let temporary = cache_dir.join(format!(".{}.tmp", cache_file_name(&url)));
    fs::write(&temporary, bytes).map_err(|err| CatalogError::cache_io(&temporary, &err))?;
    if let Err(err) = fs::rename(&temporary, &destination) {
        let rename = CatalogError::cache_io(&destination, &err);
        return match fs::remove_file(&temporary) {
            Ok(()) => Err(rename),
            Err(remove_err) => Err(CatalogError::CacheIo {
                path: temporary.display().to_string(),
                detail: format!(
                    "{rename}; also failed to remove the temporary file: {remove_err}"
                ),
            }),
        };
    }
    Ok(destination)
}

/// Parse one `data` element into a row.
fn parse_row(index: usize, item: &Value) -> Result<CatalogRow, CatalogError> {
    let Some(object) = item.as_object() else {
        return Err(CatalogError::Row {
            index,
            detail: "is not an object".to_owned(),
        });
    };
    let id = required_string(object.get("id"), index, "id")?;
    if id.is_empty() {
        return Err(CatalogError::Row {
            index,
            detail: "has no id".to_owned(),
        });
    }
    let name = optional_string(object.get("name"), index, "name")?
        .unwrap_or_default()
        .trim()
        .to_owned();
    let description = optional_string(object.get("description"), index, "description")?
        .unwrap_or_default()
        .trim()
        .to_owned();
    Ok(CatalogRow {
        id,
        name,
        description,
        context_length: optional_context_length(object.get("context_length"), index)?,
        sees_image: sees_image(object.get("architecture"), index)?,
    })
}

/// Parse a direct list-models document.
///
/// Reads `id`, `name`, `context_window`, and `input_modalities` from each
/// `data` element. Does not read `description`, `context_length`,
/// `architecture`, or any other field. Does not write a file.
fn parse_direct(bytes: &[u8]) -> Result<Vec<CatalogRow>, CatalogError> {
    let value: Value = serde_json::from_slice(bytes).map_err(|err| CatalogError::DirectJson {
        detail: err.to_string(),
    })?;
    let Some(data) = value.get("data").and_then(Value::as_array) else {
        return Err(CatalogError::DirectShape {
            detail: "the catalog JSON has no data array".to_owned(),
        });
    };
    let mut rows = Vec::with_capacity(data.len());
    for (index, item) in data.iter().enumerate() {
        rows.push(parse_direct_row(index, item)?);
    }
    Ok(rows)
}

/// Parse one direct `data` element. Other fields on the object are ignored.
fn parse_direct_row(index: usize, item: &Value) -> Result<CatalogRow, CatalogError> {
    let Some(object) = item.as_object() else {
        return Err(CatalogError::Row {
            index,
            detail: "is not an object".to_owned(),
        });
    };
    let id = required_string(object.get("id"), index, "id")?;
    if id.is_empty() {
        return Err(CatalogError::Row {
            index,
            detail: "has no id".to_owned(),
        });
    }
    let name = match optional_string(object.get("name"), index, "name")? {
        Some(text) => text.trim().to_owned(),
        None => String::new(),
    };
    Ok(CatalogRow {
        id,
        name,
        description: String::new(),
        context_length: optional_context_window(object.get("context_window"), index)?,
        sees_image: direct_sees_image(object.get("input_modalities"), index)?,
    })
}

/// Read `context_window` when it is an integer, null, or absent.
///
/// Zero and other non-positive integers are kept. A present non-integer fails the row.
fn optional_context_window(
    value: Option<&Value>,
    index: usize,
) -> Result<Option<i64>, CatalogError> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(number)) => {
            if let Some(integer) = number.as_i64() {
                return Ok(Some(integer));
            }
            if let Some(unsigned) = number.as_u64() {
                return i64::try_from(unsigned).map(Some).map_err(|_| CatalogError::Row {
                    index,
                    detail: "context_window does not fit in an i64".to_owned(),
                });
            }
            Err(CatalogError::Row {
                index,
                detail: "context_window is not an integer".to_owned(),
            })
        }
        Some(_) => Err(CatalogError::Row {
            index,
            detail: "context_window is not an integer".to_owned(),
        }),
    }
}

/// Whether top-level `input_modalities` contains `image`.
///
/// Absence and null are no image. A present non-array fails the row.
fn direct_sees_image(value: Option<&Value>, index: usize) -> Result<bool, CatalogError> {
    match value {
        None | Some(Value::Null) => Ok(false),
        Some(Value::Array(list)) => Ok(list.iter().any(|item| {
            item.as_str()
                .is_some_and(|modality| modality.eq_ignore_ascii_case("image"))
        })),
        Some(_) => Err(CatalogError::Row {
            index,
            detail: "input_modalities is not an array".to_owned(),
        }),
    }
}

/// Read a required string field.
fn required_string(
    value: Option<&Value>,
    index: usize,
    field: &str,
) -> Result<String, CatalogError> {
    match value {
        Some(Value::String(text)) => Ok(text.trim().to_owned()),
        Some(_) => Err(CatalogError::Row {
            index,
            detail: format!("has a non-string {field}"),
        }),
        None => Err(CatalogError::Row {
            index,
            detail: format!("has no {field}"),
        }),
    }
}

/// Read an optional string field. Null and absence become `None`.
fn optional_string(
    value: Option<&Value>,
    index: usize,
    field: &str,
) -> Result<Option<String>, CatalogError> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => Ok(Some(text.to_owned())),
        Some(_) => Err(CatalogError::Row {
            index,
            detail: format!("has a non-string {field}"),
        }),
    }
}

/// Read `context_length` when it is an integer, null, or absent.
fn optional_context_length(
    value: Option<&Value>,
    index: usize,
) -> Result<Option<i64>, CatalogError> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(number)) => {
            if let Some(integer) = number.as_i64() {
                return Ok(Some(integer));
            }
            if let Some(unsigned) = number.as_u64() {
                return i64::try_from(unsigned).map(Some).map_err(|_| CatalogError::Row {
                    index,
                    detail: "context_length does not fit in an i64".to_owned(),
                });
            }
            Err(CatalogError::Row {
                index,
                detail: "context_length is not an integer".to_owned(),
            })
        }
        Some(_) => Err(CatalogError::Row {
            index,
            detail: "context_length is not an integer".to_owned(),
        }),
    }
}

/// Whether architecture input modalities include image.
fn sees_image(value: Option<&Value>, index: usize) -> Result<bool, CatalogError> {
    let Some(value) = value else {
        return Ok(false);
    };
    if value.is_null() {
        return Ok(false);
    }
    let Some(architecture) = value.as_object() else {
        return Err(CatalogError::Row {
            index,
            detail: "has a non-object architecture".to_owned(),
        });
    };
    let Some(modalities) = architecture.get("input_modalities") else {
        return Ok(false);
    };
    if modalities.is_null() {
        return Ok(false);
    }
    let Some(list) = modalities.as_array() else {
        return Err(CatalogError::Row {
            index,
            detail: "architecture.input_modalities is not an array".to_owned(),
        });
    };
    Ok(list.iter().any(|item| {
        item.as_str()
            .is_some_and(|modality| modality.eq_ignore_ascii_case("image"))
    }))
}

/// Case-insensitive substring test. An empty needle matches.
fn contains_ignore_case(haystack: &str, needle: &str) -> bool {
    let needle = needle.to_lowercase();
    if needle.is_empty() {
        return true;
    }
    haystack.to_lowercase().contains(&needle)
}

/// Rows whose id equals `term`, ignoring ASCII case.
fn exact_ids<'a>(rows: &'a [CatalogRow], term: &str) -> Vec<&'a CatalogRow> {
    rows.iter()
        .filter(|row| row.id.eq_ignore_ascii_case(term))
        .collect()
}

/// Own the terms so an error can outlive the caller's slice.
fn owned_terms(terms: &[&str]) -> Vec<String> {
    terms.iter().map(|term| (*term).to_owned()).collect()
}

/// Quote terms for an error sentence.
fn display_terms(terms: &[String]) -> String {
    if terms.is_empty() {
        return "the empty term list".to_owned();
    }
    terms
        .iter()
        .map(|term| format!("{term:?}"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Age of a cache file, or `None` when the file is absent or its mtime is in the future.
fn read_cache_age(
    path: &Path,
    now: SystemTime,
) -> Result<Option<(Vec<u8>, Duration)>, CatalogError> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(CatalogError::cache_io(path, &err)),
    };
    let modified = fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .map_err(|err| CatalogError::cache_io(path, &err))?;
    let Some(age) = now.duration_since(modified).ok() else {
        return Ok(None);
    };
    Ok(Some((bytes, age)))
}

/// Remove the API key from a transport error string.
fn redact(detail: &str, request: &CatalogRequest) -> String {
    detail.replace(&request.api_key, "<redacted>")
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{Duration, SystemTime};

    use super::*;

    const COMPILED_BASE: &str = crate::config_edit::OPENROUTER_BASE_URL;

    const FIXTURE: &str = r#"{
        "data": [
            {
                "id": "gpt-4",
                "name": "GPT-4",
                "description": "Older flagship",
                "context_length": 8192,
                "architecture": { "input_modalities": ["text"] }
            },
            {
                "id": "gpt-4-turbo",
                "name": "GPT-4 Turbo",
                "description": "Sees pictures",
                "context_length": 128000,
                "architecture": { "input_modalities": ["text", "Image"] }
            },
            {
                "id": "meta/llama",
                "name": "   ",
                "description": "Text only llama",
                "context_length": null
            }
        ]
    }"#;

    fn rows() -> Vec<CatalogRow> {
        parse_catalog(FIXTURE.as_bytes()).expect("the catalog fixture should parse")
    }

    fn row<'a>(rows: &'a [CatalogRow], id: &str) -> &'a CatalogRow {
        rows.iter()
            .find(|row| row.id() == id)
            .expect("the fixture should contain the named id")
    }

    fn ids_of<'a>(rows: &[&'a CatalogRow]) -> Vec<&'a str> {
        rows.iter().map(|row| row.id()).collect()
    }

    fn remove_temp(path: &std::path::Path) {
        match fs::remove_dir_all(path) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => panic!(
                "the temp directory {} should be removable: {err}",
                path.display()
            ),
        }
    }

    fn temp_cache(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "grok-router-catalog-{label}-{}",
            std::process::id()
        ));
        remove_temp(&path);
        fs::create_dir_all(&path).expect("the temp cache directory should be created");
        path
    }

    #[test]
    fn and_match_keeps_a_row_only_when_every_term_hits_id_name_or_description() {
        let rows = rows();
        let both = matching(&rows, &["turbo", "pictures"]);
        assert_eq!(
            both.iter().map(|row| row.id()).collect::<Vec<_>>(),
            ["gpt-4-turbo"],
            "turbo is in the id and pictures is in the description, so both terms hit that row"
        );

        let split = matching(&rows, &["turbo", "older"]);
        assert!(
            split.is_empty(),
            "turbo and older live on different rows, so AND must exclude both; matched {split:?}"
        );

        let case = matching(&rows, &["OLDER"]);
        assert_eq!(
            case.iter().map(|row| row.id()).collect::<Vec<_>>(),
            ["gpt-4"],
            "term matching is case-insensitive against the description"
        );
    }

    #[test]
    fn a_term_matches_when_it_appears_in_only_one_field() {
        let rows = rows();
        let by_id = matching(&rows, &["meta/llama"]);
        let by_description = matching(&rows, &["flagship"]);
        let by_name = matching(&rows, &["GPT-4 Turbo"]);
        assert_eq!(ids_of(&by_id), ["meta/llama"]);
        assert_eq!(ids_of(&by_description), ["gpt-4"]);
        assert_eq!(ids_of(&by_name), ["gpt-4-turbo"]);
    }

    #[test]
    fn matching_preserves_catalog_order() {
        let rows = rows();
        let ids: Vec<_> = matching(&rows, &["gpt"])
            .into_iter()
            .map(CatalogRow::id)
            .collect();
        assert_eq!(ids, ["gpt-4", "gpt-4-turbo"]);
    }

    #[test]
    fn resolve_accepts_one_exact_id_even_when_that_text_also_appears_in_another_row() {
        let rows = rows();
        let resolved = resolve(&rows, &["GPT-4"]).expect("GPT-4 should be the exact id gpt-4");
        assert_eq!(resolved.id(), "gpt-4");
    }

    #[test]
    fn resolve_accepts_one_unique_term_match() {
        let rows = rows();
        let resolved = resolve(&rows, &["llama"]).expect("llama should match one row");
        assert_eq!(resolved.id(), "meta/llama");
    }

    #[test]
    fn resolve_rejects_zero_matches_and_names_the_count_and_the_next_step() {
        let rows = rows();
        let error = resolve(&rows, &["missing-model"]).expect_err("nothing should match");
        assert_eq!(error.count(), 0);
        let message = error.to_string();
        assert!(
            message.contains("0 matches"),
            "the error should name the count; message: {message}"
        );
        assert!(
            message.contains("fewer terms"),
            "the error should name the next step; message: {message}"
        );
    }

    #[test]
    fn resolve_rejects_many_matches_and_names_the_count_and_the_next_step() {
        let rows = rows();
        let error = resolve(&rows, &["gpt"]).expect_err("gpt should match two rows");
        assert_eq!(error.count(), 2);
        let message = error.to_string();
        assert!(
            message.contains("2 matches"),
            "the error should name the count; message: {message}"
        );
        assert!(
            message.contains("exact provider id"),
            "the error should name the next step; message: {message}"
        );
    }

    #[test]
    fn resolve_rejects_a_duplicated_exact_id() {
        let mut rows = rows();
        rows.push(CatalogRow {
            id: "gpt-4".to_owned(),
            name: "Second".to_owned(),
            description: String::new(),
            context_length: Some(1),
            sees_image: false,
        });
        let error = resolve(&rows, &["gpt-4"]).expect_err("two exact ids should not resolve");
        assert_eq!(error.count(), 2);
        let message = error.to_string();
        assert!(
            message.contains("2 matches"),
            "the duplicate id error should name the count; message: {message}"
        );
        assert!(
            message.contains("more terms"),
            "the duplicate id error should name the next step; message: {message}"
        );
    }

    #[test]
    fn parsed_row_keeps_context_length_and_image_modality_for_the_edit_model() {
        let rows = rows();
        let turbo = row(&rows, "gpt-4-turbo");
        assert!(
            turbo.sees_image(),
            "Image in input_modalities should count, regardless of case"
        );
        assert_eq!(turbo.context_length(), Some(128000));
        let model = turbo.as_catalog_model();
        assert_eq!(model.id, "gpt-4-turbo");
        assert_eq!(model.name, Some("GPT-4 Turbo"));
        assert_eq!(model.context_window, Some(128000));
        assert!(model.sees_image);

        let plain = row(&rows, "gpt-4");
        assert!(
            !plain.sees_image(),
            "a text modality must not set sees_image, even if other text mentions pictures"
        );
        assert_eq!(plain.context_length(), Some(8192));

        let unnamed = row(&rows, "meta/llama");
        assert_eq!(unnamed.name(), "");
        assert_eq!(unnamed.display_name(), "meta/llama");
        assert_eq!(unnamed.context_length(), None);
        assert!(!unnamed.sees_image());
        assert_eq!(unnamed.as_catalog_model().name, None);
    }

    #[test]
    fn a_description_that_says_image_does_not_set_vision() {
        let bytes = br#"{
            "data": [{
                "id": "acme/text",
                "name": "Text",
                "description": "not an image model",
                "context_length": 10,
                "architecture": { "input_modalities": ["text"] }
            }]
        }"#;
        let rows = parse_catalog(bytes).expect("the text-only fixture should parse");
        let only = rows
            .first()
            .expect("the fixture should contain one row");
        assert!(!only.sees_image());
    }

    #[test]
    fn parse_rejects_a_document_without_a_data_array() {
        let error = parse_catalog(br#"{"models":[]}"#).expect_err("a missing data array should fail");
        let message = error.to_string();
        assert!(
            message.contains("data array"),
            "the error should name the missing shape; message: {message}"
        );
        assert!(
            message.contains("--catalog-file"),
            "the error should name the next step; message: {message}"
        );
    }

    #[test]
    fn parse_rejects_a_row_without_an_id_and_names_the_index() {
        let error = parse_catalog(br#"{"data":[{}]}"#).expect_err("a row without an id should fail");
        let message = error.to_string();
        assert!(
            message.contains("data[0]"),
            "the error should name the row index; message: {message}"
        );
        assert!(
            message.contains("no id"),
            "the error should say the id is missing; message: {message}"
        );
    }

    #[test]
    fn models_request_is_a_get_with_a_bearer_header_and_does_not_call_the_network() {
        let request = catalog_request("test-key", &models_url(COMPILED_BASE))
            .expect("a non-empty key should build a request");
        assert_eq!(request.method(), "GET");
        assert_eq!(request.url(), "https://openrouter.ai/api/v1/models");
        assert_eq!(request.authorization(), "Bearer test-key");
        let rendered = format!("{request:?}");
        assert!(
            !rendered.contains("test-key"),
            "debug output must not include the key; rendered: {rendered}"
        );
    }

    #[test]
    fn an_empty_key_refuses_the_request_and_names_the_next_step() {
        let error = catalog_request("  ", MODELS_URL)
            .expect_err("a blank key must not build a request");
        assert_eq!(error, CatalogError::MissingKey);
        let message = error.to_string();
        assert!(
            message.contains("OPENROUTER_API_KEY"),
            "the error should name the missing key; message: {message}"
        );
        assert!(
            message.contains("--catalog-file"),
            "the error should name the next step; message: {message}"
        );
        assert!(
            message.contains("empty cache was not written"),
            "the missing-key path must say it wrote nothing; message: {message}"
        );
    }

    #[test]
    fn a_cache_inside_one_hour_is_used() {
        assert_eq!(
            cache_choice(Duration::from_secs(60 * 60 - 1), false),
            CacheChoice::Use
        );
    }

    #[test]
    fn a_cache_at_or_older_than_one_hour_is_not_the_answer() {
        assert_eq!(
            cache_choice(CACHE_FRESH, false),
            CacheChoice::Fetch,
            "exactly one hour is not inside the fresh window"
        );
        assert_eq!(
            cache_choice(CACHE_FRESH + Duration::from_secs(1), false),
            CacheChoice::Fetch
        );
    }

    #[test]
    fn refresh_ignores_a_fresh_cache() {
        assert_eq!(
            cache_choice(Duration::from_secs(1), true),
            CacheChoice::Fetch
        );
    }

    #[test]
    fn prepare_uses_a_fresh_cache_without_a_key_and_does_not_write() {
        let dir = temp_cache("fresh");
        let path = cache_file(&dir);
        fs::write(&path, FIXTURE).expect("the fresh cache fixture should be written");
        let modified = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        fs::File::options()
            .write(true)
            .open(&path)
            .expect("the cache file should open")
            .set_modified(modified)
            .expect("the test should set the cache mtime");
        let before = fs::read(&path).expect("the cache fixture should still be readable");
        let now = modified + Duration::from_secs(30 * 60);
        let prepared = prepare_cached_load(&dir, now, false, "", COMPILED_BASE)
            .expect("a fresh cache should load without an API key");
        match prepared {
            CachedOrFetch::Cached(rows) => assert_eq!(rows.len(), 3),
            CachedOrFetch::Fetch(request) => {
                panic!("a fresh cache must not build a download; request {request:?}")
            }
        }
        let after = fs::read(&path).expect("prepare should leave the cache readable");
        assert_eq!(before, after, "prepare must not rewrite a fresh cache");
        remove_temp(&dir);
    }

    #[test]
    fn prepare_refetches_when_the_cache_is_older_than_one_hour() {
        let dir = temp_cache("stale");
        let path = cache_file(&dir);
        fs::write(&path, FIXTURE).expect("the stale cache fixture should be written");
        let modified = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        fs::File::options()
            .write(true)
            .open(&path)
            .expect("the cache file should open")
            .set_modified(modified)
            .expect("the test should set the cache mtime");
        let now = modified + CACHE_FRESH;
        let prepared = prepare_cached_load(&dir, now, false, "test-key", COMPILED_BASE)
            .expect("a stale cache with a key should plan a download");
        match prepared {
            CachedOrFetch::Fetch(request) => {
                assert_eq!(request.method(), "GET");
                assert_eq!(request.url(), "https://openrouter.ai/api/v1/models");
                assert_eq!(request.authorization(), "Bearer test-key");
            }
            CachedOrFetch::Cached(_) => panic!("a one-hour-old cache must not be the answer"),
        }
        remove_temp(&dir);
    }

    #[test]
    fn prepare_refetches_a_fresh_cache_when_refresh_is_set() {
        let dir = temp_cache("refresh");
        let path = cache_file(&dir);
        fs::write(&path, FIXTURE).expect("the cache fixture should be written");
        let modified = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        fs::File::options()
            .write(true)
            .open(&path)
            .expect("the cache file should open")
            .set_modified(modified)
            .expect("the test should set the cache mtime");
        let now = modified + Duration::from_secs(1);
        let prepared = prepare_cached_load(&dir, now, true, "test-key", COMPILED_BASE)
            .expect("refresh with a key should plan a download");
        assert!(
            matches!(prepared, CachedOrFetch::Fetch(_)),
            "refresh must ignore a fresh cache"
        );
        remove_temp(&dir);
    }

    #[test]
    fn a_stale_cache_without_a_key_fails_and_writes_nothing() {
        let dir = temp_cache("nokey");
        let path = cache_file(&dir);
        fs::write(&path, FIXTURE).expect("the cache fixture should be written");
        let before = fs::read(&path).expect("the cache fixture should be readable");
        let modified = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        fs::File::options()
            .write(true)
            .open(&path)
            .expect("the cache file should open")
            .set_modified(modified)
            .expect("the test should set the cache mtime");
        let now = modified + CACHE_FRESH + Duration::from_secs(5);
        let error = prepare_cached_load(&dir, now, false, "", COMPILED_BASE)
            .expect_err("a stale cache without a key must not download");
        assert_eq!(error, CatalogError::MissingKey);
        let after = fs::read(&path).expect("the stale cache should still exist");
        assert_eq!(before, after, "a missing key must not rewrite the cache");
        remove_temp(&dir);
    }

    #[test]
    fn a_missing_cache_without_a_key_does_not_create_a_file() {
        let dir = temp_cache("absent");
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        let error = prepare_cached_load(&dir, now, false, "", COMPILED_BASE)
            .expect_err("a missing cache without a key must fail");
        assert_eq!(error, CatalogError::MissingKey);
        assert!(
            !cache_file(&dir).exists(),
            "a missing key must not create an empty cache"
        );
        remove_temp(&dir);
    }

    #[test]
    fn empty_cache_bytes_are_not_written() {
        let dir = temp_cache("empty");
        let error = write_cache(&dir, COMPILED_BASE, b"")
            .expect_err("an empty body must not be stored");
        assert_eq!(error, CatalogError::EmptyCacheWrite);
        assert!(
            !cache_file(&dir).exists(),
            "refusing an empty cache must not create models.json"
        );
        assert!(
            !dir.join(".models.json.tmp").exists(),
            "refusing an empty cache must not create a temporary file"
        );
        remove_temp(&dir);
    }

    #[test]
    fn write_cache_stores_bytes_only_under_the_cache_directory() {
        let dir = temp_cache("store");
        let written = write_cache(&dir, COMPILED_BASE, FIXTURE.as_bytes())
            .expect("a non-empty catalog should be stored");
        assert_eq!(written, cache_file(&dir));
        let stored = fs::read(&written).expect("the stored cache should be readable");
        assert_eq!(stored, FIXTURE.as_bytes());
        assert!(
            !dir.join(".models.json.tmp").exists(),
            "the temporary file should be renamed into place"
        );
        remove_temp(&dir);
    }

    #[test]
    fn models_url_drops_one_trailing_slash_and_does_not_trim() {
        assert_eq!(
            models_url("https://example.invalid/v1"),
            "https://example.invalid/v1/models",
            "a base with no trailing slash gains exactly /models"
        );
        assert_eq!(
            models_url("https://example.invalid/v1/"),
            "https://example.invalid/v1/models",
            "one trailing slash is removed so the result is not //models"
        );
        assert_eq!(
            models_url("https://example.invalid/v1//"),
            "https://example.invalid/v1//models",
            "a second trailing slash stays"
        );
        assert_eq!(
            models_url(" https://example.invalid/v1 "),
            " https://example.invalid/v1 /models",
            "surrounding spaces stay"
        );
        assert_eq!(
            models_url(COMPILED_BASE),
            "https://openrouter.ai/api/v1/models",
            "the compiled base still derives to the OpenRouter models URL"
        );
    }

    #[test]
    fn the_default_models_url_caches_as_models_json() {
        let dir = Path::new("cache");
        let url = models_url(COMPILED_BASE);
        assert_eq!(url, MODELS_URL);
        assert_eq!(cache_file_for(dir, &url), cache_file(dir));
        assert_eq!(
            cache_file_for(dir, MODELS_URL)
                .file_name()
                .and_then(|name| name.to_str()),
            Some(CACHE_FILE_NAME)
        );
    }

    #[test]
    fn another_models_url_uses_hex_and_never_models_json() {
        let dir = Path::new("cache");
        let url = models_url("https://example.invalid/v1");
        assert_eq!(models_url("https://example.invalid/v1/"), url);
        let path = cache_file_for(dir, &url);
        let hex = lowercase_hex(&url);
        assert_eq!(
            path,
            dir.join(format!("models-{hex}.json")),
            "the hex name is the lowercase hex of the models URL bytes"
        );
        assert_ne!(path, cache_file(dir));
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .expect("the cache file name should be Unicode");
        assert_ne!(name, "models.json");
        assert_eq!(
            cache_file_for(dir, &models_url("https://example.invalid/v1/")),
            path,
            "one trailing slash on the base does not get a second cache file"
        );
    }

    #[test]
    fn a_custom_base_reads_its_cache_and_does_not_read_models_json() {
        let dir = temp_cache("custom-read");
        let url = models_url("https://example.invalid/v1");
        let custom = cache_file_for(&dir, &url);
        let decoy = cache_file(&dir);
        let decoy_bytes = br#"{"data":[{"id":"decoy/model","name":"Decoy"}]}"#;
        fs::write(&custom, FIXTURE).expect("the custom cache fixture should be written");
        fs::write(&decoy, decoy_bytes).expect("the models.json decoy should be written");
        let modified = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        set_cache_mtime(&custom, modified);
        set_cache_mtime(&decoy, modified);
        let now = modified + Duration::from_secs(30 * 60);
        let prepared = prepare_cached_load(&dir, now, false, "", "https://example.invalid/v1/")
            .expect("a fresh cache for the derived custom URL should load");
        match prepared {
            CachedOrFetch::Cached(rows) => {
                assert!(
                    rows.iter().any(|row| row.id() == "gpt-4"),
                    "the custom cache should supply its own rows"
                );
                assert!(
                    rows.iter().all(|row| row.id() != "decoy/model"),
                    "models.json must not answer a different models URL"
                );
            }
            CachedOrFetch::Fetch(request) => {
                panic!(
                    "a fresh custom cache must not build a download; request url {}",
                    request.url()
                );
            }
        }
        let after = fs::read(&decoy).expect("models.json should still be readable");
        assert_eq!(after, decoy_bytes, "prepare must not rewrite models.json");
        remove_temp(&dir);
    }

    #[test]
    fn a_custom_base_writes_its_cache_and_does_not_write_models_json() {
        let dir = temp_cache("custom-write");
        let base = "https://example.invalid/v1/";
        let written = write_cache(&dir, base, FIXTURE.as_bytes())
            .expect("a non-empty catalog for a custom base should be stored");
        let url = models_url(base);
        let expected = cache_file_for(&dir, &url);
        assert_eq!(written, expected);
        assert_ne!(written, cache_file(&dir));
        assert!(
            !cache_file(&dir).exists(),
            "a custom base must not create models.json"
        );
        let stored = fs::read(&written).expect("the hex cache should be readable");
        assert_eq!(stored, FIXTURE.as_bytes());
        let name = written
            .file_name()
            .and_then(|file_name| file_name.to_str())
            .expect("the cache file name should be Unicode");
        assert_eq!(name, format!("models-{}.json", lowercase_hex(&url)));
        assert_ne!(name, "models.json");
        assert!(
            !dir.join(format!(".{name}.tmp")).exists(),
            "the temporary file should be renamed into place and must not stay named models.json"
        );
        remove_temp(&dir);
    }

    #[test]
    fn a_blank_models_url_does_not_build_a_request() {
        for url in ["", " ", "\t", " \n"] {
            let error = catalog_request("test-key", url)
                .expect_err("a blank models URL must not build a request");
            assert_eq!(error, CatalogError::EmptyUrl, "url {url:?}");
            let message = error.to_string();
            assert!(
                message.contains("models URL"),
                "the error should name the models URL; url {url:?}; message: {message}"
            );
            assert!(
                message.contains("Nothing was downloaded"),
                "the error should say nothing was downloaded; url {url:?}; message: {message}"
            );
            assert!(
                message.contains("cache was not written"),
                "the error should say the cache was not written; url {url:?}; message: {message}"
            );
        }
    }

    #[test]
    fn catalog_request_returns_the_models_url_it_stored() {
        let url = models_url("https://example.invalid/v1");
        let request = catalog_request("test-key", &url)
            .expect("a non-blank key and URL should build a request");
        assert_eq!(request.method(), "GET");
        assert_eq!(request.url(), url);
        assert_eq!(request.url(), "https://example.invalid/v1/models");
    }

    const DIRECT_FIXTURE: &str = r#"{
        "object": "list",
        "data": [
            {
                "id": "acme-vision",
                "object": "model",
                "owned_by": "acme",
                "name": "Acme Vision",
                "description": "marker-not-read",
                "context_window": 32768,
                "context_length": 1,
                "max_output_tokens": 8192,
                "input_modalities": ["text", "Image"],
                "output_modalities": ["text"],
                "architecture": { "input_modalities": ["text"] },
                "effort": "high",
                "api_capabilities": ["chat"]
            },
            {
                "id": "acme-text",
                "name": "   ",
                "context_window": 4096,
                "input_modalities": ["text"]
            },
            {
                "id": "acme-windowless",
                "context_length": 128000,
                "architecture": { "input_modalities": ["image"] }
            },
            {
                "id": "acme-zero",
                "name": "",
                "context_window": 0
            },
            {
                "id": "acme-null",
                "context_window": null,
                "input_modalities": null
            }
        ]
    }"#;

    fn selected(catalog_id: &str) -> Cartridge {
        cartridge(Some(catalog_id)).expect("the named catalog id should select a cartridge")
    }

    fn message_has_no_openrouter_request(error: &CatalogError) -> String {
        let message = error.to_string();
        assert!(
            !message.to_ascii_lowercase().contains("openrouter"),
            "a direct catalog error must not ask for an OpenRouter document; message: {message}"
        );
        message
    }

    #[test]
    fn cartridge_selects_only_the_two_catalog_ids() {
        assert_eq!(
            selected("openrouter-models"),
            Cartridge::OpenRouter,
            "openrouter-models is the OpenRouter cartridge"
        );
        assert_eq!(
            selected("deepseek-list-models"),
            Cartridge::DeepSeek,
            "deepseek-list-models is the direct cartridge"
        );
    }

    #[test]
    fn cartridge_refuses_a_missing_blank_or_unknown_id_and_does_not_select_openrouter() {
        let missing = cartridge(None).expect_err("a missing catalog id should refuse");
        assert_eq!(missing, CartridgeError::Missing);
        assert!(
            missing.to_string().contains("missing"),
            "the missing-id error should say the id is missing; message: {missing}"
        );

        let blank = cartridge(Some("")).expect_err("a blank catalog id should refuse");
        assert_eq!(blank, CartridgeError::Missing);

        let unknown =
            cartridge(Some("not-a-cartridge")).expect_err("an unknown catalog id should refuse");
        assert_eq!(
            unknown,
            CartridgeError::Unknown {
                id: "not-a-cartridge".to_owned(),
            }
        );
        assert!(
            !matches!(
                cartridge(Some("not-a-cartridge")),
                Ok(Cartridge::OpenRouter)
            ),
            "not-a-cartridge must not select openrouter-models"
        );
        let message = unknown.to_string();
        assert!(
            message.contains("not-a-cartridge"),
            "the unknown-id error should name that id; message: {message}"
        );
        assert!(
            message.contains("not selected"),
            "the unknown-id error should say OpenRouter was not selected; message: {message}"
        );
    }

    #[test]
    fn direct_document_keeps_id_name_window_and_image_and_ignores_openrouter_fields() {
        let rows = selected("deepseek-list-models")
            .parse(DIRECT_FIXTURE.as_bytes())
            .expect("the direct fixture should parse");
        assert_eq!(rows.len(), 5, "a window-less row is still a row");

        let vision = row(&rows, "acme-vision");
        assert_eq!(vision.name(), "Acme Vision");
        assert_eq!(vision.display_name(), "Acme Vision");
        assert_eq!(vision.description(), "");
        assert_eq!(vision.context_length(), Some(32768));
        assert!(
            vision.sees_image(),
            "Image in input_modalities should count, regardless of case"
        );
        assert_eq!(vision.as_catalog_model().context_window, Some(32768));
        assert!(vision.as_catalog_model().sees_image);

        let text = row(&rows, "acme-text");
        assert_eq!(text.name(), "");
        assert_eq!(text.display_name(), "acme-text");
        assert_eq!(text.as_catalog_model().name, None);
        assert_eq!(text.context_length(), Some(4096));
        assert!(!text.sees_image());

        let windowless = row(&rows, "acme-windowless");
        assert!(
            windowless.context_length().is_none_or(|window| window <= 0),
            "context_length without context_window is not a positive window; got {:?}",
            windowless.context_length()
        );
        assert!(
            !windowless.sees_image(),
            "architecture.input_modalities must not set sees_image"
        );

        let zero = row(&rows, "acme-zero");
        assert_eq!(zero.name(), "");
        assert_eq!(
            zero.context_length(),
            Some(0),
            "zero stays a non-positive window"
        );
        assert!(zero.context_length().is_none_or(|window| window <= 0));

        let absent = row(&rows, "acme-null");
        assert_eq!(absent.context_length(), None);
        assert!(!absent.sees_image());
    }

    #[test]
    fn openrouter_cartridge_ignores_context_window_and_top_level_input_modalities() {
        let bytes = br#"{
            "data": [{
                "id": "acme-vision",
                "name": "Acme Vision",
                "context_window": 32768,
                "input_modalities": ["image"]
            }]
        }"#;
        let openrouter = selected("openrouter-models");
        let rows = openrouter
            .parse(bytes)
            .expect("a data array should still parse as OpenRouter");
        let direct = parse_catalog(bytes).expect("parse_catalog should accept the same bytes");
        assert_eq!(
            rows, direct,
            "the OpenRouter cartridge should return parse_catalog rows"
        );
        let only = rows.first().expect("the document should contain one row");
        assert_eq!(only.id(), "acme-vision");
        assert!(
            only.context_length().is_none_or(|window| window <= 0),
            "context_window must not become a positive OpenRouter window; got {:?}",
            only.context_length()
        );
        assert!(
            !only.sees_image(),
            "top-level input_modalities must not set sees_image"
        );
    }

    #[test]
    fn deepseek_cartridge_ignores_context_length_and_architecture_on_an_openrouter_document() {
        let rows = selected("deepseek-list-models")
            .parse(FIXTURE.as_bytes())
            .expect("an OpenRouter data array should parse as a direct catalog");
        let turbo = row(&rows, "gpt-4-turbo");
        assert_eq!(turbo.id(), "gpt-4-turbo");
        assert_eq!(turbo.name(), "GPT-4 Turbo");
        assert_eq!(
            turbo.description(),
            "",
            "description is not a direct-catalog field"
        );
        assert!(
            turbo.context_length().is_none_or(|window| window <= 0),
            "context_length 128000 must not become a positive window; got {:?}",
            turbo.context_length()
        );
        assert!(
            !turbo.sees_image(),
            "architecture.input_modalities must not set sees_image"
        );
    }

    #[test]
    fn a_non_integer_context_window_fails_that_direct_row() {
        let parser = selected("deepseek-list-models");
        let bytes = br#"{
            "data": [
                {"id": "acme-ok", "context_window": 8},
                {"id": "acme-bad", "context_window": "32768"}
            ]
        }"#;
        let error = parser
            .parse(bytes)
            .expect_err("a string context_window should fail that row");
        let message = message_has_no_openrouter_request(&error);
        assert!(
            message.contains("data[1]"),
            "the error should name the failing row; message: {message}"
        );
        assert!(
            message.contains("context_window"),
            "the error should name context_window; message: {message}"
        );

        let fraction = parser
            .parse(br#"{"data":[{"id":"acme","context_window":1.5}]}"#)
            .expect_err("a fractional context_window should fail that row");
        let fraction_message = message_has_no_openrouter_request(&fraction);
        assert!(
            fraction_message.contains("not an integer"),
            "a fraction is not an integer window; message: {fraction_message}"
        );
    }

    #[test]
    fn direct_catalog_errors_do_not_ask_for_an_openrouter_document() {
        let parser = selected("deepseek-list-models");
        let cases: [(&[u8], &str); 3] = [
            (b"{", "could not be parsed"),
            (br#"{"models":[]}"#, "data array"),
            (br#"{"data":[{}]}"#, "no id"),
        ];
        for (bytes, needle) in cases {
            let error = parser
                .parse(bytes)
                .expect_err("this direct document should fail");
            let message = message_has_no_openrouter_request(&error);
            assert!(
                message.contains(needle),
                "the error should name the failure; needle {needle}; message: {message}"
            );
        }
    }

    #[test]
    fn openrouter_slug_derives_or_leaf_unless_as_is_present() {
        let openrouter = selected("openrouter-models");
        assert_eq!(
            openrouter
                .slug("z-ai/glm-5.3", None)
                .expect("an OpenRouter id should derive a slug"),
            "or-glm-5-3"
        );
        assert_eq!(
            openrouter
                .slug("z-ai/glm-5.3", Some("chosen"))
                .expect("a grammatical --as slug should be kept"),
            "chosen"
        );
        let error = openrouter
            .slug("z-ai/glm-5.3", Some("bad slug"))
            .expect_err("a space is not a slug");
        assert!(
            matches!(error, RefuseReason::InvalidSlug { ref slug } if slug == "bad slug"),
            "an explicit slug should be refused as itself; got {error}"
        );
    }

    #[test]
    fn deepseek_slug_uses_the_id_and_a_slash_requires_as_without_a_leaf() {
        let deepseek = selected("deepseek-list-models");
        assert_eq!(
            deepseek
                .slug("deepseek-v4-pro", None)
                .expect("a grammatical id should be the slug"),
            "deepseek-v4-pro"
        );
        assert_eq!(
            deepseek
                .slug("AcmeText", None)
                .expect("letters should be a slug and should keep their case"),
            "AcmeText"
        );
        assert_eq!(
            deepseek
                .slug("vendor/model", Some("direct-slug"))
                .expect("--as should replace a slash id"),
            "direct-slug"
        );
        for id in ["vendor/model", "z-ai/glm-5.3", "has space", "-leading", ""] {
            let error = deepseek
                .slug(id, None)
                .expect_err("this id is not a slug and must not grow a leaf");
            assert!(
                matches!(error, RefuseReason::InvalidSlug { ref slug } if slug == id),
                "id {id:?} should be InvalidSlug of itself, not a derived leaf; got {error}"
            );
        }
        let explicit = deepseek
            .slug("deepseek-v4-pro", Some("bad slug"))
            .expect_err("a space in --as is not a slug");
        assert!(
            matches!(explicit, RefuseReason::InvalidSlug { ref slug } if slug == "bad slug"),
            "--as should be refused as itself; got {explicit}"
        );
    }

    #[test]
    fn deepseek_base_url_does_not_cache_as_models_json() {
        let dir = Path::new("cache");
        let with_slash = models_url("https://api.deepseek.com/");
        let without_slash = models_url("https://api.deepseek.com");
        assert_eq!(
            with_slash, without_slash,
            "one trailing slash is the same models URL"
        );
        let path = cache_file_for(dir, &without_slash);
        let name = path
            .file_name()
            .and_then(|file_name| file_name.to_str())
            .expect("the cache file name should be Unicode");
        let computed = format!("models-{}.json", lowercase_hex(&without_slash));
        assert_eq!(
            name, computed,
            "the other cache name is computed from the URL bytes and is not a hardcoded hex"
        );
        assert_ne!(name, "models.json");
        assert_ne!(path, cache_file(dir));
        let openrouter = models_url(COMPILED_BASE);
        assert_eq!(openrouter, MODELS_URL);
        assert_eq!(
            cache_file_for(dir, &openrouter),
            cache_file(dir),
            "the OpenRouter base caches as models.json because the URL equals MODELS_URL"
        );
    }

    #[test]
    fn prepare_cached_body_returns_fresh_bytes_without_parsing() {
        let dir = temp_cache("body");
        let base = "https://api.deepseek.com";
        let path = cache_file_for(&dir, &models_url("https://api.deepseek.com/"));
        let bytes = b"not-json-but-fresh";
        fs::write(&path, bytes).expect("the fresh body fixture should be written");
        let modified = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        set_cache_mtime(&path, modified);
        let now = modified + Duration::from_secs(30 * 60);
        let prepared = prepare_cached_body(&dir, now, false, "", base)
            .expect("a fresh cache body should load without an API key");
        match prepared {
            CachedBodyOrFetch::Cached(got) => assert_eq!(got, bytes),
            CachedBodyOrFetch::Fetch(request) => {
                panic!(
                    "a fresh cache body must not build a download; request url {}",
                    request.url()
                );
            }
        }
        assert!(
            !cache_file(&dir).exists(),
            "a DeepSeek base must not read or create models.json"
        );
        let load = prepare_cached_load(&dir, now, false, "", base)
            .expect_err("the same fresh bytes are not catalog JSON, so the parser should refuse");
        assert!(
            matches!(load, CatalogError::CacheInvalid { .. }),
            "prepare_cached_load still parses; got {load}"
        );
        remove_temp(&dir);
    }

    #[test]
    fn prepare_cached_body_plans_the_same_fetch_when_the_cache_is_stale() {
        let dir = temp_cache("body-stale");
        let base = "https://api.deepseek.com/";
        let url = models_url(base);
        let path = cache_file_for(&dir, &url);
        fs::write(&path, b"stale").expect("the stale body fixture should be written");
        let modified = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        set_cache_mtime(&path, modified);
        let now = modified + CACHE_FRESH;
        let prepared = prepare_cached_body(&dir, now, false, "test-key", base)
            .expect("a stale cache with a key should plan a download");
        match prepared {
            CachedBodyOrFetch::Fetch(request) => {
                assert_eq!(request.method(), "GET");
                assert_eq!(request.url(), url);
                assert_eq!(request.authorization(), "Bearer test-key");
                assert_ne!(
                    cache_file_for(&dir, request.url()),
                    cache_file(&dir),
                    "the fetch URL must not be the models.json cache"
                );
            }
            CachedBodyOrFetch::Cached(bytes) => {
                panic!("a one-hour-old body must not be the answer; bytes {bytes:?}")
            }
        }
        remove_temp(&dir);
    }

    fn lowercase_hex(text: &str) -> String {
        let mut hex = String::new();
        for byte in text.as_bytes() {
            hex.push_str(&format!("{byte:02x}"));
        }
        hex
    }

    fn set_cache_mtime(path: &Path, modified: SystemTime) {
        fs::File::options()
            .write(true)
            .open(path)
            .expect("the cache file should open")
            .set_modified(modified)
            .expect("the test should set the cache mtime");
    }
}
