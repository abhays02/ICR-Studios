//! Laya: the decision layer.
//!
//! Indra perceives — it finds lines and reads glyphs. Laya decides what those
//! readings *mean* and whether they can be trusted: which recognizer should have
//! read a line, whether a recognized value satisfies the rule for its field,
//! whether two fields that must agree actually do, and which lines a human must
//! look at.
//!
//! Every rule here is deterministic and auditable. Laya never invents a value.
//! A field it cannot read or cannot validate is reported as absent or invalid,
//! with the reason, because in a banking workflow a silent guess is worse than
//! a gap.

use serde::{Deserialize, Serialize};

use crate::params::{MIN_CONF, ROUTE_SURE};

/// Recognizer groups, in `script.onnx`'s class order. `base` is `rec.onnx`.
pub const GROUPS: [&str; 4] = ["base", "cjk", "rtl", "other"];

/// Which recognizer produced a line.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScriptGroup {
    Base,
    Cjk,
    Rtl,
    Other,
}

impl ScriptGroup {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Base => "base",
            Self::Cjk => "cjk",
            Self::Rtl => "rtl",
            Self::Other => "other",
        }
    }

    pub fn from_index(i: usize) -> Self {
        match i {
            1 => Self::Cjk,
            2 => Self::Rtl,
            3 => Self::Other,
            _ => Self::Base,
        }
    }
}

/// Routing decision for one line.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RouteDecision {
    pub group: ScriptGroup,
    /// The classifier's probability for the chosen group.
    pub prob: f32,
    /// True when the classifier was sure enough to skip the base recognizer.
    /// When false, both read the line and the more confident result wins —
    /// sending a Latin line to a group recognizer loses it entirely.
    pub exclusive: bool,
}

/// `pipeline.ROUTE_SURE` applied to `script.onnx`'s softmax output.
pub fn route(probs: &[f32]) -> RouteDecision {
    if probs.is_empty() {
        return RouteDecision {
            group: ScriptGroup::Base,
            prob: 0.0,
            exclusive: false,
        };
    }
    let mut best = 0usize;
    for (i, p) in probs.iter().enumerate().skip(1) {
        if *p > probs[best] {
            best = i;
        }
    }
    let prob = probs[best];
    RouteDecision {
        group: ScriptGroup::from_index(best),
        prob,
        // Base is already the fallback, so a confident "base" needs no second
        // opinion; a confident non-base group does not get read twice.
        exclusive: prob >= ROUTE_SURE,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Info,
    Warn,
    Fault,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Anomaly {
    pub code: String,
    pub message: String,
    pub severity: Severity,
    /// Index into the page's line list, when the anomaly is about one line.
    pub line_index: Option<usize>,
}

impl Anomaly {
    fn new(
        code: &'static str,
        message: impl Into<String>,
        severity: Severity,
        line_index: Option<usize>,
    ) -> Self {
        Self {
            code: code.to_string(),
            message: message.into(),
            severity,
            line_index,
        }
    }
}

/// A recognized value bound to a form field, with the rule that judged it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtractedField {
    pub name: String,
    pub value: String,
    pub conf: f32,
    /// `None` when no rule applies to this field. Never `Some(true)` by default:
    /// unvalidated means unvalidated.
    pub valid: Option<bool>,
    /// Human-readable statement of the rule, so a reviewer can check the check.
    pub rule: String,
    pub line: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Extraction {
    pub doc_type: Option<String>,
    pub fields: Vec<ExtractedField>,
    pub anomalies: Vec<Anomaly>,
    /// `None` when either side of the comparison is missing or unreadable. A
    /// mismatch is only claimed when both amounts were actually read.
    pub car_lar_match: Option<bool>,
}

/// One recognized line as Laya sees it.
#[derive(Clone, Debug, PartialEq)]
pub struct LineReading<'a> {
    pub text: &'a str,
    pub conf: f32,
    pub group: ScriptGroup,
    pub y0: f32,
    pub y1: f32,
    pub x0: f32,
    pub x1: f32,
}

/// The comb-box digit confusions. Inside a numeric field the recognizer's
/// letter readings are near-certainly these digits: the boxes are pre-printed
/// and the writer had one character per cell.
///
/// Applied *only* inside fields known to be numeric. Applying it globally would
/// silently corrupt real words.
pub fn comb_normalize(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'O' | 'o' | 'D' => '0',
            'I' | 'l' | '|' => '1',
            'Z' | 'z' => '2',
            'S' | 's' => '5',
            'G' => '6',
            'B' => '8',
            'q' => '9',
            other => other,
        })
        .collect()
}

