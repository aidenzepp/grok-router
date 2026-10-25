//! Published scores and price from an `OpenRouter` models document.
//!
//! This is not a third catalog cartridge and it does not run prompts. The
//! `openrouter-models` cartridge reads `benchmarks.artificial_analysis` and
//! `pricing` from the same `/models` body search already downloads. Design
//! Arena categories are ignored. A negative price is `OpenRouter`'s unpublished
//! sentinel and becomes a blank, not a negative dollar amount.

use serde_json::{Map, Number, Value};

/// Why published scores could not be read. Nothing was written.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BenchmarkParseError {
    /// The bytes were not JSON.
    #[error(
        "the catalog JSON could not be parsed: {detail}. Pass an OpenRouter /api/v1/models document via --catalog-file, or pass --refresh."
    )]
    Json {
        /// Parser detail.
        detail: String,
    },
    /// The JSON was not an `OpenRouter` models document.
    #[error("{detail}. Pass an OpenRouter /api/v1/models document via --catalog-file.")]
    Shape {
        /// What was missing.
        detail: String,
    },
    /// One `data` element could not be turned into a benchmark row.
    #[error("catalog data[{index}] {detail}. Fix that element or remove it from the file.")]
    Row {
        /// Index in the `data` array.
        index: usize,
        /// What was wrong with that element.
        detail: String,
    },
    /// This cartridge has no published scores. The bytes were not parsed.
    #[error(
        "{catalog_id} has no benchmark feed. Nothing was parsed. Use a provider whose catalog is openrouter-models."
    )]
    NoFeed {
        /// Catalog id that refused.
        catalog_id: &'static str,
    },
}

/// One model row with price per million tokens and Artificial Analysis indexes.
///
/// A missing score or an unpublished price is `None`. The id, name, and
/// description stay so a term can match a row that has no score.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BenchmarkRow {
    /// Provider id, such as `moonshotai/kimi-k3.1-pro`.
    id: String,
    /// Catalog display name. Empty when the document omitted it.
    name: String,
    /// Catalog description. Empty when the document omitted it.
    description: String,
    /// Prompt price in dollars per million tokens.
    prompt_per_million: Option<String>,
    /// Completion price in dollars per million tokens.
    completion_per_million: Option<String>,
    /// `artificial_analysis.intelligence_index`.
    intelligence: Option<Score>,
    /// `artificial_analysis.coding_index`.
    coding: Option<Score>,
    /// `artificial_analysis.agentic_index`.
    agentic: Option<Score>,
}

/// One finite JSON number from an Artificial Analysis index.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Score {
    /// The number as the document wrote it.
    number: Number,
}

impl BenchmarkRow {
    /// Provider id.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
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

    /// Prompt price in dollars per million tokens, when the document published one.
    #[must_use]
    pub fn prompt_per_million(&self) -> Option<&str> {
        self.prompt_per_million.as_deref()
    }

    /// Completion price in dollars per million tokens, when the document published one.
    #[must_use]
    pub fn completion_per_million(&self) -> Option<&str> {
        self.completion_per_million.as_deref()
    }

    /// Intelligence index, as the JSON number from the document.
    #[must_use]
    pub fn intelligence_index(&self) -> Option<&Number> {
        self.intelligence.as_ref().map(Score::number)
    }

    /// Coding index, as the JSON number from the document.
    #[must_use]
    pub fn coding_index(&self) -> Option<&Number> {
        self.coding.as_ref().map(Score::number)
    }

    /// Agentic index, as the JSON number from the document.
    #[must_use]
    pub fn agentic_index(&self) -> Option<&Number> {
        self.agentic.as_ref().map(Score::number)
    }

    /// Text of the intelligence index, or `-` when the document omitted it.
    #[must_use]
    pub fn intelligence_text(&self) -> String {
        score_text(self.intelligence.as_ref())
    }

    /// Text of the coding index, or `-` when the document omitted it.
    #[must_use]
    pub fn coding_text(&self) -> String {
        score_text(self.coding.as_ref())
    }

    /// Text of the agentic index, or `-` when the document omitted it.
    #[must_use]
    pub fn agentic_text(&self) -> String {
        score_text(self.agentic.as_ref())
    }

