use crate::authorship::authorship_log::PromptRecord;
use crate::authorship::authorship_log_serialization::AuthorshipLog;
use crate::error::GitAiError;
use crate::git::refs::get_reference_as_authorship_log_v3;
use crate::git::repository::{Repository, exec_git};
use std::collections::HashMap;
use std::io::IsTerminal;

// ============================================================================
// Data Structures
// ============================================================================

#[derive(Debug)]
pub enum DiffSpec {
    SingleCommit(String),      // SHA
    TwoCommit(String, String), // start..end
}

#[derive(Debug)]
pub struct DiffHunk {
    pub file_path: String,
    pub old_start: u32,
    pub old_count: u32,
    pub new_start: u32,
    pub new_count: u32,
    pub deleted_lines: Vec<u32>, // Absolute line numbers in OLD file
    pub added_lines: Vec<u32>,   // Absolute line numbers in NEW file
}

#[derive(Debug, Hash, Eq, PartialEq, Clone)]
pub struct DiffLineKey {
    pub file: String,
    pub line: u32,
    pub side: LineSide,
}

#[derive(Debug, Hash, Eq, PartialEq, Clone)]
pub enum LineSide {
    Old, // For deleted lines
    New, // For added lines
}

#[derive(Debug, Clone)]
pub enum Attribution {
    Ai(String),    // Tool name: "cursor", "claude", etc.
    Human(String), // Username
    NoData,        // No authorship data available
}

// ============================================================================
// Main Entry Point
// ============================================================================

pub fn handle_diff(repo: &Repository, args: &[String]) -> Result<(), GitAiError> {
    if args.is_empty() {
        eprintln!("Error: diff requires a commit or commit range argument");
        eprintln!("Usage: git-ai diff <commit>");
        eprintln!("       git-ai diff <commit1>..<commit2>");
        std::process::exit(1);
    }

    let spec = parse_diff_args(args)?;
    execute_diff(repo, spec)?;

    Ok(())
}

// ============================================================================
// Argument Parsing
// ============================================================================

pub fn parse_diff_args(args: &[String]) -> Result<DiffSpec, GitAiError> {
    let arg = &args[0];

    // Check for commit range (start..end)
    if arg.contains("..") {
        let parts: Vec<&str> = arg.split("..").collect();
        if parts.len() == 2 && !parts[0].is_empty() && !parts[1].is_empty() {
            return Ok(DiffSpec::TwoCommit(
                parts[0].to_string(),
                parts[1].to_string(),
            ));
        } else {
            return Err(GitAiError::Generic(
                "Invalid commit range format. Expected: <commit>..<commit>".to_string(),
            ));
        }
    }

    // Single commit
    Ok(DiffSpec::SingleCommit(arg.to_string()))
}

// ============================================================================
// Core Execution Logic
// ============================================================================

pub fn execute_diff(repo: &Repository, spec: DiffSpec) -> Result<(), GitAiError> {
    // Resolve commits to get from/to SHAs
    let (from_commit, to_commit) = match spec {
        DiffSpec::TwoCommit(start, end) => {
            // Resolve both commits
            let from = resolve_commit(repo, &start)?;
            let to = resolve_commit(repo, &end)?;
            (from, to)
        }
        DiffSpec::SingleCommit(commit) => {
            // Resolve the commit and its parent
            let to = resolve_commit(repo, &commit)?;
            let from = resolve_parent(repo, &to)?;
            (from, to)
        }
    };

    // Step 1: Get diff hunks with line numbers
    let hunks = get_diff_with_line_numbers(repo, &from_commit, &to_commit)?;

    // Step 2: Overlay AI attributions
    let attributions = overlay_diff_attributions(repo, &from_commit, &to_commit, &hunks)?;

    // Step 3: Format and output annotated diff
    format_annotated_diff(repo, &from_commit, &to_commit, &attributions)?;

    Ok(())
}

// ============================================================================
// Commit Resolution
// ============================================================================

