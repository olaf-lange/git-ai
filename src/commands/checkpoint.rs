use crate::authorship::working_log::{Checkpoint, Line, WorkingLogEntry};
use crate::commands::checkpoint_agent::agent_preset::AgentRunResult;
use crate::error::GitAiError;
use crate::git::repo_storage::{PersistedWorkingLog, RepoStorage};
use crate::git::repository::Repository;
use crate::git::status::{EntryKind, StatusCode};
use crate::utils::debug_log;
use sha2::{Digest, Sha256};
use similar::{ChangeTag, TextDiff};
use std::collections::{HashMap, HashSet};

pub fn run(
    repo: &Repository,
    author: &str,
    show_working_log: bool,
    reset: bool,
    quiet: bool,
    agent_run_result: Option<AgentRunResult>,
    focus_on_working_log_files: bool,
) -> Result<(usize, usize, usize), GitAiError> {
    // Robustly handle zero-commit repos
    let base_commit = match repo.head() {
        Ok(head) => match head.target() {
            Ok(oid) => oid,
            Err(_) => "initial".to_string(),
        },
        Err(_) => "initial".to_string(),
    };

    // Cannot run checkpoint on bare repositories
    if repo.workdir().is_err() {
        eprintln!("Cannot run checkpoint on bare repositories");
        return Err(GitAiError::Generic(
            "Cannot run checkpoint on bare repositories".to_string(),
        ));
    }

    // Initialize the new storage system
    let repo_storage = RepoStorage::for_repo_path(repo.path());
    let working_log_storage = repo_storage.working_log_for_base_commit(&base_commit);

    let working_log = working_log_storage.read_all_checkpoints()?;
    let previously_edited_files = working_log.edited_files;

    // If focus_on_working_log_files is true, use previously_edited_files as pathspecs
    let pathspecs = if focus_on_working_log_files && !previously_edited_files.is_empty() {
        Some(previously_edited_files.clone())
    } else {
        None
    };

    let files =
        get_all_tracked_files(repo, &base_commit, &working_log_storage, pathspecs.as_ref())?;
    let mut checkpoints = if reset {
        // If reset flag is set, start with an empty working log
        working_log_storage.reset_working_log()?;
        Vec::new()
    } else {
        working_log.checkpoints
    };

    if show_working_log {
        if checkpoints.is_empty() {
            debug_log("No working log entries found.");
        } else {
            debug_log("Working Log Entries:");
            debug_log(&format!("{}", "=".repeat(80)));
            for (i, checkpoint) in checkpoints.iter().enumerate() {
                debug_log(&format!("Checkpoint {}", i + 1));
                debug_log(&format!("  Diff: {}", checkpoint.diff));
                debug_log(&format!("  Author: {}", checkpoint.author));
                debug_log(&format!(
                    "  Agent ID: {}",
                    checkpoint
                        .agent_id
                        .as_ref()
                        .map(|id| id.tool.clone())
                        .unwrap_or_default()
                ));

                // Display first user message from transcript if available
                if let Some(transcript) = &checkpoint.transcript {
                    if let Some(first_message) = transcript.messages().first() {
                        if let crate::authorship::transcript::Message::User { text, .. } =
                            first_message
                        {
                            let agent_info = checkpoint
                                .agent_id
                                .as_ref()
                                .map(|id| format!(" (Agent: {})", id.tool))
                                .unwrap_or_default();
                            let message_count = transcript.messages().len();
                            debug_log(&format!(
                                "  First message{} ({} messages): {}",
                                agent_info, message_count, text
                            ));
                        }
                    }
                }

                debug_log("  Entries:");
                for entry in &checkpoint.entries {
                    debug_log(&format!("    File: {}", entry.file));
                    debug_log(&format!("    Added lines: {:?}", entry.added_lines));
                    debug_log(&format!("    Deleted lines: {:?}", entry.deleted_lines));
                }
                debug_log("");
            }
        }
        return Ok((0, files.len(), checkpoints.len()));
    }

    // Save current file states and get content hashes
    let file_content_hashes = save_current_file_states(&working_log_storage, &files)?;

    // Order file hashes by key and create a hash of the ordered hashes
    let mut ordered_hashes: Vec<_> = file_content_hashes.iter().collect();
    ordered_hashes.sort_by_key(|(file_path, _)| *file_path);

    let mut combined_hasher = Sha256::new();
    for (file_path, hash) in ordered_hashes {
        combined_hasher.update(file_path.as_bytes());
        combined_hasher.update(hash.as_bytes());
    }
    let combined_hash = format!("{:x}", combined_hasher.finalize());

    // If this is not the first checkpoint, diff against the last saved state
    let entries = if checkpoints.is_empty() || reset {
        // First checkpoint or reset - diff against base commit
        get_initial_checkpoint_entries(repo, &files, &base_commit, &file_content_hashes)?
    } else {
        // Subsequent checkpoint - diff against last saved state
        get_subsequent_checkpoint_entries(
            &working_log_storage,
            &files,
            &file_content_hashes,
            checkpoints.last(),
        )?
    };

    // Skip adding checkpoint if there are no changes
    if !entries.is_empty() {
        let mut checkpoint =
            Checkpoint::new(combined_hash.clone(), author.to_string(), entries.clone());

        // Set transcript and agent_id if provided
        if let Some(agent_run) = &agent_run_result {
            checkpoint.transcript = Some(agent_run.transcript.clone().unwrap_or_default());
            checkpoint.agent_id = Some(agent_run.agent_id.clone());
        }

        // Append checkpoint to the working log
        working_log_storage.append_checkpoint(&checkpoint)?;
        checkpoints.push(checkpoint);
    }

    let agent_tool = if let Some(agent_run_result) = &agent_run_result {
        Some(agent_run_result.agent_id.tool.as_str())
    } else {
        None
    };

    // Print summary with new format
    if reset {
        debug_log("Working log reset. Starting fresh checkpoint.");
    }

    let label = if entries.len() > 1 {
        "checkpoint"
    } else {
        "commit"
    };

    if !quiet {
        let log_author = agent_tool.unwrap_or(author);
        // Only count files that actually have checkpoint entries to avoid confusion.
        // Files that were previously checkpointed but have no new changes won't have entries.
        let files_with_entries = entries.len();
        let total_uncommitted_files = files.len();

        if files_with_entries == total_uncommitted_files {
            // All files with changes got entries
            eprintln!(
                "{}{} changed {} file(s) that have changed since the last {}",
                if agent_run_result.is_some() {
                    "AI: "
                } else {
                    "Human: "
                },
                log_author,
                files_with_entries,
                label
            );
        } else {
            // Some files were already checkpointed
            eprintln!(
                "{}{} changed {} of the {} file(s) that have changed since the last {} ({} already checkpointed)",
                if agent_run_result.is_some() {
                    "AI: "
                } else {
                    "Human: "
                },
                log_author,
                files_with_entries,
                total_uncommitted_files,
                label,
                total_uncommitted_files - files_with_entries
            );
        }
    }

    // Return the requested values: (entries_len, files_len, working_log_len)
    Ok((entries.len(), files.len(), checkpoints.len()))
}

