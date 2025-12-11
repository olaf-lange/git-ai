use crate::authorship::authorship_log::PromptRecord;
use crate::error::GitAiError;
use crate::git::find_repository;
use crate::git::refs::{get_authorship, grep_ai_notes};
use crate::git::repository::Repository;

/// Handle the `show-prompt` command
///
/// Usage: git-ai show-prompt <prompt_id> [--commit <rev>] [--offset <n>]
///
/// Returns the prompt object from the authorship note where the given prompt ID is found.
/// By default returns from the most recent commit containing the prompt.
pub fn handle_show_prompt(args: &[String]) {
    let parsed = match parse_args(args) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    };

    let repo = match find_repository(&Vec::<String>::new()) {
        Ok(repo) => repo,
        Err(e) => {
            eprintln!("Failed to find repository: {}", e);
            std::process::exit(1);
        }
    };

    match find_prompt(
        &repo,
        &parsed.prompt_id,
        parsed.commit.as_deref(),
        parsed.offset,
    ) {
        Ok((commit_sha, prompt_record)) => {
            // Output the prompt as JSON, including the commit SHA for context
            let output = serde_json::json!({
                "commit": commit_sha,
                "prompt_id": parsed.prompt_id,
                "prompt": prompt_record,
            });
            println!(
                "{}",
                serde_json::to_string_pretty(&output).unwrap_or_else(|_| "{}".to_string())
            );
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    }
}

#[derive(Debug)]
pub struct ParsedArgs {
    pub prompt_id: String,
    pub commit: Option<String>,
    pub offset: usize,
}

pub fn parse_args(args: &[String]) -> Result<ParsedArgs, String> {
    let mut prompt_id: Option<String> = None;
    let mut commit: Option<String> = None;
    let mut offset: Option<usize> = None;

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];

        if arg == "--commit" {
            if i + 1 >= args.len() {
                return Err("--commit requires a value".to_string());
            }
            i += 1;
            commit = Some(args[i].clone());
        } else if arg == "--offset" {
            if i + 1 >= args.len() {
                return Err("--offset requires a value".to_string());
            }
            i += 1;
            offset = Some(
                args[i]
                    .parse::<usize>()
                    .map_err(|_| "--offset must be a non-negative integer")?,
            );
        } else if arg.starts_with('-') {
            return Err(format!("Unknown option: {}", arg));
        } else {
            if prompt_id.is_some() {
                return Err("Only one prompt ID can be specified".to_string());
            }
            prompt_id = Some(arg.clone());
        }

        i += 1;
    }

    let prompt_id = prompt_id.ok_or("show-prompt requires a prompt ID")?;

    // Validate mutual exclusivity of --commit and --offset
    if commit.is_some() && offset.is_some() {
        return Err("--commit and --offset are mutually exclusive".to_string());
    }

    Ok(ParsedArgs {
        prompt_id,
        commit,
        offset: offset.unwrap_or(0),
    })
}

/// Find a prompt in the repository history
///
/// If `commit` is provided, look only in that specific commit.
/// Otherwise, search through history and skip `offset` occurrences (0 = most recent).
fn find_prompt(
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
fn find_prompt_in_commit(
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
fn find_prompt_in_history(
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