fn resolve_commit(repo: &Repository, rev: &str) -> Result<String, GitAiError> {
    let mut args = repo.global_args_for_exec();
    args.push("rev-parse".to_string());
    args.push(rev.to_string());

    let output = exec_git(&args)?;
    let sha = String::from_utf8(output.stdout)
        .map_err(|e| GitAiError::Generic(format!("Failed to parse rev-parse output: {}", e)))?
        .trim()
        .to_string();

    if sha.is_empty() {
        return Err(GitAiError::Generic(format!(
            "Could not resolve commit: {}",
            rev
        )));
    }

    Ok(sha)
}

fn resolve_parent(repo: &Repository, commit: &str) -> Result<String, GitAiError> {
    let parent_rev = format!("{}^", commit);

    // Try to resolve parent
    let mut args = repo.global_args_for_exec();
    args.push("rev-parse".to_string());
    args.push(parent_rev);

    let output = exec_git(&args);

    match output {
        Ok(out) => {
            let sha = String::from_utf8(out.stdout)
                .map_err(|e| GitAiError::Generic(format!("Failed to parse parent SHA: {}", e)))?
                .trim()
                .to_string();

            if sha.is_empty() {
                // No parent, this is initial commit - use empty tree
                Ok("4b825dc642cb6eb9a060e54bf8d69288fbee4904".to_string())
            } else {
                Ok(sha)
            }
        }
        Err(_) => {
            // No parent, this is initial commit - use empty tree hash
            Ok("4b825dc642cb6eb9a060e54bf8d69288fbee4904".to_string())
        }
    }
}

// ============================================================================
// Diff Retrieval with Line Numbers
// ============================================================================

pub fn get_diff_with_line_numbers(
    repo: &Repository,
    from: &str,
    to: &str,
) -> Result<Vec<DiffHunk>, GitAiError> {
    let mut args = repo.global_args_for_exec();
    args.push("diff".to_string());
    args.push("-U0".to_string()); // No context lines, just changes
    args.push("--no-color".to_string());
    args.push(from.to_string());
    args.push(to.to_string());

    let output = exec_git(&args)?;
    let diff_text = String::from_utf8(output.stdout)
        .map_err(|e| GitAiError::Generic(format!("Failed to parse diff output: {}", e)))?;

    parse_diff_hunks(&diff_text)
}

fn parse_diff_hunks(diff_text: &str) -> Result<Vec<DiffHunk>, GitAiError> {
    let mut hunks = Vec::new();
    let mut current_file = String::new();

    for line in diff_text.lines() {
        if line.starts_with("+++ b/") {
            // New file path
            current_file = line[6..].to_string();
        } else if line.starts_with("@@ ") {
            // Hunk header
            if let Some(hunk) = parse_hunk_line(line, &current_file)? {
                hunks.push(hunk);
            }
        }
    }

    Ok(hunks)
}

fn parse_hunk_line(line: &str, file_path: &str) -> Result<Option<DiffHunk>, GitAiError> {
    // Parse hunk header format: @@ -old_start,old_count +new_start,new_count @@
    // Also handles: @@ -old_start +new_start,new_count @@ (single line deletion)
    // Also handles: @@ -old_start,old_count +new_start @@ (single line addition)

    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() < 3 {
        return Ok(None);
    }

    let old_part = parts[1]; // e.g., "-10,3" or "-10"
    let new_part = parts[2]; // e.g., "+15,5" or "+15"

    // Parse old part
    let (old_start, old_count) = if old_part.starts_with('-') {
        let old_str = &old_part[1..];
        if let Some((start_str, count_str)) = old_str.split_once(',') {
            let start: u32 = start_str.parse().unwrap_or(0);
            let count: u32 = count_str.parse().unwrap_or(0);
            (start, count)
        } else {
            let start: u32 = old_str.parse().unwrap_or(0);
            (start, 1)
        }
    } else {
        (0, 0)
    };

    // Parse new part
    let (new_start, new_count) = if new_part.starts_with('+') {
        let new_str = &new_part[1..];
        if let Some((start_str, count_str)) = new_str.split_once(',') {
            let start: u32 = start_str.parse().unwrap_or(0);
            let count: u32 = count_str.parse().unwrap_or(0);
            (start, count)
        } else {
            let start: u32 = new_str.parse().unwrap_or(0);
            (start, 1)
        }
    } else {
        (0, 0)
    };

    // Build line number lists
    let deleted_lines: Vec<u32> = if old_count > 0 {
        (old_start..old_start + old_count).collect()
    } else {
        Vec::new()
    };

    let added_lines: Vec<u32> = if new_count > 0 {
        (new_start..new_start + new_count).collect()
    } else {
        Vec::new()
    };

    Ok(Some(DiffHunk {
        file_path: file_path.to_string(),
        old_start,
        old_count,
        new_start,
        new_count,
        deleted_lines,
        added_lines,
    }))
}