    /// Finite intelligence index, used only to order the default view.
    fn intelligence_rank(&self) -> Option<f64> {
        self.intelligence.as_ref().and_then(Score::rank)
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

impl Score {
    /// Keep `number` when it is a finite float.
    fn parse(number: &Number) -> Option<Self> {
        let rank = number.as_f64()?;
        if !rank.is_finite() {
            return None;
        }
        Some(Self {
            number: number.clone(),
        })
    }

    /// The document's number.
    fn number(&self) -> &Number {
        &self.number
    }

    /// Finite rank. `None` means the number no longer converts, which parse already rejected.
    fn rank(&self) -> Option<f64> {
        self.number.as_f64().filter(|value| value.is_finite())
    }

    /// The document's spelling of this number.
    fn text(&self) -> String {
        self.number.to_string()
    }
}

/// `-` when `score` is absent, otherwise the document's number text.
fn score_text(score: Option<&Score>) -> String {
    match score {
        Some(score) => score.text(),
        None => "-".to_owned(),
    }
}

/// Rows in which every term appears in the id, the name, or the description.
///
/// Matching is case-insensitive. An empty term list returns every row, including
/// rows with no score. Order is the catalog order.
#[must_use]
pub fn matching<'a>(rows: &'a [BenchmarkRow], terms: &[&str]) -> Vec<&'a BenchmarkRow> {
    rows.iter().filter(|row| row.matches_all(terms)).collect()
}

/// Rows that have an intelligence index, highest first.
///
/// Ties keep catalog order. Rows with no intelligence index are left out.
#[must_use]
pub fn ranked_by_intelligence(rows: &[BenchmarkRow]) -> Vec<&BenchmarkRow> {
    let mut ranked = Vec::new();
    for row in rows {
        if row.intelligence_rank().is_some() {
            ranked.push(row);
        }
    }
    ranked.sort_by(|left, right| {
        let left_rank = left.intelligence_rank().unwrap_or(f64::NEG_INFINITY);
        let right_rank = right.intelligence_rank().unwrap_or(f64::NEG_INFINITY);
        right_rank.total_cmp(&left_rank)
    });
    ranked
}

/// Parse published scores from an `OpenRouter` `/api/v1/models` document.
///
/// A row with no `benchmarks` object is kept, with blank scores, so a term can
/// still find it. `design_arena` is not read.
///
/// # Errors
///
/// Returns a [`BenchmarkParseError`] when the bytes are not a `data` array of model objects,
/// or when a present score or price has the wrong shape.
pub fn parse_openrouter(bytes: &[u8]) -> Result<Vec<BenchmarkRow>, BenchmarkParseError> {
    let value: Value = serde_json::from_slice(bytes).map_err(|err| BenchmarkParseError::Json {
        detail: err.to_string(),
    })?;
    let Some(data) = value.get("data").and_then(Value::as_array) else {
        return Err(BenchmarkParseError::Shape {
            detail: "the catalog JSON has no data array".to_owned(),
        });
    };
    let mut rows = Vec::with_capacity(data.len());
    for (index, item) in data.iter().enumerate() {
        rows.push(parse_row(index, item)?);
    }
    Ok(rows)
}

/// Parse one `data` element. Other benchmark publishers on the object are ignored.
///
/// # Errors
///
/// Returns [`BenchmarkParseError::Row`] when the element is not an object or a present field has the wrong type.
fn parse_row(index: usize, item: &Value) -> Result<BenchmarkRow, BenchmarkParseError> {
    let Some(object) = item.as_object() else {
        return Err(row_error(index, "is not an object"));
    };
    let id = required_string(object.get("id"), index, "id")?;
    if id.is_empty() {
        return Err(row_error(index, "has no id"));
    }
    let name = optional_string(object.get("name"), index, "name")?
        .unwrap_or_default()
        .trim()
        .to_owned();
    let description = optional_string(object.get("description"), index, "description")?
        .unwrap_or_default()
        .trim()
        .to_owned();
    let pricing = optional_object(object.get("pricing"), index, "pricing")?;
    let benchmarks = optional_object(object.get("benchmarks"), index, "benchmarks")?;
    let analysis = match benchmarks {
        Some(benchmarks) => optional_object(
            benchmarks.get("artificial_analysis"),
            index,
            "benchmarks.artificial_analysis",
        )?,
        None => None,
    };
    Ok(BenchmarkRow {
        id,
        name,
        description,
        prompt_per_million: price_per_million(
            pricing.and_then(|pricing| pricing.get("prompt")),
            index,
            "pricing.prompt",
        )?,
        completion_per_million: price_per_million(
            pricing.and_then(|pricing| pricing.get("completion")),
            index,
            "pricing.completion",
        )?,
        intelligence: score_field(
            field(analysis, "intelligence_index"),
            index,
            "benchmarks.artificial_analysis.intelligence_index",
        )?,
        coding: score_field(
            field(analysis, "coding_index"),
            index,
            "benchmarks.artificial_analysis.coding_index",
        )?,
        agentic: score_field(
            field(analysis, "agentic_index"),
            index,
            "benchmarks.artificial_analysis.agentic_index",
        )?,
    })
}

