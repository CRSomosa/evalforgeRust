//! The `.eval` task file format and its parser.
//!
//! The format is deliberately simple — line-based, human-writable,
//! no external parser needed. Example:
//!
//! ```text
//! # Comments start with '#'. Blank lines are ignored.
//!
//! [task]
//! id: arith_01
//! prompt: What is 17 * 23? Answer with just the number.
//! grader: numeric
//! expect: 391
//! points: 2
//!
//! [task]
//! id: multi_line_example
//! prompt: |
//! Summarize this text in one word:
//! "The quick brown fox jumps over the lazy dog."
//! |
//! grader: contains
//! expect: fox
//! ```
//!
//! Recognized keys:
//!   id             (required)  unique name for the task
//!   prompt         (required)  the text sent to the model;
//!                              use `|` on its own to start a multi-line
//!                              block ended by a line containing only `|`
//!   expect         (required)  the expectation, interpreted by the grader
//!   grader         (optional)  exact | contains | not_contains | any_of
//!                              | numeric | json       (default: exact)
//!   points         (optional)  weight of this task     (default: 1)
//!   tolerance      (optional)  numeric grader slack    (default: 0.001)
//!   mock_response  (optional)  what the mock provider should answer;
//!                              lets you test the harness offline

use crate::grader::Grader;

#[derive(Debug, Clone)]
pub struct Task {
    pub id: String,
    pub prompt: String,
    pub grader: Grader,
    pub expect: String,
    pub points: u32,
    pub mock_response: Option<String>,
}

/// Parse an entire `.eval` file into tasks.
pub fn parse_tasks(source: &str) -> Result<Vec<Task>, String> {
    let lines: Vec<&str> = source.lines().collect();

    // Phase 1: split the file into blocks of raw key/value pairs.
    // Each `[task]` header starts a new block.
    let mut blocks: Vec<Vec<(String, String, usize)>> = Vec::new(); // (key, value, line_no)
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i].trim();
        let line_no = i + 1; // humans count lines from 1

        if line.is_empty() || line.starts_with('#') {
            i += 1;
            continue;
        }

        if line == "[task]" {
            blocks.push(Vec::new());
            i += 1;
            continue;
        }

        // Everything else must be `key: value` inside a task block.
        let (key, value) = line
            .split_once(':')
            .ok_or_else(|| format!("line {}: expected `key: value`, got `{}`", line_no, line))?;
        let key = key.trim().to_string();
        let mut value = value.trim().to_string();

        // Multi-line block: `key: |` followed by lines until a lone `|`.
        if value == "|" {
            let mut body = Vec::new();
            i += 1;
            loop {
                if i >= lines.len() {
                    return Err(format!(
                        "line {}: multi-line block for `{}` never closed with `|`",
                        line_no, key
                    ));
                }
                if lines[i].trim() == "|" {
                    break;
                }
                body.push(lines[i]);
                i += 1;
            }
            value = body.join("\n");
        }

        match blocks.last_mut() {
            Some(block) => block.push((key, value, line_no)),
            None => {
                return Err(format!(
                    "line {}: `{}` appears before any [task] header",
                    line_no, key
                ))
            }
        }
        i += 1;
    }

    // Phase 2: turn each raw block into a validated Task.
    let mut tasks = Vec::new();
    for block in &blocks {
        tasks.push(block_to_task(block)?);
    }

    // Phase 3: ids must be unique, otherwise reports get confusing.
    for (a, task) in tasks.iter().enumerate() {
        for other in &tasks[a + 1..] {
            if task.id == other.id {
                return Err(format!("duplicate task id `{}`", task.id));
            }
        }
    }

    Ok(tasks)
}

fn block_to_task(block: &[(String, String, usize)]) -> Result<Task, String> {
    let mut id = None;
    let mut prompt = None;
    let mut expect = None;
    let mut grader_name = "exact".to_string();
    let mut points = 1u32;
    let mut tolerance = 0.001f64;
    let mut mock_response = None;

    for (key, value, line_no) in block {
        match key.as_str() {
            "id" => id = Some(value.clone()),
            "prompt" => prompt = Some(value.clone()),
            "expect" => expect = Some(value.clone()),
            "grader" => grader_name = value.clone(),
            "mock_response" => mock_response = Some(value.clone()),
            "points" => {
                points = value.parse().map_err(|_| {
                    format!("line {}: points must be a positive integer", line_no)
                })?;
            }
            "tolerance" => {
                tolerance = value.parse().map_err(|_| {
                    format!("line {}: tolerance must be a number", line_no)
                })?;
            }
            other => {
                return Err(format!("line {}: unknown key `{}`", line_no, other));
            }
        }
    }

    let id = id.ok_or("a task is missing its `id`")?;
    let prompt = prompt.ok_or_else(|| format!("task `{}` is missing `prompt`", id))?;
    let expect = expect.ok_or_else(|| format!("task `{}` is missing `expect`", id))?;
    let grader = Grader::from_name(&grader_name, tolerance)
        .map_err(|e| format!("task `{}`: {}", id, e))?;

    if prompt.trim().is_empty() {
        return Err(format!("task `{}` has an empty prompt", id));
    }
    if points == 0 {
        return Err(format!("task `{}`: points must be at least 1", id));
    }

    Ok(Task {
        id,
        prompt,
        grader,
        expect,
        points,
        mock_response,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
# a comment
[task]
id: t1
prompt: What is 2+2?
grader: numeric
expect: 4
points: 3

[task]
id: t2
prompt: |
line one
line two
|
expect: something
";

    #[test]
    fn parses_basic_file() {
        let tasks = parse_tasks(SAMPLE).unwrap();
        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0].id, "t1");
        assert_eq!(tasks[0].points, 3);
        assert_eq!(tasks[0].grader.name(), "numeric");
        assert_eq!(tasks[1].prompt, "line one\nline two");
        assert_eq!(tasks[1].grader.name(), "exact"); // the default
        assert_eq!(tasks[1].points, 1); // the default
    }

    #[test]
    fn rejects_missing_fields() {
        assert!(parse_tasks("[task]\nid: x\nexpect: y\n").is_err()); // no prompt
        assert!(parse_tasks("[task]\nprompt: p\nexpect: y\n").is_err()); // no id
        assert!(parse_tasks("[task]\nid: x\nprompt: p\n").is_err()); // no expect
    }

    #[test]
    fn rejects_duplicate_ids() {
        let src = "[task]\nid: a\nprompt: p\nexpect: e\n[task]\nid: a\nprompt: p\nexpect: e\n";
        assert!(parse_tasks(src).is_err());
    }

    #[test]
    fn rejects_keys_outside_tasks() {
        assert!(parse_tasks("id: orphan\n").is_err());
    }

    #[test]
    fn rejects_unclosed_multiline() {
        assert!(parse_tasks("[task]\nid: x\nprompt: |\nnever closed").is_err());
    }

    #[test]
    fn rejects_unknown_grader() {
        let src = "[task]\nid: a\nprompt: p\ngrader: vibes\nexpect: e\n";
        assert!(parse_tasks(src).is_err());
    }
}