fn get_all_files(
    repo: &Repository,
    pathspecs: Option<&HashSet<String>>,
) -> Result<Vec<String>, GitAiError> {
    let mut files = Vec::new();

    // Use porcelain v2 format to get status
    let statuses = repo.status(pathspecs)?;

    for entry in statuses {
        // Skip ignored files
        if entry.kind == EntryKind::Ignored {
            continue;
        }

        // Skip unmerged/conflicted files - we'll track them once the conflict is resolved
        if entry.kind == EntryKind::Unmerged {
            continue;
        }

        // Include files that have any change (staged or unstaged) or are untracked
        let has_change = entry.staged != StatusCode::Unmodified
            || entry.unstaged != StatusCode::Unmodified
            || entry.kind == EntryKind::Untracked;

        if has_change {
            // For deleted files, check if they were text files in HEAD
            let is_deleted =
                entry.staged == StatusCode::Deleted || entry.unstaged == StatusCode::Deleted;

            let is_text = if is_deleted {
                is_text_file_in_head(repo, &entry.path)
            } else {
                is_text_file(repo, &entry.path)
            };

            if is_text {
                files.push(entry.path.clone());
            }
        }
    }

    Ok(files)
}

/// Get all files that should be tracked, including those from previous checkpoints
fn get_all_tracked_files(
    repo: &Repository,
    _base_commit: &str,
    working_log: &PersistedWorkingLog,
    pathspecs: Option<&HashSet<String>>,
) -> Result<Vec<String>, GitAiError> {
    let mut files = get_all_files(repo, pathspecs)?;

    // Also include files that were in previous checkpoints but might not show up in git status
    // This ensures we track deletions when files return to their original state
    if let Ok(working_log_data) = working_log.read_all_checkpoints() {
        for checkpoint in &working_log_data.checkpoints {
            for entry in &checkpoint.entries {
                // If pathspecs are provided, only include files that match the pathspecs
                let should_include = if let Some(pathspecs) = pathspecs {
                    pathspecs.contains(&entry.file)
                } else {
                    true
                };

                if should_include && !files.contains(&entry.file) {
                    // Check if it's a text file before adding
                    if is_text_file(repo, &entry.file) {
                        files.push(entry.file.clone());
                    }
                }
            }
        }
    }

    Ok(files)
}