/// Strip everything that is not a digit. Scanned separators are unreliable: a
/// comma can read as an apostrophe and a space as nothing at all.
pub fn digits_only(s: &str) -> String {
    s.chars().filter(|c| c.is_ascii_digit()).collect()
}

/// IFSC: 4 uppercase letters, a literal `0`, then 6 alphanumerics.
/// e.g. `HDFC0001234`.
pub fn validate_ifsc(s: &str) -> bool {
    let t: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    let b: Vec<char> = t.chars().collect();
    b.len() == 11
        && b[..4].iter().all(|c| c.is_ascii_uppercase())
        && b[4] == '0'
        && b[5..].iter().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
}

/// PAN: 5 letters, 4 digits, 1 letter. e.g. `ABCDE1234F`.
pub fn validate_pan(s: &str) -> bool {
    let t: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    let b: Vec<char> = t.chars().collect();
    b.len() == 10
        && b[..5].iter().all(|c| c.is_ascii_uppercase())
        && b[5..9].iter().all(|c| c.is_ascii_digit())
        && b[9].is_ascii_uppercase()
}

/// A `DD/MM/YYYY` date that is also a real calendar date.
/// Validate a date written either `DD/MM/YYYY` (the handwritten fields on these
/// forms) or `YYYY-MM-DD` (printed date-of-birth fields). Both are checked
/// against the real calendar, so `30/02/1991` and `1991-02-30` both fail.
pub fn validate_date(s: &str) -> bool {
    let parts: Vec<&str> = s.trim().split(['/', '-', '.']).collect();
    if parts.len() != 3 {
        return false;
    }
    let nums: Option<Vec<i64>> = parts.iter().map(|p| p.parse::<i64>().ok()).collect();
    let Some(nums) = nums else {
        return false;
    };
    // The year is whichever end carries four digits; if neither or both do, this
    // is not a date. Two-digit years are ambiguous against day and month and are
    // rejected rather than guessed.
    let (d, m, y) = if parts[0].len() == 4 && parts[2].len() != 4 {
        (nums[2], nums[1], nums[0])
    } else if parts[2].len() == 4 && parts[0].len() != 4 {
        (nums[0], nums[1], nums[2])
    } else {
        return false;
    };
    if !(1..=12).contains(&m) || d <= 0 || y < 1900 || y > 2100 {
        return false;
    }
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let days = match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if leap {
                29
            } else {
                28
            }
        }
        _ => return false,
    };
    d <= days
}

/// Parse an amount written in words, including the Indian numbering system
/// (lakh = 100,000; crore = 10,000,000) which the CAR field on these forms uses.
///
/// Returns `None` rather than a partial number: a half-parsed amount compared
/// against the figures field would produce a false mismatch.
pub fn parse_amount_words(s: &str) -> Option<u64> {
    let units = [
        ("one", 1u64),
        ("two", 2),
        ("three", 3),
        ("four", 4),
        ("five", 5),
        ("six", 6),
        ("seven", 7),
        ("eight", 8),
        ("nine", 9),
        ("ten", 10),
        ("eleven", 11),
        ("twelve", 12),
        ("thirteen", 13),
        ("fourteen", 14),
        ("fifteen", 15),
        ("sixteen", 16),
        ("seventeen", 17),
        ("eighteen", 18),
        ("nineteen", 19),
    ];
    let tens = [
        ("twenty", 20u64),
        ("thirty", 30),
        ("forty", 40),
        ("fourty", 40), // a common misspelling on real forms
        ("fifty", 50),
        ("sixty", 60),
        ("seventy", 70),
        ("eighty", 80),
        ("ninety", 90),
    ];

    let norm: String = s
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() || c.is_whitespace() { c } else { ' ' })
        .collect();
    let tokens: Vec<&str> = norm.split_whitespace().filter(|w| !w.is_empty()).collect();

    let is_number_word = |w: &str| {
        units.iter().any(|(k, _)| *k == w) || tens.iter().any(|(k, _)| *k == w)
    };

    // "...five hundred and fifty paise only" is 12,500.50, not 12,550: the number
    // directly before `paise` is a sub-rupee amount. `parse_amount_figures`
    // truncates at the decimal point for the same reason, so dropping it here
    // keeps the two sides of the CAR/LAR check comparable.
    let mut drop_paise = vec![false; tokens.len()];
    for (idx, w) in tokens.iter().enumerate() {
        if *w != "paise" {
            continue;
        }
        let mut back = idx;
        let mut removed = 0;
        while back > 0 && removed < 2 {
            back -= 1;
            if !is_number_word(tokens[back]) {
                break;
            }
            drop_paise[back] = true;
            removed += 1;
        }
    }

    let mut total: u64 = 0; // everything above the current scale word
    let mut current: u64 = 0; // the group being accumulated
    let mut saw_number = false;

    for (raw, dropped) in tokens.iter().zip(&drop_paise) {
        let w = *raw;
        if *dropped {
            continue;
        }
        if w == "and" || w == "only" || w == "rupees" || w == "rupee" || w == "paise" {
            continue;
        }
        if let Some((_, v)) = units.iter().find(|(k, _)| *k == w) {
            current += v;
            saw_number = true;
            continue;
        }
        if let Some((_, v)) = tens.iter().find(|(k, _)| *k == w) {
            current += v;
            saw_number = true;
            continue;
        }
        match w {
            "hundred" => {
                current = if current == 0 { 100 } else { current * 100 };
                saw_number = true;
            }
            "thousand" => {
                total += if current == 0 { 1000 } else { current * 1000 };
                current = 0;
                saw_number = true;
            }
            // lakh and crore scale the whole accumulated amount so far, which is
            // what makes "one lakh twenty thousand" = 120,000.
            "lakh" | "lac" | "lakhs" => {
                total += if current == 0 { 100_000 } else { current * 100_000 };
                current = 0;
                saw_number = true;
            }
            "crore" | "crores" | "cr" => {
                total += if current == 0 { 10_000_000 } else { current * 10_000_000 };
                current = 0;
                saw_number = true;
            }
            // An unrecognised word does not abort the parse: OCR of the CAR field
            // routinely misreads one word, and the rest still carries the amount.
            _ => continue,
        }
    }

    if !saw_number {
        return None;
    }
    Some(total + current)
}