// ============================================================================
// Attribution Overlay
// ============================================================================

pub fn overlay_diff_attributions(
    repo: &Repository,
    from_commit: &str,
    to_commit: &str,
    hunks: &[DiffHunk],
) -> Result<HashMap<DiffLineKey, Attribution>, GitAiError> {
    let mut attributions = HashMap::new();

    // Cache authorship logs per commit
    let mut old_log_cache: Option<AuthorshipLog> = None;
    let mut new_log_cache: Option<AuthorshipLog> = None;
    let mut foreign_prompts_cache: HashMap<String, Option<PromptRecord>> = HashMap::new();

    // Track which commits we've tried to load
    let mut old_log_loaded = false;
    let mut new_log_loaded = false;

    for hunk in hunks {
        let file = &hunk.file_path;

        // Load authorship log for old commit if needed (for deleted lines)
        if !hunk.deleted_lines.is_empty() && !old_log_loaded {
            old_log_cache = get_reference_as_authorship_log_v3(repo, from_commit).ok();
            old_log_loaded = true;
        }

        // Load authorship log for new commit if needed (for added lines)
        if !hunk.added_lines.is_empty() && !new_log_loaded {
            new_log_cache = get_reference_as_authorship_log_v3(repo, to_commit).ok();
            new_log_loaded = true;
        }

        // Process deleted lines
        for &line_num in &hunk.deleted_lines {
            let attribution = if let Some(ref log) = old_log_cache {
                get_line_attribution(repo, log, file, line_num, &mut foreign_prompts_cache)
            } else {
                Attribution::NoData
            };

            let key = DiffLineKey {
                file: file.clone(),
                line: line_num,
                side: LineSide::Old,
            };
            attributions.insert(key, attribution);
        }

        // Process added lines
        for &line_num in &hunk.added_lines {
            let attribution = if let Some(ref log) = new_log_cache {
                get_line_attribution(repo, log, file, line_num, &mut foreign_prompts_cache)
            } else {
                Attribution::NoData
            };

            let key = DiffLineKey {
                file: file.clone(),
                line: line_num,
                side: LineSide::New,
            };
            attributions.insert(key, attribution);
        }
    }

    Ok(attributions)
}

fn get_line_attribution(
    repo: &Repository,
    log: &AuthorshipLog,
    file: &str,
    line: u32,
    foreign_prompts_cache: &mut HashMap<String, Option<PromptRecord>>,
) -> Attribution {
    if let Some((author, _prompt_hash, prompt)) =
        log.get_line_attribution(repo, file, line, foreign_prompts_cache)
    {
        if let Some(pr) = prompt {
            // AI authorship
            Attribution::Ai(pr.agent_id.tool.clone())
        } else {
            // Human authorship
            Attribution::Human(author.username.clone())
        }
    } else {
        Attribution::NoData
    }
}

// ============================================================================
// Output Formatting
// ============================================================================