fn save_current_file_states(
    working_log: &PersistedWorkingLog,
    files: &[String],
) -> Result<HashMap<String, String>, GitAiError> {
    let mut file_content_hashes = HashMap::new();

    for file_path in files {
        let abs_path = working_log.repo_root.join(file_path);
        let content = if abs_path.exists() {
            // Read file as bytes first, then convert to string with UTF-8 lossy conversion
            match std::fs::read(&abs_path) {
                Ok(bytes) => String::from_utf8_lossy(&bytes).to_string(),
                Err(_) => String::new(), // If we can't read the file, treat as empty
            }
        } else {
            String::new()
        };

        // Persist the file content and get the content hash
        let content_hash = working_log.persist_file_version(&content)?;
        file_content_hashes.insert(file_path.clone(), content_hash);
    }

    Ok(file_content_hashes)
}

fn get_initial_checkpoint_entries(
    repo: &Repository,
    files: &[String],
    _base_commit: &str,
    file_content_hashes: &HashMap<String, String>,
) -> Result<Vec<WorkingLogEntry>, GitAiError> {
    let mut entries = Vec::new();

    // Diff working directory against HEAD tree for each file
    let head_commit = repo
        .head()
        .ok()
        .and_then(|h| h.target().ok())
        .and_then(|oid| repo.find_commit(oid).ok());
    let head_tree = head_commit.as_ref().and_then(|c| c.tree().ok());

    for file_path in files {
        let repo_workdir = repo.workdir().unwrap();
        let abs_path = repo_workdir.join(file_path);

        // Previous content from HEAD tree if present, otherwise empty
        let previous_content = if let Some(tree) = &head_tree {
            match tree.get_path(std::path::Path::new(file_path)) {
                Ok(entry) => {
                    if let Ok(blob) = repo.find_blob(entry.id()) {
                        let blob_content = blob.content()?;
                        String::from_utf8_lossy(&blob_content).to_string()
                    } else {
                        String::new()
                    }
                }
                Err(_) => String::new(),
            }
        } else {
            String::new()
        };

        // Current content from filesystem
        let current_content = std::fs::read_to_string(&abs_path).unwrap_or_else(|_| String::new());

        // Normalize trailing newlines to avoid spurious inserts
        let prev_norm = if previous_content.ends_with('\n') {
            previous_content.clone()
        } else {
            format!("{}\n", previous_content)
        };
        let curr_norm = if current_content.ends_with('\n') {
            current_content.clone()
        } else {
            format!("{}\n", current_content)
        };

        let diff = TextDiff::from_lines(&prev_norm, &curr_norm);
        let mut added_line_numbers = Vec::new();
        let mut deleted_line_numbers = Vec::new();
        let mut current_line = 1u32;

        let mut deletions_at_current_line = 0u32;

        for change in diff.iter_all_changes() {
            match change.tag() {
                ChangeTag::Equal => {
                    current_line += change.value().lines().count() as u32;
                    deletions_at_current_line = 0; // Reset deletion counter when we hit non-deleted content
                }
                ChangeTag::Delete => {
                    let delete_start = current_line + deletions_at_current_line;
                    let delete_count = change.value().lines().count() as u32;
                    // Collect individual line numbers for consolidation
                    for i in 0..delete_count {
                        deleted_line_numbers.push(delete_start + i);
                    }
                    deletions_at_current_line += delete_count;
                    // Don't advance current_line for deletions - insertions will happen at the same position
                }
                ChangeTag::Insert => {
                    let insert_start = current_line;
                    let insert_count = change.value().lines().count() as u32;
                    // Collect individual line numbers for consolidation
                    for i in 0..insert_count {
                        added_line_numbers.push(insert_start + i);
                    }
                    current_line += insert_count;
                    deletions_at_current_line = 0; // Reset deletion counter after insertions
                }
            }
        }

        // Consolidate consecutive lines into ranges
        let added_lines = consolidate_lines(added_line_numbers);
        let deleted_lines = consolidate_lines(deleted_line_numbers);

        if !added_lines.is_empty() || !deleted_lines.is_empty() {
            // Get the blob SHA for this file from the pre-computed hashes
            let blob_sha = file_content_hashes
                .get(file_path)
                .cloned()
                .unwrap_or_default();

            entries.push(WorkingLogEntry::new(
                file_path.clone(),
                blob_sha,
                added_lines,
                deleted_lines,
            ));
        }
    }

    Ok(entries)
}