/// Parse an amount in figures: `1,23,456.00`, `Rs 5000`, `INR 1 000`.
pub fn parse_amount_figures(s: &str) -> Option<u64> {
    let chars: Vec<char> = s.chars().collect();
    // Skip a currency prefix; the amount starts at the first digit.
    let start = chars.iter().position(|c| c.is_ascii_digit())?;
    let mut out = String::new();
    for &c in &chars[start..] {
        if c.is_ascii_digit() {
            out.push(c);
        } else if c == ',' || c == ' ' || c == '\u{a0}' {
            // Thousands separators carry no value and are read unreliably.
            continue;
        } else {
            // Decimal point or trailing text ("5000.00", "5000/-"): paise are
            // not part of the integer amount the CAR field is compared against.
            break;
        }
    }
    if out.is_empty() {
        return None;
    }
    out.parse::<u64>().ok()
}

/// Field labels as they appear on the forms, matched case-insensitively against
/// the alphanumeric-only normalization of a line's leading label.
///
/// `(field, aliases, also match as a suffix)`. Every alias has to be specific
/// enough that no unrelated label matches it: the bare abbreviations are
/// deliberately absent, because `car` catches "Carbon copy", `lar` catches
/// "Declaration" and `account` catches "Account type" — all three were measured
/// producing false faults on the shipped samples.
///
/// Suffix matching is what makes "Requested amount" and "SB Account" resolve. It
/// is off for `date` on purpose: "Date of birth" and "Declaration date" are
/// different fields, and attributing either to *the* date field is a guess.
const FIELD_LABELS: [(&str, &[&str], bool); 7] = [
    ("ifsc", &["ifsc", "ifsccode", "bankcode"], true),
    ("pan", &["pan", "panno", "pannumber", "incometax"], true),
    (
        "account_no",
        &["accountno", "acctno", "accountnumber", "acno", "sbaccount"],
        true,
    ),
    ("phone", &["mobile", "phone", "contact", "telephone"], true),
    (
        "amount_words",
        &["amountinwords", "inwords", "rupeesinwords"],
        true,
    ),
    (
        "amount_figures",
        &["amountinfigures", "infigures", "amount"],
        true,
    ),
    ("date", &["date", "dated", "ddmmyyyy"], false),
];

fn normalize_label(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// Which field a line's leading label indicates, if any.
fn field_for(label: &str) -> Option<&'static str> {
    let n = normalize_label(label);
    if n.is_empty() {
        return None;
    }
    FIELD_LABELS
        .iter()
        .find(|(_, aliases, suffix)| {
            aliases.iter().any(|k| {
                n.starts_with(k) || (*suffix && n.len() > k.len() && n.ends_with(k))
            })
        })
        .map(|(name, _, _)| *name)
}

/// Split a line into `label: value` on the first colon, or on a long gap.
/// Returns `(label, value)`; when there is no separator the whole line is the
/// value and the label is empty.
fn split_label_value(text: &str) -> (&str, &str) {
    if let Some(i) = text.find(':') {
        return (text[..i].trim(), text[i + 1..].trim());
    }
    (text.trim(), "")
}

