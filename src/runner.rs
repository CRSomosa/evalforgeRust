//! The runner: execute every task against the provider, in parallel.
//!
//! Concurrency design (all std-lib, no async runtime needed):
//!   - an AtomicUsize hands out task indexes — a lock-free work queue.
//!     Fast tasks don't wait for slow ones; each worker grabs the next
//!     index the moment it finishes the previous task.
//!   - results go into a Mutex<Vec<...>> and are sorted back into file
//!     order at the end.
//!   - std::thread::scope guarantees the workers finish before we return,
//!     which is what lets them safely borrow `tasks` and `provider`
//!     without any Arc or cloning.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use crate::provider::Provider;
use crate::task::Task;

/// Everything we know about one finished task.
#[derive(Debug, Clone)]
pub struct TaskResult {
    pub index: usize, // position in the file, for stable ordering
    pub id: String,
    pub grader_name: &'static str,
    pub passed: bool,
    pub points_earned: u32,
    pub points_possible: u32,
    pub expect: String,
    pub actual: String,
    /// Set when the provider or grader itself failed (not a wrong answer).
    pub error: Option<String>,
    pub duration_ms: u128,
}

/// Run every task and return results in file order.
pub fn run_all(tasks: &[Task], provider: &dyn Provider, threads: usize) -> Vec<TaskResult> {
    let next_index = AtomicUsize::new(0);
    let results: Mutex<Vec<TaskResult>> = Mutex::new(Vec::with_capacity(tasks.len()));

    // Never spawn more workers than there are tasks.
    let worker_count = threads.min(tasks.len()).max(1);

    std::thread::scope(|scope| {
        for _ in 0..worker_count {
            scope.spawn(|| {
                loop {
                    // Atomically claim the next unclaimed task.
                    let i = next_index.fetch_add(1, Ordering::SeqCst);
                    if i >= tasks.len() {
                        break; // queue exhausted -> this worker is done
                    }
                    let result = run_one(i, &tasks[i], provider);
                    results.lock().unwrap().push(result);
                }
            });
        }
    }); // <- scope blocks here until every worker has finished

    let mut all = results.into_inner().unwrap();
    all.sort_by_key(|r| r.index);
    all
}

/// Execute and grade a single task, timing the provider call.
fn run_one(index: usize, task: &Task, provider: &dyn Provider) -> TaskResult {
    let started = Instant::now();
    let completion = provider.complete(task);
    let duration_ms = started.elapsed().as_millis();

    let (passed, actual, error) = match completion {
        Ok(answer) => match task.grader.grade(&task.expect, &answer) {
            Ok(ok) => (ok, answer, None),
            Err(grading_error) => (false, answer, Some(grading_error)),
        },
        Err(provider_error) => (false, String::new(), Some(provider_error)),
    };

    TaskResult {
        index,
        id: task.id.clone(),
        grader_name: task.grader.name(),
        passed,
        points_earned: if passed { task.points } else { 0 },
        points_possible: task.points,
        expect: task.expect.clone(),
        actual,
        error,
        duration_ms,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::MockProvider;
    use crate::task::parse_tasks;

    const SUITE: &str = "\
[task]
id: pass_me
prompt: say ok
expect: ok
mock_response: ok

[task]
id: fail_me
prompt: say ok
expect: ok
mock_response: definitely not

[task]
id: numbers
prompt: 17 * 23?
grader: numeric
expect: 391
mock_response: The answer is 391.
";

    #[test]
    fn runs_a_suite_and_keeps_order() {
        let tasks = parse_tasks(SUITE).unwrap();
        let results = run_all(&tasks, &MockProvider, 8);
        assert_eq!(results.len(), 3);
        // Order must match the file even with 8 threads racing.
        assert_eq!(results[0].id, "pass_me");
        assert_eq!(results[1].id, "fail_me");
        assert_eq!(results[2].id, "numbers");
        assert!(results[0].passed);
        assert!(!results[1].passed);
        assert!(results[2].passed);
    }

    #[test]
    fn single_thread_also_works() {
        let tasks = parse_tasks(SUITE).unwrap();
        let results = run_all(&tasks, &MockProvider, 1);
        assert_eq!(results.iter().filter(|r| r.passed).count(), 2);
    }

    #[test]
    fn points_are_tallied_per_task() {
        let tasks = parse_tasks(SUITE).unwrap();
        let results = run_all(&tasks, &MockProvider, 4);
        let earned: u32 = results.iter().map(|r| r.points_earned).sum();
        let possible: u32 = results.iter().map(|r| r.points_possible).sum();
        assert_eq!(earned, 2);
        assert_eq!(possible, 3);
    }
}