/// A field on `object`, or `None` when the object or the field is absent.
fn field<'a>(object: Option<&'a Map<String, Value>>, key: &str) -> Option<&'a Value> {
    object.and_then(|object| object.get(key))
}

/// Read a required string field.
///
/// # Errors
///
/// Returns [`BenchmarkParseError::Row`] when the field is missing or not a string.
fn required_string(
    value: Option<&Value>,
    index: usize,
    field_name: &str,
) -> Result<String, BenchmarkParseError> {
    match value {
        Some(Value::String(text)) => Ok(text.trim().to_owned()),
        Some(_) => Err(row_error(index, &format!("has a non-string {field_name}"))),
        None => Err(row_error(index, &format!("has no {field_name}"))),
    }
}

/// Read an optional string field. Null and absence become `None`.
///
/// # Errors
///
/// Returns [`BenchmarkParseError::Row`] when the field is present and is not a string or null.
fn optional_string(
    value: Option<&Value>,
    index: usize,
    field_name: &str,
) -> Result<Option<String>, BenchmarkParseError> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => Ok(Some(text.to_owned())),
        Some(_) => Err(row_error(index, &format!("has a non-string {field_name}"))),
    }
}

/// Read an optional object. Null and absence become `None`.
///
/// # Errors
///
/// Returns [`BenchmarkParseError::Row`] when the field is present and is not an object or null.
fn optional_object<'a>(
    value: Option<&'a Value>,
    index: usize,
    field_name: &str,
) -> Result<Option<&'a Map<String, Value>>, BenchmarkParseError> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Object(object)) => Ok(Some(object)),
        Some(_) => Err(row_error(index, &format!("{field_name} is not an object"))),
    }
}

/// Read one index. Null and absence become `None`.
///
/// # Errors
///
/// Returns [`BenchmarkParseError::Row`] when the field is present and is not a finite number or null.
fn score_field(
    value: Option<&Value>,
    index: usize,
    field_name: &str,
) -> Result<Option<Score>, BenchmarkParseError> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(number)) => match Score::parse(number) {
            Some(score) => Ok(Some(score)),
            None => Err(row_error(
                index,
                &format!("{field_name} is not a finite number"),
            )),
        },
        Some(_) => Err(row_error(index, &format!("{field_name} is not a number"))),
    }
}

/// Dollars per million tokens from a per-token decimal string.
///
/// Null and absence become `None`. A negative number is the unpublished-price
/// sentinel and also becomes `None`.
///
/// # Errors
///
/// Returns [`BenchmarkParseError::Row`] when the field is present and is not a decimal string or null.
fn price_per_million(
    value: Option<&Value>,
    index: usize,
    field_name: &str,
) -> Result<Option<String>, BenchmarkParseError> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => match per_million(text) {
            Ok(price) => Ok(price),
            Err(()) => Err(row_error(
                index,
                &format!("{field_name} is not a decimal price"),
            )),
        },
        Some(_) => Err(row_error(index, &format!("{field_name} is not a string"))),
    }
}

/// A row error that names `detail`.
fn row_error(index: usize, detail: &str) -> BenchmarkParseError {
    BenchmarkParseError::Row {
        index,
        detail: detail.to_owned(),
    }
}