/// Apply one field's rule to a value string.
///
/// Returns `(name, value, valid, rule)`. `valid` is `Some(false)` when the value
/// was read but fails the rule — that is a fault worth raising, and is the whole
/// point of validating rather than pattern-matching.
fn build_candidate(
    named: &'static str,
    value: &str,
) -> Option<(&'static str, String, Option<bool>, &'static str)> {
    Some(match named {
        "ifsc" => {
            let v = value.to_uppercase();
            ("ifsc", v.clone(), Some(validate_ifsc(&v)), "4 letters + '0' + 6 alphanumerics")
        }
        "pan" => {
            let v = value.to_uppercase();
            ("pan", v.clone(), Some(validate_pan(&v)), "5 letters + 4 digits + 1 letter")
        }
        // Comb box: normalize the known digit confusions, then require that
        // nothing but digits remains.
        "account_no" => {
            let normalized = comb_normalize(value);
            let d = digits_only(&normalized);
            let valid = d.len() >= 9 && d.len() <= 18 && d == normalized.trim();
            (
                "account_no",
                d.clone(),
                Some(valid),
                "digits only after comb-box normalization",
            )
        }
        // A phone number is printed, not boxed, so separators and a country code
        // are expected rather than a fault. Only the digit count is checked.
        "phone" => {
            let d = digits_only(value);
            (
                "phone",
                d.clone(),
                Some((10..=15).contains(&d.len())),
                "10 to 15 digits once separators are dropped",
            )
        }
        "date" => (
            "date",
            value.to_string(),
            Some(validate_date(value)),
            "DD/MM/YYYY or YYYY-MM-DD, and a real calendar date",
        ),
        "amount_words" => (
            "amount_words",
            value.to_string(),
            Some(parse_amount_words(value).is_some()),
            "parses to an amount, including lakh/crore",
        ),
        "amount_figures" => (
            "amount_figures",
            value.to_string(),
            Some(parse_amount_figures(value).is_some()),
            "parses to an integer amount",
        ),
        _ => return None,
    })
}

