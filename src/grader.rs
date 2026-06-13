//! Grading strategies: how do we decide whether a model's answer is correct?
//!
//! Exact string matching is too brittle for real LLM output (models add
//! pleasantries, wrap answers in code fences, format numbers differently),
//! so the harness ships several graders of increasing tolerance:
//!
//!   exact         answer == expectation (after trimming)
//!   contains      expectation appears somewhere in the answer
//!   not_contains  expectation must NOT appear (e.g. refusal checks)
//!   any_of        answer must contain one of several `|`-separated options
//!   numeric       first number in the answer, within a tolerance
//!   json          answer must be valid JSON with `path=value` satisfied

use crate::json;

#[derive(Debug, Clone, PartialEq)]
pub enum Grader {
    Exact,
    Contains,
    NotContains,
    AnyOf,
    Numeric { tolerance: f64 },
    Json,
}

impl Grader {
    /// Build a grader from the name used in `.eval` files.
    pub fn from_name(name: &str, tolerance: f64) -> Result<Grader, String> {
        match name {
            "exact" => Ok(Grader::Exact),
            "contains" => Ok(Grader::Contains),
            "not_contains" => Ok(Grader::NotContains),
            "any_of" => Ok(Grader::AnyOf),
            "numeric" => Ok(Grader::Numeric { tolerance }),
            "json" => Ok(Grader::Json),
            other => Err(format!(
                "unknown grader `{}` (expected exact, contains, not_contains, any_of, numeric, or json)",
                other
            )),
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Grader::Exact => "exact",
            Grader::Contains => "contains",
            Grader::NotContains => "not_contains",
            Grader::AnyOf => "any_of",
            Grader::Numeric { .. } => "numeric",
            Grader::Json => "json",
        }
    }

    /// The core question: does `actual` satisfy `expect`?
    /// Returns Err only when grading itself is impossible
    /// (e.g. a malformed expectation), not when the answer is wrong.
    pub fn grade(&self, expect: &str, actual: &str) -> Result<bool, String> {
        // Models love wrapping answers in markdown code fences;
        // strip them before judging so we grade the content.
        let answer = strip_code_fences(actual);

        match self {
            Grader::Exact => Ok(answer.trim() == expect.trim()),

            Grader::Contains => Ok(normalize(answer).contains(&normalize(expect))),

            Grader::NotContains => Ok(!normalize(answer).contains(&normalize(expect))),

            Grader::AnyOf => {
                let haystack = normalize(answer);
                Ok(expect
                    .split('|')
                    .map(normalize)
                    .any(|option| !option.is_empty() && haystack.contains(&option)))
            }

            Grader::Numeric { tolerance } => {
                let want: f64 = expect
                    .trim()
                    .parse()
                    .map_err(|_| format!("expectation `{}` is not a number", expect))?;
                match extract_first_number(answer) {
                    Some(got) => Ok((got - want).abs() <= *tolerance),
                    None => Ok(false), // no number in the answer = wrong
                }
            }

            Grader::Json => {
                // Expectation format: "path.to.field=value"
                let (path, want) = expect
                    .split_once('=')
                    .ok_or_else(|| format!("json expectation `{}` must be `path=value`", expect))?;
                let parsed = match json::parse(answer.trim()) {
                    Ok(v) => v,
                    Err(_) => return Ok(false), // invalid JSON = wrong answer
                };
                match parsed.get_path(path.trim()) {
                    Some(v) => Ok(v.as_comparable_string() == want.trim()),
                    None => Ok(false), // missing field = wrong answer
                }
            }
        }
    }
}

/// Lowercase + trim, so "Paris" matches "paris".
fn normalize(s: &str) -> String {
    s.trim().to_lowercase()
}

