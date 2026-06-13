//! evalforge — a tiny LLM evaluation harness written in pure std-lib Rust.
//!
//! The flow is simple:
//!   1. Parse a `.eval` file into a list of `Task`s        (task.rs)
//!   2. Send each task's prompt to a `Provider` (a model)  (provider.rs)
//!   3. Grade the model's answer against the expectation   (grader.rs)
//!   4. Print / export a report                            (report.rs)
//!
//! Tasks run in parallel across a small thread pool        (runner.rs)
//! and a hand-written JSON parser supports structured
//! grading and JSON report export                          (json.rs)

mod grader;
mod json;
mod provider;
mod report;
mod runner;
mod task;

use std::process::exit;

use provider::{CommandProvider, MockProvider, Provider};

fn main() {
    // Skip argv[0] (the program name itself).
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.is_empty() {
        print_usage();
        exit(1);
    }

    match args[0].as_str() {
        "run" => cmd_run(&args[1..]),
        "list" => cmd_list(&args[1..]),
        "help" | "--help" | "-h" => print_usage(),
        other => {
            eprintln!("error: unknown command `{}`\n", other);
            print_usage();
            exit(1);
        }
    }
}

/// Options for the `run` command, filled in by parse_run_args.
struct RunOptions {
    file: String,
    provider: String,         // "mock" or "cmd"
    command: Option<String>,  // shell command used when provider == "cmd"
    threads: usize,
    report_md: Option<String>,  // optional markdown report path
    report_json: Option<String>, // optional JSON results path
}

fn cmd_run(args: &[String]) {
    let opts = parse_run_args(args);

    // 1. Load and parse the task file.
    let source = match std::fs::read_to_string(&opts.file) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: cannot read `{}`: {}", opts.file, e);
            exit(1);
        }
    };
    let tasks = match task::parse_tasks(&source) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("error: {}: {}", opts.file, e);
            exit(1);
        }
    };
    if tasks.is_empty() {
        eprintln!("error: `{}` contains no tasks", opts.file);
        exit(1);
    }

    // 2. Build the provider. Box<dyn Provider> lets us pick the
    //    implementation at runtime while the rest of the code only
    //    talks to the trait.
    let provider: Box<dyn Provider> = match opts.provider.as_str() {
        "mock" => Box::new(MockProvider),
        "cmd" => match &opts.command {
            Some(c) => Box::new(CommandProvider::new(c.clone())),
            None => {
                eprintln!("error: --provider cmd requires --cmd \"<shell command>\"");
                exit(1);
            }
        },
        other => {
            eprintln!("error: unknown provider `{}` (use mock or cmd)", other);
            exit(1);
        }
    };

    println!(
        "evalforge: running {} task(s) with provider `{}` on {} thread(s)\n",
        tasks.len(),
        provider.name(),
        opts.threads
    );

    // 3. Run everything (in parallel) and grade.
    let results = runner::run_all(&tasks, provider.as_ref(), opts.threads);

    // 4. Report.
    report::print_console(&results);

    if let Some(path) = &opts.report_md {
        match report::write_markdown(&results, path) {
            Ok(()) => println!("markdown report written to {}", path),
            Err(e) => eprintln!("error writing markdown report: {}", e),
        }
    }
    if let Some(path) = &opts.report_json {
        match report::write_json(&results, path) {
            Ok(()) => println!("json results written to {}", path),
            Err(e) => eprintln!("error writing json results: {}", e),
        }
    }

    // Exit non-zero if anything failed, so this works in CI pipelines.
    let any_failed = results.iter().any(|r| !r.passed);
    if any_failed {
        exit(2);
    }
}

fn cmd_list(args: &[String]) {
    let file = match args.first() {
        Some(f) => f,
        None => {
            eprintln!("usage: evalforge list <tasks.eval>");
            exit(1);
        }
    };
    let source = match std::fs::read_to_string(file) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: cannot read `{}`: {}", file, e);
            exit(1);
        }
    };
    match task::parse_tasks(&source) {
        Ok(tasks) => {
            println!("{} task(s) in {}:\n", tasks.len(), file);
            for t in &tasks {
                println!(
                    "  {:<20} grader={:<12} points={}",
                    t.id,
                    t.grader.name(),
                    t.points
                );
            }
        }
        Err(e) => {
            eprintln!("error: {}: {}", file, e);
            exit(1);
        }
    }
}

/// Hand-rolled flag parsing. For a project this size a CLI crate
/// would be overkill — and zero dependencies is the whole point.
fn parse_run_args(args: &[String]) -> RunOptions {
    let mut opts = RunOptions {
        file: String::new(),
        provider: "mock".to_string(),
        command: None,
        threads: 4,
        report_md: None,
        report_json: None,
    };

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        // Helper: fetch the value that must follow a flag.
        let mut take_value = |flag: &str| -> String {
            if i + 1 >= args.len() {
                eprintln!("error: {} requires a value", flag);
                exit(1);
            }
            i += 1;
            args[i].clone()
        };

        match arg.as_str() {
            "--provider" => opts.provider = take_value("--provider"),
            "--cmd" => opts.command = Some(take_value("--cmd")),
            "--threads" => {
                let v = take_value("--threads");
                opts.threads = match v.parse::<usize>() {
                    Ok(n) if n >= 1 => n,
                    _ => {
                        eprintln!("error: --threads must be a positive integer");
                        exit(1);
                    }
                };
            }
            "--report" => opts.report_md = Some(take_value("--report")),
            "--json" => opts.report_json = Some(take_value("--json")),
            other if other.starts_with("--") => {
                eprintln!("error: unknown flag `{}`", other);
                exit(1);
            }
            // First bare argument is the task file.
            _ => {
                if opts.file.is_empty() {
                    opts.file = arg.clone();
                } else {
                    eprintln!("error: unexpected argument `{}`", arg);
                    exit(1);
                }
            }
        }
        i += 1;
    }

    if opts.file.is_empty() {
        eprintln!("error: no task file given\n");
        print_usage();
        exit(1);
    }
    opts
}

fn print_usage() {
    println!(
        "evalforge — a zero-dependency LLM evaluation harness

USAGE:
  evalforge run <tasks.eval> [options]
  evalforge list <tasks.eval>

OPTIONS (run):
  --provider <mock|cmd>   model backend (default: mock)
  --cmd \"<command>\"       shell command for the cmd provider;
                          the prompt is piped to its stdin and the
                          completion is read from its stdout
                          e.g. --cmd \"ollama run llama3.2\"
  --threads <n>           parallel workers (default: 4)
  --report <file.md>      also write a markdown report
  --json <file.json>      also write machine-readable JSON results

EXAMPLES:
  evalforge run tasks/sample.eval
  evalforge run tasks/sample.eval --provider cmd --cmd \"ollama run llama3.2\"
  evalforge run tasks/sample.eval --report report.md --json results.json"
    );
}
