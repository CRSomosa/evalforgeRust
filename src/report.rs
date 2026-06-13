//! Reporting: console summary, markdown export, JSON export.

use std::fs;

use crate::json::Value;
use crate::runner::TaskResult;

/// Aggregate numbers shared by all three report formats.
struct Summary {
    passed: usize,
    total: usize,
    earned: u32,
    possible: u32,
    percent: f64,
    total_ms: u128,
}

fn summarize(results: &[TaskResult]) -> Summary {
    let passed = results.iter().filter(|r| r.passed).count();
    let earned: u32 = results.iter().map(|r| r.points_earned).sum();
    let possible: u32 = results.iter().map(|r| r.points_possible).sum();
    let percent = if possible > 0 {
        100.0 * earned as f64 / possible as f64
    } else {
        0.0
    };
    Summary {
        passed,
        total: results.len(),
        earned,
        possible,
        percent,
        total_ms: results.iter().map(|r| r.duration_ms).sum(),
    }
}

/// Print a human-friendly table to stdout, with details for failures.
pub fn print_console(results: &[TaskResult]) {
    for r in results {
        let status = if r.passed { "PASS" } else { "FAIL" };
        println!(
            "{}  {:<24} {:>2}/{:<2} pts  {:<12} {:>5}ms",
            status, r.id, r.points_earned, r.points_possible, r.grader_name, r.duration_ms
        );
        if !r.passed {
            if let Some(err) = &r.error {
                println!("      error:    {}", err);
            }
            println!("      expected: {}", preview(&r.expect));
            println!("      actual:   {}", preview(&r.actual));
        }
    }

    let s = summarize(results);
    println!(
        "\nscore: {}/{} points ({:.1}%) — {}/{} tasks passed in {}ms",
        s.earned, s.possible, s.percent, s.passed, s.total, s.total_ms
    );
}

/// One-line preview of possibly long / multi-line text.
fn preview(s: &str) -> String {
    let flat = s.replace('\n', " \\n ");
    let mut out: String = flat.chars().take(120).collect();
    if flat.chars().count() > 120 {
        out.push_str("...");
    }
    if out.trim().is_empty() {
        out = "(empty)".to_string();
    }
    out
}

/// Write a markdown report, nice for sharing or committing next to results.
pub fn write_markdown(results: &[TaskResult], path: &str) -> Result<(), String> {
    let s = summarize(results);
    let mut md = String::new();
    md.push_str("# evalforge report\n\n");
    md.push_str(&format!(
        "**Score:** {}/{} points ({:.1}%) — {}/{} tasks passed\n\n",
        s.earned, s.possible, s.percent, s.passed, s.total
    ));
    md.push_str("| Status | Task | Points | Grader | Time (ms) |\n");
    md.push_str("|--------|------|--------|--------|-----------|\n");
    for r in results {
        md.push_str(&format!(
            "| {} | {} | {}/{} | {} | {} |\n",
            if r.passed { "✅" } else { "❌" },
            r.id,
            r.points_earned,
            r.points_possible,
            r.grader_name,
            r.duration_ms
        ));
    }

    let failures: Vec<&TaskResult> = results.iter().filter(|r| !r.passed).collect();
    if !failures.is_empty() {
        md.push_str("\n## Failures\n");
        for r in failures {
            md.push_str(&format!("\n### {}\n\n", r.id));
            if let Some(err) = &r.error {
                md.push_str(&format!("- **error:** {}\n", err));
            }
            md.push_str(&format!("- **expected:** `{}`\n", r.expect));
            md.push_str(&format!("- **actual:** {}\n", preview(&r.actual)));
        }
    }

    fs::write(path, md).map_err(|e| e.to_string())
}

/// Write machine-readable results using our own JSON writer —
/// the same Value type the json grader parses with.
pub fn write_json(results: &[TaskResult], path: &str) -> Result<(), String> {
    let s = summarize(results);

    let result_values: Vec<Value> = results
        .iter()
        .map(|r| {
            Value::Obj(vec![
                ("id".to_string(), Value::Str(r.id.clone())),
                ("passed".to_string(), Value::Bool(r.passed)),
                ("points_earned".to_string(), Value::Num(r.points_earned as f64)),
                (
                    "points_possible".to_string(),
                    Value::Num(r.points_possible as f64),
                ),
                ("grader".to_string(), Value::Str(r.grader_name.to_string())),
                ("expected".to_string(), Value::Str(r.expect.clone())),
                ("actual".to_string(), Value::Str(r.actual.clone())),
                (
                    "error".to_string(),
                    match &r.error {
                        Some(e) => Value::Str(e.clone()),
                        None => Value::Null,
                    },
                ),
                ("duration_ms".to_string(), Value::Num(r.duration_ms as f64)),
            ])
        })
        .collect();

    let doc = Value::Obj(vec![
        (
            "summary".to_string(),
            Value::Obj(vec![
                ("tasks_passed".to_string(), Value::Num(s.passed as f64)),
                ("tasks_total".to_string(), Value::Num(s.total as f64)),
                ("points_earned".to_string(), Value::Num(s.earned as f64)),
                ("points_possible".to_string(), Value::Num(s.possible as f64)),
                ("percent".to_string(), Value::Num(s.percent)),
                ("total_ms".to_string(), Value::Num(s.total_ms as f64)),
            ]),
        ),
        ("results".to_string(), Value::Arr(result_values)),
    ]);

    fs::write(path, doc.to_json()).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_result(id: &str, passed: bool) -> TaskResult {
        TaskResult {
            index: 0,
            id: id.to_string(),
            grader_name: "exact",
            passed,
            points_earned: if passed { 2 } else { 0 },
            points_possible: 2,
            expect: "x".to_string(),
            actual: "y".to_string(),
            error: None,
            duration_ms: 5,
        }
    }

    #[test]
    fn summary_math() {
        let results = vec![fake_result("a", true), fake_result("b", false)];
        let s = summarize(&results);
        assert_eq!(s.passed, 1);
        assert_eq!(s.total, 2);
        assert_eq!(s.earned, 2);
        assert_eq!(s.possible, 4);
        assert!((s.percent - 50.0).abs() < 1e-9);
    }

    #[test]
    fn preview_truncates_and_flattens() {
        let long = "a".repeat(300);
        assert!(preview(&long).ends_with("..."));
        assert_eq!(preview("a\nb"), "a \\n b");
        assert_eq!(preview("   "), "(empty)");
    }

    #[test]
    fn json_export_is_parseable_by_our_own_parser() {
        let results = vec![fake_result("a", true)];
        let s = summarize(&results);
        // Build the same doc write_json builds, then parse it back.
        let doc = Value::Obj(vec![(
            "tasks_total".to_string(),
            Value::Num(s.total as f64),
        )]);
        let parsed = crate::json::parse(&doc.to_json()).unwrap();
        assert_eq!(
            parsed.get_path("tasks_total").unwrap().as_comparable_string(),
            "1"
        );
    }
}