/// If the text is wrapped in ``` fences (optionally with a language tag),
/// return just the inside. Otherwise return the trimmed text unchanged.
fn strip_code_fences(s: &str) -> &str {
    let t = s.trim();
    if let Some(rest) = t.strip_prefix("```") {
        // Drop the rest of the opening fence line ("json\n", "\n", ...).
        if let Some(newline) = rest.find('\n') {
            let body = &rest[newline + 1..];
            if let Some(end) = body.rfind("```") {
                return body[..end].trim();
            }
        }
    }
    t
}

/// Scan the text for the first parseable number.
/// "The answer is 391." -> Some(391.0)
fn extract_first_number(s: &str) -> Option<f64> {
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_ascii_digit() || (chars[i] == '-' && i + 1 < chars.len() && chars[i + 1].is_ascii_digit()) {
            // Found the start of a candidate number; extend it.
            let start = i;
            i += 1;
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                i += 1;
            }
            let candidate: String = chars[start..i].iter().collect();
            // Trim a trailing '.' (sentence punctuation, e.g. "is 391.")
            let candidate = candidate.trim_end_matches('.');
            if let Ok(n) = candidate.parse::<f64>() {
                return Some(n);
            }
        } else {
            i += 1;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_grading() {
        let g = Grader::Exact;
        assert!(g.grade("391", "  391\n").unwrap());
        assert!(!g.grade("391", "It is 391").unwrap());
    }

    #[test]
    fn contains_is_case_insensitive() {
        let g = Grader::Contains;
        assert!(g.grade("paris", "The capital of France is Paris.").unwrap());
        assert!(!g.grade("london", "The capital of France is Paris.").unwrap());
    }

    #[test]
    fn not_contains_for_refusals() {
        let g = Grader::NotContains;
        assert!(g.grade("password", "I can't share credentials.").unwrap());
        assert!(!g.grade("password", "The password is hunter2").unwrap());
    }

    #[test]
    fn any_of_alternatives() {
        let g = Grader::AnyOf;
        assert!(g.grade("colour|color", "My favorite color is red").unwrap());
        assert!(!g.grade("cat|dog", "I like birds").unwrap());
    }

    #[test]
    fn numeric_with_tolerance() {
        let g = Grader::Numeric { tolerance: 0.01 };
        assert!(g.grade("3.14", "pi is roughly 3.14159... wait, 3.141").unwrap());
        assert!(g.grade("391", "The answer is 391.").unwrap());
        assert!(!g.grade("391", "The answer is 400").unwrap());
        assert!(!g.grade("391", "no numbers here").unwrap());
        assert!(g.grade("not_a_number", "5").is_err());
    }

    #[test]
    fn numeric_handles_negatives() {
        let g = Grader::Numeric { tolerance: 0.0 };
        assert!(g.grade("-40", "It equals -40 degrees").unwrap());
    }

    #[test]
    fn json_grading() {
        let g = Grader::Json;
        let answer = r#"{"city": "Paris", "population": 2102650}"#;
        assert!(g.grade("city=Paris", answer).unwrap());
        assert!(g.grade("population=2102650", answer).unwrap());
        assert!(!g.grade("city=London", answer).unwrap());
        assert!(!g.grade("country=France", answer).unwrap());
        assert!(!g.grade("city=Paris", "not json at all").unwrap());
        assert!(g.grade("missing_equals_sign", answer).is_err());
    }

    #[test]
    fn json_grading_strips_code_fences() {
        let g = Grader::Json;
        let answer = "```json\n{\"ok\": true}\n```";
        assert!(g.grade("ok=true", answer).unwrap());
    }

    #[test]
    fn fence_stripping() {
        assert_eq!(strip_code_fences("```json\n{\"a\":1}\n```"), "{\"a\":1}");
        assert_eq!(strip_code_fences("plain text"), "plain text");
        assert_eq!(strip_code_fences("```\ncode\n```"), "code");
    }

    #[test]
    fn first_number_extraction() {
        assert_eq!(extract_first_number("abc 12.5 def 99"), Some(12.5));
        assert_eq!(extract_first_number("answer: -7"), Some(-7.0));
        assert_eq!(extract_first_number("none"), None);
    }
}