/// Multiply a per-token decimal string by one million.
///
/// A leading minus is a valid number and means the price was not published.
/// The result trims leading and trailing zeros. This does not use binary floats.
///
/// # Errors
///
/// Returns an error when `raw` is not an optional minus, digits, and at most one dot.
fn per_million(raw: &str) -> Result<Option<String>, ()> {
    let raw = raw.trim();
    let (negative, digits) = match raw.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, raw),
    };
    let (whole, fraction) = split_decimal(digits)?;
    if negative {
        return Ok(None);
    }
    Ok(Some(shift_decimal(&whole, &fraction, 6)))
}

/// Split `digits` on a single dot. Both sides are digits. One of them may be empty.
///
/// # Errors
///
/// Returns an error when `digits` is empty, has more than one dot, or contains a non-digit.
fn split_decimal(digits: &str) -> Result<(String, String), ()> {
    if digits.is_empty() {
        return Err(());
    }
    let (whole, fraction) = match digits.split_once('.') {
        Some((whole, fraction)) => (whole, fraction),
        None => (digits, ""),
    };
    if fraction.contains('.') || !ascii_digits(whole) || !ascii_digits(fraction) {
        return Err(());
    }
    if whole.is_empty() && fraction.is_empty() {
        return Err(());
    }
    Ok((whole.to_owned(), fraction.to_owned()))
}

/// Whether every character is an ASCII digit. An empty string is true.
fn ascii_digits(text: &str) -> bool {
    text.chars().all(|ch| ch.is_ascii_digit())
}

/// Move the decimal point `places` to the right and trim zeros.
fn shift_decimal(whole: &str, fraction: &str, places: usize) -> String {
    let mut digits = String::with_capacity(
        whole
            .len()
            .saturating_add(fraction.len())
            .saturating_add(places),
    );
    digits.push_str(whole);
    digits.push_str(fraction);
    if places >= fraction.len() {
        for _ in 0..places.saturating_sub(fraction.len()) {
            digits.push('0');
        }
        return trim_integer(&digits);
    }
    let fraction_digits = fraction.len().saturating_sub(places);
    let split_at = digits.len().saturating_sub(fraction_digits);
    let Some((head, tail)) = split_chars(&digits, split_at) else {
        return trim_integer(&digits);
    };
    trim_decimal(&format!("{head}.{tail}"))
}

/// Split `text` at a character index. `None` when `index` is past the end.
fn split_chars(text: &str, index: usize) -> Option<(&str, &str)> {
    if text.is_char_boundary(index) {
        Some(text.split_at(index))
    } else {
        None
    }
}

/// Drop leading zeros. An all-zero string becomes `0`.
fn trim_integer(text: &str) -> String {
    let trimmed = text.trim_start_matches('0');
    if trimmed.is_empty() {
        "0".to_owned()
    } else {
        trimmed.to_owned()
    }
}

/// Drop leading zeros on the whole part and trailing zeros on the fraction.
fn trim_decimal(text: &str) -> String {
    let Some((whole, fraction)) = text.split_once('.') else {
        return trim_integer(text);
    };
    let fraction = fraction.trim_end_matches('0');
    let whole = trim_integer(whole);
    if fraction.is_empty() {
        whole
    } else {
        format!("{whole}.{fraction}")
    }
}

/// Whether `needle` appears in `haystack`, ignoring case. An empty needle matches.
fn contains_ignore_case(haystack: &str, needle: &str) -> bool {
    let needle = needle.to_lowercase();
    if needle.is_empty() {
        return true;
    }
    haystack.to_lowercase().contains(&needle)
}

#[cfg(test)]
mod tests {
    use super::{matching, parse_openrouter, per_million, ranked_by_intelligence};

