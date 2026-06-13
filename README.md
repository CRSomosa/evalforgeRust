# evalforge

A small, zero-dependency LLM evaluation harness written in Rust.

You write evaluation tasks in a plain-text `.eval` file — a prompt, an
expectation, and a grading strategy. evalforge sends each prompt to a model,
grades the answer, and reports a score, in parallel, with markdown and JSON
exports.

```
PASS  arithmetic_chain          2/2  pts  numeric          0ms
PASS  capital_city              1/1  pts  contains         0ms
FAIL  tricky_capital            0/1  pts  contains         0ms
      expected: Ankara
      actual:   Istanbul
...
score: 10/12 points (83.3%) — 7/9 tasks passed in 2ms
```

## Why this project

Evaluating generative models is mostly *not* about calling an API — it's about
the unglamorous machinery around it: defining tasks precisely, grading fuzzy
natural-language output fairly, running suites fast, and reporting results in
a way both humans and CI pipelines can consume. evalforge implements that
machinery from scratch.

Design choices worth noting:

- **Zero dependencies.** Everything — the task-file parser, a complete JSON
  parser/writer, CLI argument handling, and a multi-threaded work queue — is
  built on the standard library. `cargo build` finishes in seconds and there
  is no supply chain to audit.
- **Realistic grading.** LLMs wrap answers in code fences, add pleasantries,
  and format numbers inconsistently. Graders strip markdown fences, compare
  case-insensitively where appropriate, extract numbers from prose, and
  validate structured JSON output by path (`user.address.city=Paris`).
- **Model-agnostic.** The harness talks to a `Provider` trait. The built-in
  `cmd` provider pipes prompts through any shell command, so it works with
  Ollama, the `llm` CLI, or a 5-line API script — no HTTP code needed here.
- **Parallel by default.** A lock-free atomic counter hands out tasks to a
  pool of scoped threads; results are re-sorted into file order afterwards.

## Quickstart

Requires Rust (install from https://rustup.rs).

```bash
cargo test               # run the unit test suite (~30 tests)
cargo run -- run tasks/sample.eval          # demo with the offline mock provider
cargo run -- list tasks/sample.eval         # just list the tasks
```

The sample suite uses scripted `mock_response` answers (two deliberately
wrong) so you can see passing and failing output without any model.

### Evaluating a real model

Any command that reads a prompt on stdin and prints a completion works:

```bash
# Local model via Ollama
cargo run -- run tasks/sample.eval --provider cmd --cmd "ollama run llama3.2"

# Any API, via a tiny script you control
cargo run -- run tasks/sample.eval --provider cmd --cmd "python ask_model.py"
```

`ask_model.py` can be as simple as: read stdin, call your favorite API,
print the text. The harness stays vendor-neutral.

### Other options

```
--threads <n>       parallel workers (default 4)
--report out.md     write a markdown report
--json out.json     write machine-readable results
```

The process exits with code 2 if any task failed, so `evalforge run` drops
straight into CI.

## Task file format

```
[task]
id: arithmetic_chain                  # unique name
prompt: Compute (17 * 23) - (12 * 11). Reply with only the final number.
grader: numeric                       # how to judge the answer
expect: 259                           # what to judge against
points: 2                             # weight (default 1)
```

Multi-line prompts use `|` blocks:

```
prompt: |
Return a JSON object with one key "year".
Return ONLY the JSON object.
|
```

### Graders

| Grader         | Passes when…                                                  |
|----------------|---------------------------------------------------------------|
| `exact`        | answer equals expectation (trimmed)                           |
| `contains`     | expectation appears in the answer (case-insensitive)          |
| `not_contains` | expectation does NOT appear — great for redaction/refusals    |
| `any_of`       | answer contains one of several `\|`-separated alternatives    |
| `numeric`      | first number found in the answer is within `tolerance`        |
| `json`         | answer parses as JSON and `path.to.field=value` holds         |

## Architecture

```
main.rs      CLI: arg parsing, wiring, exit codes
task.rs      .eval file format + parser
provider.rs  Provider trait, MockProvider, CommandProvider
runner.rs    parallel execution (scoped threads + atomic work queue)
grader.rs    six grading strategies + answer normalization
json.rs      hand-written recursive-descent JSON parser & writer
report.rs    console / markdown / JSON reporting
```

Each module carries its own unit tests (`cargo test`).

## Possible extensions

- An HTTP provider (would be the first dependency — reqwest)
- Model-graded evals: use a second model as the judge
- Per-task retries and majority voting
- Comparing two providers side by side in one report