fn get_subsequent_checkpoint_entries(
    working_log: &PersistedWorkingLog,
    files: &[String],
    file_content_hashes: &HashMap<String, String>,
    previous_checkpoint: Option<&Checkpoint>,
) -> Result<Vec<WorkingLogEntry>, GitAiError> {
    let mut entries = Vec::new();

    // Build a map of file path -> blob_sha from the previous checkpoint's entries
    let previous_file_hashes: HashMap<String, String> =
        if let Some(prev_checkpoint) = previous_checkpoint {
            prev_checkpoint
                .entries
                .iter()
                .map(|entry| (entry.file.clone(), entry.blob_sha.clone()))
                .collect()
        } else {
            HashMap::new()
        };

    for file_path in files {
        let abs_path = working_log.repo_root.join(file_path);

        // Read current content directly from the file system
        let current_content = std::fs::read_to_string(&abs_path).unwrap_or_else(|_| String::new());

        // Read the previous content from the blob storage using the previous checkpoint's blob_sha
        let previous_content = if let Some(prev_content_hash) = previous_file_hashes.get(file_path)
        {
            working_log
                .get_file_version(prev_content_hash)
                .unwrap_or_default()
        } else {
            String::new() // No previous version, treat as empty
        };

        // Normalize by ensuring trailing newline to avoid off-by-one when appending lines
        let prev_norm = if previous_content.ends_with('\n') {
            previous_content.clone()
        } else {
            format!("{}\n", previous_content)
        };
        let curr_norm = if current_content.ends_with('\n') {
            current_content.clone()
        } else {
            format!("{}\n", current_content)
        };

        let diff = TextDiff::from_lines(&prev_norm, &curr_norm);
        let mut added_line_numbers = Vec::new();
        let mut deleted_line_numbers = Vec::new();
        let mut current_line = 1u32;

        let mut deletions_at_current_line = 0u32;

        for change in diff.iter_all_changes() {
            match change.tag() {
                ChangeTag::Equal => {
                    current_line += change.value().lines().count() as u32;
                    deletions_at_current_line = 0; // Reset deletion counter when we hit non-deleted content
                }
                ChangeTag::Delete => {
                    let delete_start = current_line + deletions_at_current_line;
                    let delete_count = change.value().lines().count() as u32;
                    // Collect individual line numbers for consolidation
                    for i in 0..delete_count {
                        deleted_line_numbers.push(delete_start + i);
                    }
                    deletions_at_current_line += delete_count;
                    // Don't advance current_line for deletions - insertions will happen at the same position
                }
                ChangeTag::Insert => {
                    let insert_start = current_line;
                    let insert_count = change.value().lines().count() as u32;
                    // Collect individual line numbers for consolidation
                    for i in 0..insert_count {
                        added_line_numbers.push(insert_start + i);
                    }
                    current_line += insert_count;
                    deletions_at_current_line = 0; // Reset deletion counter after insertions
                }
            }
        }

        // Consolidate consecutive lines into ranges
        let added_lines = consolidate_lines(added_line_numbers);
        let deleted_lines = consolidate_lines(deleted_line_numbers);

        if !added_lines.is_empty() || !deleted_lines.is_empty() {
            // Get the blob SHA for this file from the pre-computed hashes
            let blob_sha = file_content_hashes
                .get(file_path)
                .cloned()
                .unwrap_or_default();

            entries.push(WorkingLogEntry::new(
                file_path.clone(),
                blob_sha,
                added_lines,
                deleted_lines,
            ));
        }
    }

    Ok(entries)
}

