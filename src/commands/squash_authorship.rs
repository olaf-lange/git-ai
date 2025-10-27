use crate::authorship::rebase_authorship::prepare_working_log_after_squash;
use crate::git::find_repository_in_path;

pub fn handle_squash_authorship(args: &[String]) {
    // Parse squash-authorship-specific arguments
    let mut new_sha = None;
    let mut old_sha = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--dry-run" => {
                // Dry-run flag is parsed but not used in current implementation
                i += 1;
            }
            _ => {
                // Positional arguments: branch, new_sha, old_sha
                // Note: branch argument kept for CLI compatibility but not used
                if new_sha.is_none() {
                    new_sha = Some(args[i].clone());
                } else if old_sha.is_none() {
                    old_sha = Some(args[i].clone());
                } else {
                    eprintln!("Unknown squash-authorship argument: {}", args[i]);
                    std::process::exit(1);
                }
                i += 1;
            }
        }
    }

    // Validate required arguments
    let new_sha = match new_sha {
        Some(s) => s,
        None => {
            eprintln!("Error: new_sha argument is required");
            eprintln!("Usage: git-ai squash-authorship <branch> <new_sha> <old_sha> [--dry-run]");
            std::process::exit(1);
        }
    };

    let old_sha = match old_sha {
        Some(s) => s,
        None => {
            eprintln!("Error: old_sha argument is required");
            eprintln!("Usage: git-ai squash-authorship <branch> <new_sha> <old_sha> [--dry-run]");
            std::process::exit(1);
        }
    };

    // TODO Think about whether or not path should be an optional argument

    // Find the git repository
    let repo = match find_repository_in_path(".") {
        Ok(repo) => repo,
        Err(e) => {
            eprintln!("Failed to find repository: {}", e);
            std::process::exit(1);
        }
    };

    if let Err(e) = prepare_working_log_after_squash(&repo, &old_sha, &new_sha, "") {
        eprintln!("Squash authorship failed: {}", e);
        std::process::exit(1);
    }
}