    #[test]
    fn per_million_shifts_the_decimal_without_a_float() {
        assert_eq!(
            per_million("0.000002").expect("2 per million"),
            Some("2".to_owned())
        );
        assert_eq!(
            per_million("0.00001").expect("10 per million"),
            Some("10".to_owned())
        );
        assert_eq!(
            per_million("0.000000079").expect("a fractional dollar"),
            Some("0.079".to_owned())
        );
        assert_eq!(
            per_million("0.0000014").expect("1.4 per million"),
            Some("1.4".to_owned())
        );
        assert_eq!(
            per_million("0.0000001188").expect("a longer fraction"),
            Some("0.1188".to_owned())
        );
        assert_eq!(
            per_million("0").expect("a free model"),
            Some("0".to_owned())
        );
        assert_eq!(
            per_million("00.10").expect("leading zeros"),
            Some("100000".to_owned())
        );
        assert_eq!(per_million("-1").expect("the unpublished sentinel"), None);
        assert_eq!(per_million(" -1 ").expect("a padded sentinel"), None);
        assert!(
            per_million("-nope").is_err(),
            "a negative word is not a sentinel"
        );
        assert!(per_million("").is_err(), "an empty price is not a number");
        assert!(per_million("1.2.3").is_err(), "two dots are not a decimal");
    }

    #[test]
    fn openrouter_scores_keep_blank_rows_and_ignore_design_arena() {
        let bytes = r#"{
            "data": [
                {
                    "id": "low/one",
                    "name": "Low",
                    "description": "plain",
                    "pricing": {"prompt": "0.000001", "completion": "0.000002"},
                    "benchmarks": {
                        "artificial_analysis": {"intelligence_index": 10, "coding_index": 20, "agentic_index": null},
                        "design_arena": {"uicomponent": {"elo": 9999}}
                    }
                },
                {
                    "id": "high/two",
                    "name": "High Two",
                    "description": "kimi cousin",
                    "pricing": {"prompt": "0.000002", "completion": "0.00001"},
                    "benchmarks": {"artificial_analysis": {"intelligence_index": 55.1, "coding_index": null, "agentic_index": 3}}
                },
                {
                    "id": "tie/four",
                    "name": "Tie",
                    "pricing": {"prompt": "0.000000079", "completion": "0.0000014"},
                    "benchmarks": {"artificial_analysis": {"intelligence_index": 55.1, "coding_index": 1, "agentic_index": 1}}
                },
                {
                    "id": "blank/three",
                    "name": "Blank",
                    "description": "kimi blank",
                    "pricing": {"prompt": "-1", "completion": "0"}
                }
            ]
        }"#;
        let rows = parse_openrouter(bytes.as_bytes()).expect("the fixture should parse");
        assert_eq!(
            rows.len(),
            4,
            "a row with no benchmarks object is still a row"
        );
        let high = rows
            .iter()
            .find(|row| row.id() == "high/two")
            .expect("high/two");
        assert_eq!(high.prompt_per_million(), Some("2"));
        assert_eq!(high.completion_per_million(), Some("10"));
        assert_eq!(high.intelligence_text(), "55.1");
        assert_eq!(high.coding_text(), "-");
        assert_eq!(high.agentic_text(), "3");
        let blank = rows
            .iter()
            .find(|row| row.id() == "blank/three")
            .expect("blank");
        assert_eq!(blank.prompt_per_million(), None);
        assert_eq!(blank.completion_per_million(), Some("0"));
        let ranked = ranked_by_intelligence(&rows);
        assert_eq!(
            ranked.iter().map(|row| row.id()).collect::<Vec<_>>(),
            ["high/two", "tie/four", "low/one"],
            "ties keep catalog order and a blank score is left out"
        );
        let matched = matching(&rows, &["KIMI"]);
        assert_eq!(
            matched.iter().map(|row| row.id()).collect::<Vec<_>>(),
            ["high/two", "blank/three"],
            "a term still returns the row that has no score, in catalog order"
        );
        let rendered = format!("{rows:?}");
        assert!(
            !rendered.contains("9999") && !rendered.contains("design_arena"),
            "design arena must not be stored on the row; rendered: {rendered}"
        );
    }

    #[test]
    fn a_bad_price_names_the_row_and_does_not_invent_a_number() {
        let bytes = r#"{"data":[{"id":"bad","pricing":{"prompt":"free","completion":"0"}}]}"#;
        let err = parse_openrouter(bytes.as_bytes()).expect_err("a word is not a price");
        let message = err.to_string();
        assert!(
            message.contains("data[0]") && message.contains("pricing.prompt"),
            "the row error should name the field; message: {message}"
        );
    }
}