pub fn format_annotated_diff(
    repo: &Repository,
    from_commit: &str,
    to_commit: &str,
    attributions: &HashMap<DiffLineKey, Attribution>,
) -> Result<(), GitAiError> {
    // Execute git diff with normal context
    let mut args = repo.global_args_for_exec();
    args.push("diff".to_string());
    args.push("--no-color".to_string());
    args.push(from_commit.to_string());
    args.push(to_commit.to_string());

    let output = exec_git(&args)?;
    let diff_text = String::from_utf8(output.stdout)
        .map_err(|e| GitAiError::Generic(format!("Failed to parse diff output: {}", e)))?;

    // Check if we should use colors
    let use_color = std::io::stdout().is_terminal();

    // Parse and annotate diff
    let mut current_file = String::new();
    let mut old_line_num = 0u32;
    let mut new_line_num = 0u32;

    for line in diff_text.lines() {
        if line.starts_with("diff --git") {
            // Diff header
            print_line(line, LineType::DiffHeader, use_color, None);
            current_file.clear();
            old_line_num = 0;
            new_line_num = 0;
        } else if line.starts_with("index ") {
            print_line(line, LineType::DiffHeader, use_color, None);
        } else if line.starts_with("--- ") {
            print_line(line, LineType::DiffHeader, use_color, None);
        } else if line.starts_with("+++ b/") {
            current_file = line[6..].to_string();
            print_line(line, LineType::DiffHeader, use_color, None);
        } else if line.starts_with("@@ ") {
            // Hunk header - update line counters
            if let Some((old_start, new_start)) = parse_hunk_header_for_line_nums(line) {
                old_line_num = old_start;
                new_line_num = new_start;
            }
            print_line(line, LineType::HunkHeader, use_color, None);
        } else if line.starts_with('-') && !line.starts_with("---") {
            // Deleted line
            let key = DiffLineKey {
                file: current_file.clone(),
                line: old_line_num,
                side: LineSide::Old,
            };
            let attribution = attributions.get(&key);
            print_line(line, LineType::Deletion, use_color, attribution);
            old_line_num += 1;
        } else if line.starts_with('+') && !line.starts_with("+++") {
            // Added line
            let key = DiffLineKey {
                file: current_file.clone(),
                line: new_line_num,
                side: LineSide::New,
            };
            let attribution = attributions.get(&key);
            print_line(line, LineType::Addition, use_color, attribution);
            new_line_num += 1;
        } else if line.starts_with(' ') {
            // Context line
            print_line(line, LineType::Context, use_color, None);
            old_line_num += 1;
            new_line_num += 1;
        } else if line.starts_with("Binary files") {
            // Binary file marker
            print_line(line, LineType::Binary, use_color, None);
        } else {
            // Other lines (e.g., "\ No newline at end of file")
            print_line(line, LineType::Context, use_color, None);
        }
    }

    Ok(())
}

fn parse_hunk_header_for_line_nums(line: &str) -> Option<(u32, u32)> {
    // Parse @@ -old_start,old_count +new_start,new_count @@
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() < 3 {
        return None;
    }

    let old_part = parts[1];
    let new_part = parts[2];

    // Extract old_start
    let old_start = if old_part.starts_with('-') {
        let old_str = &old_part[1..];
        if let Some((start_str, _)) = old_str.split_once(',') {
            start_str.parse::<u32>().ok()?
        } else {
            old_str.parse::<u32>().ok()?
        }
    } else {
        return None;
    };

    // Extract new_start
    let new_start = if new_part.starts_with('+') {
        let new_str = &new_part[1..];
        if let Some((start_str, _)) = new_str.split_once(',') {
            start_str.parse::<u32>().ok()?
        } else {
            new_str.parse::<u32>().ok()?
        }
    } else {
        return None;
    };

    Some((old_start, new_start))
}

#[derive(Debug)]
enum LineType {
    DiffHeader,
    HunkHeader,
    Addition,
    Deletion,
    Context,
    Binary,
}