/// Run Laya over a page's recognized lines.
///
/// `min_conf` gates text: a line below it contributes no value, because on
/// empty form fields, checkboxes and rules the recognizer reads `-`, `I` or `1`
/// at low confidence, and those readings look exactly like real data.
///
/// Values arrive in two shapes. `Date: 15/10/2026` carries its label and its
/// value on one line. Printed forms more often put the label on one line and the
/// handwritten value on the next, so a label-only line arms the field and the
/// *immediately following* line fills it. Adjacency is required on purpose: a
/// bare line three rows below a label is far more likely to belong to a
/// different field, and misattributing a value is worse than reporting none.
pub fn extract(readings: &[LineReading], min_conf: f32) -> Extraction {
    let mut fields: Vec<ExtractedField> = Vec::new();
    let mut anomalies: Vec<Anomaly> = Vec::new();
    let mut pending: Option<(&'static str, usize)> = None;

    for (i, r) in readings.iter().enumerate() {
        // A line the recognizer was unsure about is evidence, not text.
        if r.conf < min_conf || r.text.trim().is_empty() {
            if r.conf < min_conf && !r.text.trim().is_empty() {
                anomalies.push(Anomaly::new(
                    "low_confidence_suppressed",
                    format!(
                        "line {} read {:?} at confidence {:.2}, below the {:.2} gate; no text emitted",
                        i,
                        r.text.trim(),
                        r.conf,
                        min_conf
                    ),
                    Severity::Warn,
                    Some(i),
                ));
            }
            continue;
        }

        let (label, raw_value) = split_label_value(r.text);
        let named = field_for(label);

        let candidate = if raw_value.is_empty() {
            match named {
                // Label alone: it names the field the next line fills in.
                // Emitting it as its own value would report `date = "Date"` and
                // then fault it, which is a lie about the page.
                Some(name) => {
                    pending = Some((name, i));
                    None
                }
                // No label of its own, so it is a value for the line above —
                // but only for the line directly above.
                None => pending
                    .filter(|(_, at)| *at + 1 == i)
                    .and_then(|(name, _)| build_candidate(name, r.text.trim())),
            }
        } else {
            named.and_then(|name| build_candidate(name, raw_value))
        };

        if let Some((name, v, valid, rule)) = candidate {
            if pending.is_some_and(|(p, _)| p == name) {
                pending = None;
            }
            if valid == Some(false) {
                anomalies.push(Anomaly::new(
                    "field_invalid",
                    format!("{name} value {v:?} fails its rule: {rule}"),
                    Severity::Fault,
                    Some(i),
                ));
            }
            fields.push(ExtractedField {
                name: name.to_string(),
                value: v,
                conf: r.conf,
                valid,
                rule: rule.to_string(),
                line: i,
            });
        }
    }

    // CAR vs LAR: the same amount written twice, in words and in figures. A
    // mismatch is the single most common fraud and error signal on these forms.
    let words = fields.iter().find(|f| f.name == "amount_words");
    let figs = fields.iter().find(|f| f.name == "amount_figures");
    let car_lar_match = match (words, figs) {
        (Some(w), Some(f)) => match (parse_amount_words(&w.value), parse_amount_figures(&f.value)) {
            (Some(a), Some(b)) => {
                let matched = a == b;
                anomalies.push(if matched {
                    Anomaly::new(
                        "car_lar_agree",
                        format!("amount in words and figures both read {a}"),
                        Severity::Info,
                        Some(w.line),
                    )
                } else {
                    Anomaly::new(
                        "car_lar_mismatch",
                        format!("amount in words reads {a} but amount in figures reads {b}"),
                        Severity::Fault,
                        Some(w.line),
                    )
                });
                Some(matched)
            }
            // One side unreadable: report the gap, do not claim a mismatch.
            (a, b) => {
                anomalies.push(Anomaly::new(
                    "car_lar_incomplete",
                    format!(
                        "cannot cross-check: words={}, figures={}",
                        if a.is_some() { "parsed" } else { "unreadable" },
                        if b.is_some() { "parsed" } else { "unreadable" }
                    ),
                    Severity::Warn,
                    None,
                ));
                None
            }
        },
        _ => None,
    };

    // Duplicate fields: the same field read twice usually means a line was
    // split, or the form genuinely has two of them. Either way a human decides.
    let mut seen: Vec<&str> = Vec::new();
    for f in &fields {
        if seen.contains(&f.name.as_str()) {
            anomalies.push(Anomaly::new(
                "duplicate_field",
                format!("field {} was read more than once", f.name),
                Severity::Warn,
                Some(f.line),
            ));
        } else {
            seen.push(f.name.as_str());
        }
    }

    let doc_type = guess_doc_type(readings, min_conf);

    Extraction {
        doc_type,
        fields,
        anomalies,
        car_lar_match,
    }
}

/// Which lines need a human, and why. Drives the review queue in Studio.
pub fn review_flags(readings: &[LineReading], min_conf: f32) -> Vec<bool> {
    readings
        .iter()
        .map(|r| {
            r.conf < min_conf
                || r.conf < 0.9 && r.text.trim().is_empty()
                || r.group != ScriptGroup::Base
        })
        .collect()
}

/// A deliberately weak document-type guess, from which field labels appeared.
/// It is a routing hint for the operator, never evidence.
fn guess_doc_type(readings: &[LineReading], min_conf: f32) -> Option<String> {
    let lines: Vec<String> = readings
        .iter()
        .filter(|r| r.conf >= min_conf)
        .map(|r| r.text.to_lowercase())
        .collect();
    // The short tokens have to match as whole words. `normalize_label` strips
    // every separator, and under that normalization "declaration" contains "lar"
    // and "carbon" contains "car" — which is how a CAR form used to be typed as
    // a LAR. Longer phrases are safe as plain substrings.
    let word = |k: &str| {
        lines.iter().any(|t| {
            t.split(|c: char| !c.is_alphanumeric())
                .any(|w| w == k)
        })
    };
    let has = |k: &str| lines.iter().any(|t| t.contains(k));

    if word("cheque") || word("chq") || (has("amount in words") && has("ifsc")) {
        Some("cheque".into())
    } else if word("kyc") || has("know your customer") {
        Some("kyc".into())
    } else if word("rtgs") || word("neft") {
        Some("rtgs".into())
    } else if word("lar") || has("loan application") {
        Some("lar".into())
    } else if word("car") || has("customer acquisition") {
        Some("car".into())
    } else {
        None
    }
}

/// The confidence band that decides a line's colour in every UI: teal, amber or
/// fault. Kept here so the three surfaces cannot drift apart.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfidenceBand {
    /// At or above 0.9: machine-accepted.
    Verified,
    /// At or above `MIN_CONF`: readable but worth a glance.
    Review,
    /// Below `MIN_CONF`: not emitted as text at all.
    Rejected,
}

impl ConfidenceBand {
    pub fn of(conf: f32) -> Self {
        if conf < MIN_CONF {
            Self::Rejected
        } else if conf < 0.9 {
            Self::Review
        } else {
            Self::Verified
        }
    }