/// Consolidate consecutive line numbers into ranges for efficiency
fn consolidate_lines(mut lines: Vec<u32>) -> Vec<Line> {
    if lines.is_empty() {
        return Vec::new();
    }

    // Sort lines to ensure proper consolidation
    lines.sort_unstable();
    lines.dedup(); // Remove duplicates

    let mut consolidated = Vec::new();
    let mut start = lines[0];
    let mut end = lines[0];

    for &line in lines.iter().skip(1) {
        if line == end + 1 {
            // Consecutive line, extend the range
            end = line;
        } else {
            // Gap found, save the current range and start a new one
            if start == end {
                consolidated.push(Line::Single(start));
            } else {
                consolidated.push(Line::Range(start, end));
            }
            start = line;
            end = line;
        }
    }

    // Add the final range
    if start == end {
        consolidated.push(Line::Single(start));
    } else {
        consolidated.push(Line::Range(start, end));
    }

    consolidated
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::authorship::working_log::Line;
    use crate::git::test_utils::TmpRepo;

    #[test]
    fn test_consolidate_lines() {
        // Test consecutive lines
        let lines = vec![1, 2, 3, 4];
        let result = consolidate_lines(lines);
        assert_eq!(result, vec![Line::Range(1, 4)]);

        // Test single line
        let lines = vec![5];
        let result = consolidate_lines(lines);
        assert_eq!(result, vec![Line::Single(5)]);

        // Test mixed consecutive and single
        let lines = vec![1, 2, 5, 6, 7, 10];
        let result = consolidate_lines(lines);
        assert_eq!(
            result,
            vec![Line::Range(1, 2), Line::Range(5, 7), Line::Single(10)]
        );

        // Test unsorted input
        let lines = vec![5, 1, 3, 2, 4];
        let result = consolidate_lines(lines);
        assert_eq!(result, vec![Line::Range(1, 5)]);

        // Test duplicates
        let lines = vec![1, 1, 2, 2, 3];
        let result = consolidate_lines(lines);
        assert_eq!(result, vec![Line::Range(1, 3)]);

        // Test empty input
        let lines = vec![];
        let result = consolidate_lines(lines);
        assert_eq!(result, vec![]);
    }

    #[test]
    fn test_checkpoint_with_staged_changes() {
        // Create a repo with an initial commit
        let (tmp_repo, mut file, _) = TmpRepo::new_with_base_commit().unwrap();

        // Make changes to the file
        file.append("New line added by user\n").unwrap();

        // Note: TmpFile.append() automatically stages changes (see write_to_disk in test_utils)
        // So at this point, the file has staged changes

        // Run checkpoint - it should track the changes even though they're staged
        let (entries_len, files_len, _checkpoints_len) =
            tmp_repo.trigger_checkpoint_with_author("Aidan").unwrap();

        // The bug: when changes are staged, entries_len is 0 instead of 1
        assert_eq!(files_len, 1, "Should have 1 file with changes");
        assert_eq!(
            entries_len, 1,
            "Should have 1 file entry in checkpoint (staged changes should be tracked)"
        );
    }

    #[test]
    fn test_checkpoint_with_unstaged_changes() {
        // Create a repo with an initial commit
        let (tmp_repo, file, _) = TmpRepo::new_with_base_commit().unwrap();

        // Make changes to the file BUT keep them unstaged
        // We need to manually write to the file without staging
        let file_path = file.path();
        let mut current_content = std::fs::read_to_string(&file_path).unwrap();
        current_content.push_str("New line added by user\n");
        std::fs::write(&file_path, current_content).unwrap();

        // Run checkpoint - it should track the unstaged changes
        let (entries_len, files_len, _checkpoints_len) =
            tmp_repo.trigger_checkpoint_with_author("Aidan").unwrap();

        // This should work correctly
        assert_eq!(files_len, 1, "Should have 1 file with changes");
        assert_eq!(entries_len, 1, "Should have 1 file entry in checkpoint");
    }

    #[test]
    fn test_checkpoint_with_staged_changes_after_previous_checkpoint() {
        // Create a repo with an initial commit
        let (tmp_repo, mut file, _) = TmpRepo::new_with_base_commit().unwrap();

        // Make first changes and checkpoint
        file.append("First change\n").unwrap();
        let (entries_len_1, files_len_1, _) =
            tmp_repo.trigger_checkpoint_with_author("Aidan").unwrap();

        assert_eq!(
            files_len_1, 1,
            "First checkpoint: should have 1 file with changes"
        );
        assert_eq!(
            entries_len_1, 1,
            "First checkpoint: should have 1 file entry"
        );

        // Make second changes - these are already staged by append()
        file.append("Second change\n").unwrap();

        // Run checkpoint again - it should track the staged changes even after a previous checkpoint
        let (entries_len_2, files_len_2, _) =
            tmp_repo.trigger_checkpoint_with_author("Aidan").unwrap();

        // The bug might show up here
        println!(
            "Second checkpoint: entries_len={}, files_len={}",
            entries_len_2, files_len_2
        );
        assert_eq!(
            files_len_2, 1,
            "Second checkpoint: should have 1 file with changes"
        );
        assert_eq!(
            entries_len_2, 1,
            "Second checkpoint: should have 1 file entry in checkpoint (staged changes should be tracked)"
        );
    }

    #[test]
    fn test_checkpoint_with_only_staged_no_unstaged_changes() {
        use std::fs;

        // Create a repo with an initial commit
        let (tmp_repo, file, _) = TmpRepo::new_with_base_commit().unwrap();

        // Get the file path
        let file_path = file.path();
        let filename = file.filename();

        // Manually modify the file (bypassing TmpFile's automatic staging)
        let mut content = fs::read_to_string(&file_path).unwrap();
        content.push_str("New line for staging test\n");
        fs::write(&file_path, &content).unwrap();

        // Now manually stage it using git (this is what "git add" does)
        tmp_repo.stage_file(filename).unwrap();

        // At this point: HEAD has old content, index has new content, workdir has new content
        // And unstaged should be "Unmodified" because workdir == index

        // Now run checkpoint
        let (entries_len, files_len, _checkpoints_len) =
            tmp_repo.trigger_checkpoint_with_author("Aidan").unwrap();

        println!(
            "Checkpoint result: entries_len={}, files_len={}",
            entries_len, files_len
        );

        // This should work: we should see 1 file with 1 entry
        assert_eq!(files_len, 1, "Should detect 1 file with staged changes");
        assert_eq!(
            entries_len, 1,
            "Should track the staged changes in checkpoint"
        );
    }

    #[test]
    fn test_checkpoint_then_stage_then_checkpoint_again() {
        use std::fs;

        // Create a repo with an initial commit
        let (tmp_repo, file, _) = TmpRepo::new_with_base_commit().unwrap();

        // Get the file path
        let file_path = file.path();
        let filename = file.filename();

        // Step 1: Manually modify the file WITHOUT staging
        let mut content = fs::read_to_string(&file_path).unwrap();
        content.push_str("New line added\n");
        fs::write(&file_path, &content).unwrap();

        // Step 2: Checkpoint the unstaged changes
        let (entries_len_1, files_len_1, _) =
            tmp_repo.trigger_checkpoint_with_author("Aidan").unwrap();

        println!(
            "First checkpoint (unstaged): entries_len={}, files_len={}",
            entries_len_1, files_len_1
        );
        assert_eq!(files_len_1, 1, "First checkpoint: should detect 1 file");
        assert_eq!(entries_len_1, 1, "First checkpoint: should create 1 entry");

        // Step 3: Now stage the file (without making any new changes)
        tmp_repo.stage_file(filename).unwrap();

        // Step 4: Try to checkpoint again - the file is now staged but content hasn't changed
        let (entries_len_2, files_len_2, _) =
            tmp_repo.trigger_checkpoint_with_author("Aidan").unwrap();

        println!(
            "Second checkpoint (staged, no new changes): entries_len={}, files_len={}",
            entries_len_2, files_len_2
        );

        // After the fix: The checkpoint correctly recognizes that the file was already checkpointed
        // and doesn't create a duplicate entry. The improved message clarifies this to the user:
        // "changed 0 of the 1 file(s) that have changed since the last commit (1 already checkpointed)"
        assert_eq!(files_len_2, 1, "Second checkpoint: file is still staged");
        assert_eq!(
            entries_len_2, 0,
            "Second checkpoint: no NEW changes, so no new entry (already checkpointed)"
        );
    }

    #[test]
    fn test_checkpoint_skips_conflicted_files() {
        // Create a repo with an initial commit
        let (tmp_repo, mut file, _) = TmpRepo::new_with_base_commit().unwrap();

        // Get the current branch name (whatever the default is)
        let base_branch = tmp_repo.current_branch().unwrap();

        // Create a branch and make different changes on each branch to create a conflict
        tmp_repo.create_branch("feature-branch").unwrap();

        // On feature branch, modify the file
        file.append("Feature branch change\n").unwrap();
        tmp_repo
            .trigger_checkpoint_with_author("FeatureUser")
            .unwrap();
        tmp_repo.commit_with_message("Feature commit").unwrap();

        // Switch back to base branch and make conflicting changes
        tmp_repo.switch_branch(&base_branch).unwrap();
        file.append("Main branch change\n").unwrap();
        tmp_repo.trigger_checkpoint_with_author("MainUser").unwrap();
        tmp_repo.commit_with_message("Main commit").unwrap();

        // Attempt to merge feature-branch into base branch - this should create a conflict
        let has_conflicts = tmp_repo.merge_with_conflicts("feature-branch").unwrap();
        assert!(has_conflicts, "Should have merge conflicts");

        // Try to checkpoint while there are conflicts
        let (entries_len, files_len, _) = tmp_repo.trigger_checkpoint_with_author("Human").unwrap();

        // Checkpoint should skip conflicted files
        assert_eq!(
            files_len, 0,
            "Should have 0 files (conflicted file should be skipped)"
        );
        assert_eq!(
            entries_len, 0,
            "Should have 0 entries (conflicted file should be skipped)"
        );
    }

    #[test]
    fn test_checkpoint_works_after_conflict_resolution_maintains_authorship() {
        // Create a repo with an initial commit
        let (tmp_repo, mut file, _) = TmpRepo::new_with_base_commit().unwrap();

        // Get the current branch name (whatever the default is)
        let base_branch = tmp_repo.current_branch().unwrap();

        // Checkpoint initial state to track the base authorship
        let file_path = file.path();
        let initial_content = std::fs::read_to_string(&file_path).unwrap();
        println!("Initial content:\n{}", initial_content);

        // Create a branch and make changes
        tmp_repo.create_branch("feature-branch").unwrap();
        file.append("Feature line 1\n").unwrap();
        file.append("Feature line 2\n").unwrap();
        tmp_repo.trigger_checkpoint_with_author("AI_Agent").unwrap();
        tmp_repo.commit_with_message("Feature commit").unwrap();

        // Switch back to base branch and make conflicting changes
        tmp_repo.switch_branch(&base_branch).unwrap();
        file.append("Main line 1\n").unwrap();
        file.append("Main line 2\n").unwrap();
        tmp_repo.trigger_checkpoint_with_author("Human").unwrap();
        tmp_repo.commit_with_message("Main commit").unwrap();

        // Attempt to merge feature-branch into base branch - this should create a conflict
        let has_conflicts = tmp_repo.merge_with_conflicts("feature-branch").unwrap();
        assert!(has_conflicts, "Should have merge conflicts");

        // While there are conflicts, checkpoint should skip the file
        let (entries_len_conflict, files_len_conflict, _) =
            tmp_repo.trigger_checkpoint_with_author("Human").unwrap();
        assert_eq!(
            files_len_conflict, 0,
            "Should skip conflicted files during conflict"
        );
        assert_eq!(
            entries_len_conflict, 0,
            "Should not create entries for conflicted files"
        );

        // Resolve the conflict by choosing "ours" (base branch)
        tmp_repo.resolve_conflict(file.filename(), "ours").unwrap();

        // Verify content to ensure the resolution was applied correctly
        let resolved_content = std::fs::read_to_string(&file_path).unwrap();
        println!("Resolved content after resolution:\n{}", resolved_content);
        assert!(
            resolved_content.contains("Main line 1"),
            "Should contain base branch content (we chose 'ours')"
        );
        assert!(
            resolved_content.contains("Main line 2"),
            "Should contain base branch content (we chose 'ours')"
        );
        assert!(
            !resolved_content.contains("Feature line 1"),
            "Should not contain feature branch content (we chose 'ours')"
        );

        // After resolution, make additional changes to test that checkpointing works again
        file.append("Post-resolution line 1\n").unwrap();
        file.append("Post-resolution line 2\n").unwrap();

        // Now checkpoint should work and track the new changes
        let (entries_len_after, files_len_after, _) =
            tmp_repo.trigger_checkpoint_with_author("Human").unwrap();

        println!(
            "After resolution and new changes: entries_len={}, files_len={}",
            entries_len_after, files_len_after
        );

        // The file should be tracked with the new changes
        assert_eq!(
            files_len_after, 1,
            "Should detect 1 file with new changes after conflict resolution"
        );
        assert_eq!(
            entries_len_after, 1,
            "Should create 1 entry for new changes after conflict resolution"
        );
    }
}