fn print_line(line: &str, line_type: LineType, use_color: bool, attribution: Option<&Attribution>) {
    let annotation = if let Some(attr) = attribution {
        format_attribution(attr)
    } else {
        String::new()
    };

    if use_color {
        match line_type {
            LineType::DiffHeader => {
                println!("\x1b[1m{}\x1b[0m", line); // Bold
            }
            LineType::HunkHeader => {
                println!("\x1b[36m{}\x1b[0m", line); // Cyan
            }
            LineType::Addition => {
                if annotation.is_empty() {
                    println!("\x1b[32m{}\x1b[0m", line); // Green
                } else {
                    println!("\x1b[32m{}\x1b[0m  \x1b[2m{}\x1b[0m", line, annotation); // Green + dim annotation
                }
            }
            LineType::Deletion => {
                if annotation.is_empty() {
                    println!("\x1b[31m{}\x1b[0m", line); // Red
                } else {
                    println!("\x1b[31m{}\x1b[0m  \x1b[2m{}\x1b[0m", line, annotation); // Red + dim annotation
                }
            }
            LineType::Context | LineType::Binary => {
                println!("{}", line);
            }
        }
    } else {
        // No color
        if annotation.is_empty() {
            println!("{}", line);
        } else {
            println!("{}  {}", line, annotation);
        }
    }
}