    /// The design-token custom property each band maps to.
    pub fn token(&self) -> &'static str {
        match self {
            Self::Verified => "var(--signal-500)",
            Self::Review => "var(--warn-500)",
            Self::Rejected => "var(--fault-500)",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(text: &str, conf: f32) -> LineReading<'_> {
        LineReading {
            text,
            conf,
            group: ScriptGroup::Base,
            y0: 0.0,
            y1: 20.0,
            x0: 0.0,
            x1: 200.0,
        }
    }

    #[test]
    fn routing_is_exclusive_only_above_the_threshold() {
        let sure = route(&[0.02, 0.97, 0.005, 0.005]);
        assert_eq!(sure.group, ScriptGroup::Cjk);
        assert!(sure.exclusive);

        let unsure = route(&[0.5, 0.45, 0.03, 0.02]);
        assert_eq!(unsure.group, ScriptGroup::Base);
        assert!(!unsure.exclusive, "0.5 < ROUTE_SURE must not skip the base recognizer");

        assert_eq!(route(&[]).group, ScriptGroup::Base);
    }

    #[test]
    fn comb_normalization_maps_only_known_confusions() {
        assert_eq!(comb_normalize("O1234I56"), "01234156");
        // S -> 5 and B -> 8 by design. N is not a known confusion and survives,
        // which is what makes the digits-only validity check fail loudly rather
        // than silently accept a mangled account number.
        assert_eq!(comb_normalize("SBIN0001234"), "581N0001234");
        assert_eq!(comb_normalize("1234567890"), "1234567890");
        assert_eq!(digits_only("12-34, 56"), "123456");
    }

    #[test]
    fn ifsc_validation() {
        assert!(validate_ifsc("HDFC0001234"));
        assert!(validate_ifsc(" SBIN0000001 "));
        assert!(!validate_ifsc("HDFC1001234"), "5th char must be a literal 0");
        assert!(!validate_ifsc("HDFC000123"), "must be 11 characters");
        assert!(!validate_ifsc("hdfc0001234"), "must be uppercase");
    }

    #[test]
    fn pan_validation() {
        assert!(validate_pan("ABCDE1234F"));
        assert!(!validate_pan("ABCD1234F"), "too short");
        assert!(!validate_pan("ABCDE12F4G"), "digits in the wrong place");
    }

    #[test]
    fn date_validation_rejects_impossible_dates() {
        assert!(validate_date("15/08/1947"));
        assert!(validate_date("29/02/2024"), "2024 is a leap year");
        assert!(!validate_date("29/02/2023"), "2023 is not");
        assert!(!validate_date("31/04/2020"), "April has 30 days");
        assert!(!validate_date("00/01/2020"));
        assert!(!validate_date("13/13/2020"));
        assert!(!validate_date("not a date"));
    }

    #[test]
    fn indian_numbering_parses() {
        assert_eq!(parse_amount_words("One Lakh Twenty Thousand"), Some(120_000));
        assert_eq!(parse_amount_words("Two Crore Fifty Lakh"), Some(25_000_000));
        assert_eq!(parse_amount_words("Rupees Five Thousand Only"), Some(5_000));
        assert_eq!(parse_amount_words("One Hundred and Fifty"), Some(150));
        assert_eq!(parse_amount_words("forty two"), Some(42));
        assert_eq!(parse_amount_words(""), None);
        assert_eq!(parse_amount_words("no numbers here"), None);
    }

    #[test]
    fn figures_parsing_ignores_separators_and_suffixes() {
        assert_eq!(parse_amount_figures("1,23,456.00"), Some(123_456));
        assert_eq!(parse_amount_figures("Rs 5000/-"), Some(5000));
        assert_eq!(parse_amount_figures("INR 1000"), Some(1000));
        assert_eq!(parse_amount_figures("no digits"), None);
    }

    #[test]
    fn car_lar_mismatch_is_flagged_as_a_fault() {
        let readings = vec![
            line("Amount in Words: Rupees Five Thousand Only", 0.95),
            line("Amount in Figures: 6000.00", 0.95),
        ];
        let e = extract(&readings, MIN_CONF);
        assert_eq!(e.car_lar_match, Some(false));
        assert!(e
            .anomalies
            .iter()
            .any(|a| a.code == "car_lar_mismatch" && matches!(a.severity, Severity::Fault)));
    }

