use crate::git::diff_tree_to_tree::Diff;
use std::collections::HashMap;
use std::time::Instant;

/// Debug logging utility function
///
/// Prints debug messages with a colored prefix when debug assertions are enabled.
/// This function only outputs messages when the code is compiled with debug assertions.
///
/// # Arguments
///
/// * `msg` - The debug message to print
pub fn debug_log(msg: &str) {
    if cfg!(debug_assertions) {
        eprintln!("\x1b[1;33m[git-ai]\x1b[0m {}", msg);
    }
}

/// Print a git diff in a readable format
///
/// Prints the diff between two commits/trees showing which files changed and their status.
/// This is useful for debugging and understanding what changes occurred.
///
/// # Arguments
///
/// * `diff` - The git diff object to print
/// * `old_label` - Label for the "old" side (e.g., commit SHA or description)
/// * `new_label` - Label for the "new" side (e.g., commit SHA or description)
pub fn _print_diff(diff: &Diff, old_label: &str, new_label: &str) {
    println!("Diff between {} and {}:", old_label, new_label);

    let mut file_count = 0;
    for delta in diff.deltas() {
        file_count += 1;
        let old_file = delta.old_file().path().unwrap_or(std::path::Path::new(""));
        let new_file = delta.new_file().path().unwrap_or(std::path::Path::new(""));
        let status = delta.status();

        println!(
            "  File {}: {} -> {} (status: {:?})",
            file_count,
            old_file.display(),
            new_file.display(),
            status
        );
    }

    if file_count == 0 {
        println!("  No changes between {} and {}", old_label, new_label);
    }
}

/// Timer utility for measuring execution time
///
/// Tracks start times for named operations and logs the duration when they complete.
/// Useful for performance debugging and optimization.
///
/// # Example
///
/// ```
/// let mut timing = Timer::new();
/// timing.start("git_commit");
/// // ... do work ...
/// timing.end("git_commit"); // Prints: timer: git_commit took 1.23s
/// ```
pub struct Timer {
    timings: HashMap<String, Instant>,
    enabled: bool,
}

impl Timer {
    /// Create a new Timer instance
    pub fn new() -> Self {
        Timer {
            timings: HashMap::new(),
            enabled: cfg!(debug_assertions) || std::env::var("GIT_AI_TIMER").is_ok(),
        }
    }

    /// Start timing an operation
    ///
    /// # Arguments
    ///
    /// * `key` - A unique identifier for this timing operation
    pub fn start(&mut self, key: &str) {
        // keep this a toy in production
        if self.enabled {
            self.timings.insert(key.to_string(), Instant::now());
        }
    }

    /// End timing an operation and log the duration
    ///
    /// Removes the timing entry and prints the elapsed time in yellow.
    /// If the key doesn't exist (no matching start() call), this is a no-op.
    ///
    /// # Arguments
    ///
    /// * `key` - The identifier used in the corresponding start() call
    pub fn end(&mut self, key: &str) {
        if self.enabled {
            // keep this a toy in production
            if let Some(start_time) = self.timings.remove(key) {
                let duration = start_time.elapsed();
                println!("\x1b[1;33mtimer:\x1b[0m {} took {:?}", key, duration);
            }
        }
    }
}

impl Default for Timer {
    fn default() -> Self {
        Self::new()
    }
}