fn format_attribution(attribution: &Attribution) -> String {
    match attribution {
        Attribution::Ai(tool) => format!("🤖{}", tool),
        Attribution::Human(username) => format!("👤{}", username),
        Attribution::NoData => "[no-data]".to_string(),
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_diff_args_single_commit() {
        let args = vec!["abc123".to_string()];
        let result = parse_diff_args(&args).unwrap();

        match result {
            DiffSpec::SingleCommit(sha) => {
                assert_eq!(sha, "abc123");
            }
            _ => panic!("Expected SingleCommit"),
        }
    }

    #[test]
    fn test_parse_diff_args_commit_range() {
        let args = vec!["abc123..def456".to_string()];
        let result = parse_diff_args(&args).unwrap();

        match result {
            DiffSpec::TwoCommit(start, end) => {
                assert_eq!(start, "abc123");
                assert_eq!(end, "def456");
            }
            _ => panic!("Expected TwoCommit"),
        }
    }

    #[test]
    fn test_parse_diff_args_invalid_range() {
        let args = vec!["..".to_string()];
        let result = parse_diff_args(&args);
        assert!(result.is_err());

        let args = vec!["abc..".to_string()];
        let result = parse_diff_args(&args);
        assert!(result.is_err());

        let args = vec!["..def".to_string()];
        let result = parse_diff_args(&args);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_hunk_line_basic() {
        let line = "@@ -10,3 +15,5 @@ fn main() {";
        let result = parse_hunk_line(line, "test.rs").unwrap().unwrap();

        assert_eq!(result.file_path, "test.rs");
        assert_eq!(result.old_start, 10);
        assert_eq!(result.old_count, 3);
        assert_eq!(result.new_start, 15);
        assert_eq!(result.new_count, 5);
        assert_eq!(result.deleted_lines, vec![10, 11, 12]);
        assert_eq!(result.added_lines, vec![15, 16, 17, 18, 19]);
    }

    #[test]
    fn test_parse_hunk_line_single_line_deletion() {
        let line = "@@ -10 +10,2 @@ fn main() {";
        let result = parse_hunk_line(line, "test.rs").unwrap().unwrap();

        assert_eq!(result.old_start, 10);
        assert_eq!(result.old_count, 1);
        assert_eq!(result.new_start, 10);
        assert_eq!(result.new_count, 2);
        assert_eq!(result.deleted_lines, vec![10]);
        assert_eq!(result.added_lines, vec![10, 11]);
    }

    #[test]
    fn test_parse_hunk_line_single_line_addition() {
        let line = "@@ -10,2 +10 @@ fn main() {";
        let result = parse_hunk_line(line, "test.rs").unwrap().unwrap();

        assert_eq!(result.old_start, 10);
        assert_eq!(result.old_count, 2);
        assert_eq!(result.new_start, 10);
        assert_eq!(result.new_count, 1);
        assert_eq!(result.deleted_lines, vec![10, 11]);
        assert_eq!(result.added_lines, vec![10]);
    }

    #[test]
    fn test_parse_hunk_line_pure_addition() {
        let line = "@@ -0,0 +1,3 @@ fn main() {";
        let result = parse_hunk_line(line, "test.rs").unwrap().unwrap();

        assert_eq!(result.old_start, 0);
        assert_eq!(result.old_count, 0);
        assert_eq!(result.new_start, 1);
        assert_eq!(result.new_count, 3);
        assert_eq!(result.deleted_lines.len(), 0);
        assert_eq!(result.added_lines, vec![1, 2, 3]);
    }

    #[test]
    fn test_parse_hunk_line_pure_deletion() {
        let line = "@@ -5,3 +0,0 @@ fn main() {";
        let result = parse_hunk_line(line, "test.rs").unwrap().unwrap();

        assert_eq!(result.old_start, 5);
        assert_eq!(result.old_count, 3);
        assert_eq!(result.new_start, 0);
        assert_eq!(result.new_count, 0);
        assert_eq!(result.deleted_lines, vec![5, 6, 7]);
        assert_eq!(result.added_lines.len(), 0);
    }

    #[test]
    fn test_parse_hunk_header_for_line_nums() {
        let line = "@@ -10,5 +20,3 @@ context";
        let result = parse_hunk_header_for_line_nums(line).unwrap();
        assert_eq!(result, (10, 20));
    }

    #[test]
    fn test_parse_hunk_header_for_line_nums_single_line() {
        let line = "@@ -10 +20,3 @@ context";
        let result = parse_hunk_header_for_line_nums(line).unwrap();
        assert_eq!(result, (10, 20));

        let line = "@@ -10,5 +20 @@ context";
        let result = parse_hunk_header_for_line_nums(line).unwrap();
        assert_eq!(result, (10, 20));
    }

    #[test]
    fn test_parse_hunk_header_for_line_nums_invalid() {
        let line = "not a hunk header";
        let result = parse_hunk_header_for_line_nums(line);
        assert!(result.is_none());

        let line = "@@ invalid @@";
        let result = parse_hunk_header_for_line_nums(line);
        assert!(result.is_none());
    }

    #[test]
    fn test_format_attribution_ai() {
        let attr = Attribution::Ai("cursor".to_string());
        assert_eq!(format_attribution(&attr), "🤖cursor");

        let attr = Attribution::Ai("claude".to_string());
        assert_eq!(format_attribution(&attr), "🤖claude");
    }

    #[test]
    fn test_format_attribution_human() {
        let attr = Attribution::Human("alice".to_string());
        assert_eq!(format_attribution(&attr), "👤alice");

        let attr = Attribution::Human("bob@example.com".to_string());
        assert_eq!(format_attribution(&attr), "👤bob@example.com");
    }

    #[test]
    fn test_format_attribution_no_data() {
        let attr = Attribution::NoData;
        assert_eq!(format_attribution(&attr), "[no-data]");
    }

    #[test]
    fn test_diff_line_key_equality() {
        let key1 = DiffLineKey {
            file: "test.rs".to_string(),
            line: 10,
            side: LineSide::Old,
        };

        let key2 = DiffLineKey {
            file: "test.rs".to_string(),
            line: 10,
            side: LineSide::Old,
        };

        let key3 = DiffLineKey {
            file: "test.rs".to_string(),
            line: 10,
            side: LineSide::New,
        };

        assert_eq!(key1, key2);
        assert_ne!(key1, key3);
    }

    #[test]
    fn test_parse_diff_hunks_multiple_files() {
        let diff_text = r#"diff --git a/file1.rs b/file1.rs
index abc123..def456 100644
--- a/file1.rs
+++ b/file1.rs
@@ -10,2 +10,3 @@ fn main() {
diff --git a/file2.rs b/file2.rs
index 111222..333444 100644
--- a/file2.rs
+++ b/file2.rs
@@ -5,1 +5,2 @@ fn test() {
"#;

        let result = parse_diff_hunks(diff_text).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].file_path, "file1.rs");
        assert_eq!(result[1].file_path, "file2.rs");
    }

    #[test]
    fn test_parse_diff_hunks_empty() {
        let diff_text = "";
        let result = parse_diff_hunks(diff_text).unwrap();
        assert_eq!(result.len(), 0);
    }
}