    #[test]
    fn car_lar_agreement_is_info_not_fault() {
        let readings = vec![
            line("Amount in Words: Rupees Five Thousand Only", 0.95),
            line("Amount in Figures: 5,000.00", 0.95),
        ];
        let e = extract(&readings, MIN_CONF);
        assert_eq!(e.car_lar_match, Some(true));
        assert!(e.anomalies.iter().any(|a| a.code == "car_lar_agree"));
    }

    #[test]
    fn unreadable_side_yields_no_claim() {
        let readings = vec![
            line("Amount in Words: Rupees Five Thousand Only", 0.95),
            line("Amount in Figures: #####", 0.95),
        ];
        let e = extract(&readings, MIN_CONF);
        assert_eq!(
            e.car_lar_match, None,
            "a mismatch must not be claimed when one side is unreadable"
        );
        assert!(e.anomalies.iter().any(|a| a.code == "car_lar_incomplete"));
    }

    #[test]
    fn low_confidence_lines_emit_no_text_and_are_reported() {
        let readings = vec![LineReading {
            text: "I",
            conf: 0.2, // a rule or checkbox read as a glyph
            group: ScriptGroup::Base,
            y0: 0.0,
            y1: 20.0,
            x0: 0.0,
            x1: 200.0,
        }];
        let e = extract(&readings, MIN_CONF);
        assert!(e.fields.is_empty(), "no field may be built from a suppressed line");
        assert!(e.anomalies.iter().any(|a| a.code == "low_confidence_suppressed"));
    }

    #[test]
    fn invalid_ifsc_is_a_fault_with_the_reason() {
        let readings = vec![line("IFSC Code: HDFC1001234", 0.95)];
        let e = extract(&readings, MIN_CONF);
        let f = e.fields.iter().find(|f| f.name == "ifsc").unwrap();
        assert_eq!(f.valid, Some(false));
        assert!(e.anomalies.iter().any(|a| a.code == "field_invalid"));
    }

    #[test]
    fn account_number_uses_comb_normalization() {
        // "SB1N00O1234": O -> 0 is a genuine comb-box confusion, but the stray N
        // is not, so the field is reported invalid rather than quietly accepted.
        let readings = vec![line("Account No: SB1N00O1234", 0.95)];
        let e = extract(&readings, MIN_CONF);
        let f = e.fields.iter().find(|f| f.name == "account_no").unwrap();
        assert_eq!(f.value, "5810001234");
        assert_eq!(f.valid, Some(false), "a surviving letter means the read is not trustworthy");

        // A clean comb-box read validates.
        let clean = extract(&[line("Account No: O0O1234567", 0.95)], MIN_CONF);
        let f = clean.fields.iter().find(|f| f.name == "account_no").unwrap();
        assert_eq!(f.value, "0001234567");
        assert_eq!(f.valid, Some(true));
    }

    #[test]
    fn confidence_bands_map_to_tokens() {
        assert_eq!(ConfidenceBand::of(0.95), ConfidenceBand::Verified);
        assert_eq!(ConfidenceBand::of(0.7), ConfidenceBand::Review);
        assert_eq!(ConfidenceBand::of(0.3), ConfidenceBand::Rejected);
        assert_eq!(ConfidenceBand::Verified.token(), "var(--signal-500)");
        assert_eq!(ConfidenceBand::Review.token(), "var(--warn-500)");
        assert_eq!(ConfidenceBand::Rejected.token(), "var(--fault-500)");
    }

    #[test]
    fn review_flags_low_confidence_and_non_base_scripts() {
        let readings = vec![
            line("clear text", 0.99),
            line("muddy", 0.4),
            LineReading {
                group: ScriptGroup::Cjk,
                ..line("non-latin", 0.99)
            },
        ];
        assert_eq!(review_flags(&readings, MIN_CONF), vec![false, true, true]);
    }

    #[test]
    fn doc_type_guess_is_a_hint_not_evidence() {
        let readings = vec![line("CHEQUE No 123456", 0.95), line("IFSC Code: HDFC0001234", 0.95)];
        assert_eq!(extract(&readings, MIN_CONF).doc_type.as_deref(), Some("cheque"));
        assert_eq!(extract(&[line("hello world", 0.95)], MIN_CONF).doc_type, None);
    }

    #[test]
    fn doc_type_does_not_read_lar_out_of_declaration() {
        // "declaration" contains "lar" once separators are stripped, which used to
        // type every CAR page as a LAR.
        let page = vec![
            line("CAR / version 1.0.0 / TEST ONLY - VOID", 0.99),
            line("Declaration date", 0.99),
        ];
        assert_eq!(extract(&page, MIN_CONF).doc_type.as_deref(), Some("car"));
        assert_eq!(extract(&[line("Declaration", 0.99)], MIN_CONF).doc_type, None);
    }

