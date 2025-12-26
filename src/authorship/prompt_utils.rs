use crate::authorship::authorship_log::PromptRecord;
use crate::authorship::internal_db::InternalDatabase;
use crate::error::GitAiError;
use crate::git::refs::{get_authorship, grep_ai_notes};
use crate::git::repository::Repository;

/// Find a prompt in the repository history
///
/// If `commit` is provided, look only in that specific commit.
/// Otherwise, search through history and skip `offset` occurrences (0 = most recent).
pub fn find_prompt(
    repo: &Repository,
    prompt_id: &str,
    commit: Option<&str>,
    offset: usize,
) -> Result<(String, PromptRecord), GitAiError> {
    if let Some(commit_rev) = commit {
        // Look in specific commit
        find_prompt_in_commit(repo, prompt_id, commit_rev)
    } else {
        // Search through history with offset
        find_prompt_in_history(repo, prompt_id, offset)
    }
}

/// Find a prompt in a specific commit
pub fn find_prompt_in_commit(
    repo: &Repository,
    prompt_id: &str,
    commit_rev: &str,
) -> Result<(String, PromptRecord), GitAiError> {
    // Resolve the revision to a commit SHA
    let commit = repo.revparse_single(commit_rev)?;
    let commit_sha = commit.id();

    // Get the authorship log for this commit
    let authorship_log = get_authorship(repo, &commit_sha).ok_or_else(|| {
        GitAiError::Generic(format!(
            "No authorship data found for commit: {}",
            commit_rev
        ))
    })?;

    // Look for the prompt in the log
    authorship_log
        .metadata
        .prompts
        .get(prompt_id)
        .map(|prompt| (commit_sha, prompt.clone()))
        .ok_or_else(|| {
            GitAiError::Generic(format!(
                "Prompt '{}' not found in commit {}",
                prompt_id, commit_rev
            ))
        })
}

/// Find a prompt in history, skipping `offset` occurrences
/// Returns the (N+1)th occurrence where N = offset (0 = most recent)
pub fn find_prompt_in_history(
    repo: &Repository,
    prompt_id: &str,
    offset: usize,
) -> Result<(String, PromptRecord), GitAiError> {
    // Use git grep to search for the prompt ID in authorship notes
    // grep_ai_notes returns commits sorted by date (newest first)
    let shas = grep_ai_notes(repo, &format!("\"{}\"", prompt_id)).unwrap_or_default();

    if shas.is_empty() {
        return Err(GitAiError::Generic(format!(
            "Prompt not found in history: {}",
            prompt_id
        )));
    }

    // Iterate through commits, looking for the prompt and counting occurrences
    let mut found_count = 0;
    for sha in &shas {
        if let Some(authorship_log) = get_authorship(repo, sha) {
            if let Some(prompt) = authorship_log.metadata.prompts.get(prompt_id) {
                if found_count == offset {
                    return Ok((sha.clone(), prompt.clone()));
                }
                found_count += 1;
            }
        }
    }

    // If we get here, we didn't find enough occurrences
    if found_count == 0 {
        Err(GitAiError::Generic(format!(
            "Prompt not found in history: {}",
            prompt_id
        )))
    } else {
        Err(GitAiError::Generic(format!(
            "Prompt '{}' found {} time(s), but offset {} requested (max offset: {})",
            prompt_id,
            found_count,
            offset,
            found_count - 1
        )))
    }
}

/// Find a prompt, trying the database first, then falling back to repository if provided
///
/// Returns `(Option<commit_sha>, PromptRecord)` where commit_sha is None if found in DB
/// and Some(sha) if found in repository.
pub fn find_prompt_with_db_fallback(
    prompt_id: &str,
    repo: Option<&Repository>,
) -> Result<(Option<String>, PromptRecord), GitAiError> {
    // First, try to get from database
    let db = InternalDatabase::global()?;
    let db_guard = db
        .lock()
        .map_err(|e| GitAiError::Generic(format!("Failed to lock database: {}", e)))?;

    if let Some(db_record) = db_guard.get_prompt(prompt_id)? {
        // Convert PromptDbRecord to PromptRecord
        let prompt_record = db_record.to_prompt_record();
        return Ok((db_record.commit_sha, prompt_record));
    }

    // Not found in DB, try repository if provided
    if let Some(repo) = repo {
        // Try to find in history (most recent occurrence)
        match find_prompt_in_history(repo, prompt_id, 0) {
            Ok((commit_sha, prompt)) => Ok((Some(commit_sha), prompt)),
            Err(_) => Err(GitAiError::Generic(format!(
                "Prompt '{}' not found in database or repository",
                prompt_id
            ))),
        }
    } else {
        Err(GitAiError::Generic(format!(
            "Prompt '{}' not found in database and no repository provided",
            prompt_id
        )))
    }
}