fn is_text_file(repo: &Repository, path: &str) -> bool {
    let repo_workdir = repo.workdir().unwrap();
    let abs_path = repo_workdir.join(path);

    if let Ok(metadata) = std::fs::metadata(&abs_path) {
        if !metadata.is_file() {
            return false;
        }
    } else {
        return false; // If metadata can't be read, treat as non-text
    }

    if let Ok(content) = std::fs::read(&abs_path) {
        // Consider a file text if it contains no null bytes
        !content.contains(&0)
    } else {
        false
    }
}

fn is_text_file_in_head(repo: &Repository, path: &str) -> bool {
    // For deleted files, check if they were text files in HEAD
    let head_commit = match repo
        .head()
        .ok()
        .and_then(|h| h.target().ok())
        .and_then(|oid| repo.find_commit(oid).ok())
    {
        Some(commit) => commit,
        None => return false,
    };

    let head_tree = match head_commit.tree().ok() {
        Some(tree) => tree,
        None => return false,
    };

    match head_tree.get_path(std::path::Path::new(path)) {
        Ok(entry) => {
            if let Ok(blob) = repo.find_blob(entry.id()) {
                // Consider a file text if it contains no null bytes
                let blob_content = match blob.content() {
                    Ok(content) => content,
                    Err(_) => return false,
                };
                !blob_content.contains(&0)
            } else {
                false
            }
        }
        Err(_) => false,
    }
}