    #[test]
    fn a_label_on_its_own_line_arms_the_line_below_it() {
        let page = vec![line("Date", 0.99), line("15/10/2026", 0.99)];
        let got = extract(&page, MIN_CONF);
        assert_eq!(got.fields.len(), 1);
        assert_eq!(got.fields[0].name, "date");
        assert_eq!(got.fields[0].value, "15/10/2026");
        assert_eq!(got.fields[0].valid, Some(true));
        assert!(got.anomalies.is_empty(), "a valid date must not raise a fault");
    }

    #[test]
    fn a_label_is_never_reported_as_its_own_value() {
        // Both of these used to arm a field and then fault the following line:
        // "car" is a prefix of the CAR header, "account" of "Account type".
        let header = vec![
            line("CAR / version 1.0.0 / TEST ONLY - VOID", 0.99),
            line("SYNTHETIC DATA. Script FONT, NOT real handwriting.", 0.99),
        ];
        assert!(extract(&header, MIN_CONF).fields.is_empty());

        let account_type = vec![line("Account type", 0.99), line("savings", 0.96)];
        assert!(extract(&account_type, MIN_CONF).fields.is_empty());
    }

    #[test]
    fn a_bare_value_must_be_directly_below_its_label() {
        // The blank line between them breaks adjacency, so the date is reported
        // as not found rather than guessed at from two rows down.
        let page = vec![line("Date", 0.99), line("   ", 0.99), line("15/10/2026", 0.99)];
        assert!(extract(&page, MIN_CONF).fields.is_empty());
    }

    #[test]
    fn a_value_on_the_same_line_as_its_label_still_works() {
        let page = vec![line("IFSC Code: HDFC0001234", 0.99)];
        let got = extract(&page, MIN_CONF);
        assert_eq!(got.fields[0].name, "ifsc");
        assert_eq!(got.fields[0].value, "HDFC0001234");
        assert_eq!(got.fields[0].valid, Some(true));
    }

    #[test]
    fn requested_amount_resolves_by_suffix() {
        let page = vec![line("Requested amount", 0.95), line("25000.00", 0.99)];
        let got = extract(&page, MIN_CONF);
        assert_eq!(got.fields.len(), 1);
        assert_eq!(got.fields[0].name, "amount_figures");
        assert_eq!(got.fields[0].valid, Some(true));
    }

    #[test]
    fn dates_accept_both_orders_and_reject_impossible_days() {
        assert!(validate_date("15/10/2026"));
        assert!(validate_date("2026-10-15"), "printed DOB fields are ISO");
        assert!(validate_date("29/02/2024"), "2024 is a leap year");
        assert!(!validate_date("1991-02-30"), "February has no 30th");
        assert!(!validate_date("29/02/1900"), "1900 is not a leap year");
        assert!(!validate_date("15/10/26"), "a two-digit year is ambiguous, not guessed");
        assert!(!validate_date("not a date"));
    }

    #[test]
    fn phone_tolerates_separators_and_a_country_code() {
        let page = vec![line("Phone (reserved fictional range)", 0.99), line("+1 202 555 0107", 0.96)];
        let got = extract(&page, MIN_CONF);
        assert_eq!(got.fields[0].name, "phone");
        assert_eq!(got.fields[0].value, "12025550107");
        assert_eq!(got.fields[0].valid, Some(true));

        let short = vec![line("Phone", 0.99), line("555", 0.96)];
        assert_eq!(extract(&short, MIN_CONF).fields[0].valid, Some(false));
    }

    #[test]
    fn paise_is_not_added_to_the_rupee_total() {
        // 12,500.50 — "fifty paise" is a fraction, and the figures side truncates
        // at the decimal point, so both have to drop it for CAR/LAR to compare.
        assert_eq!(
            parse_amount_words("Twelve thousand five hundred and fifty paise only"),
            Some(12_500)
        );
        assert_eq!(parse_amount_words("Seven thousand and twenty five only"), Some(7_025));
        assert_eq!(parse_amount_words("One lakh twenty thousand only"), Some(120_000));
        assert_eq!(parse_amount_words("Rupees two crore only"), Some(20_000_000));
        assert_eq!(parse_amount_words("no numbers here"), None);
    }

    #[test]
    fn car_and_lar_agree_across_the_paise_boundary() {
        let page = vec![
            line("Amount in words", 0.98),
            line("Twelve thousand five hundred and fifty paise only", 0.98),
            line("Amount in figures", 0.99),
            line("12500.50", 0.99),
        ];
        let got = extract(&page, MIN_CONF);
        assert_eq!(got.car_lar_match, Some(true));
    }
}
