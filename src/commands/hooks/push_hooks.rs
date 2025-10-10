use crate::git::cli_parser::{ParsedGitInvocation, is_dry_run};
use crate::git::find_repository;
use crate::git::refs::{
    AI_AUTHORSHIP_PUSH_REFSPEC, copy_ref, merge_notes_from_ref, ref_exists, tracking_ref_for_remote,
};
use crate::git::repository::exec_git;
use crate::utils::debug_log;

pub fn push_post_command_hook(
    parsed_args: &ParsedGitInvocation,
    exit_status: std::process::ExitStatus,
) {
    if is_dry_run(&parsed_args.command_args)
        || !exit_status.success()
        || parsed_args
            .command_args
            .iter()
            .any(|a| a == "-d" || a == "--delete")
        || parsed_args.command_args.iter().any(|a| a == "--mirror")
    {
        return;
    }

    // Find the git repository
    let repo = match find_repository(&parsed_args.global_args) {
        Ok(repo) => repo,
        Err(e) => {
            eprintln!("Failed to find repository: {}", e);
            std::process::exit(1);
        }
    };

    let remotes = repo.remotes().ok();
    let remote_names: Vec<String> = remotes
        .as_ref()
        .map(|r| {
            (0..r.len())
                .filter_map(|i| r.get(i).map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    // 2) Push authorship refs to the appropriate remote
    let positional_remote = extract_remote_from_push_args(&parsed_args.command_args, &remote_names);

    let specified_remote = positional_remote.or_else(|| {
        parsed_args
            .command_args
            .iter()
            .find(|a| remote_names.iter().any(|r| r == *a))
            .cloned()
    });

    let remote = specified_remote
        .or_else(|| repo.upstream_remote().ok().flatten())
        .or_else(|| repo.get_default_remote().ok().flatten());

    if let Some(remote) = remote {
        // STEP 1: Fetch remote notes into tracking ref and merge before pushing
        // This ensures we don't lose notes from other branches/clones
        let tracking_ref = tracking_ref_for_remote(&remote);
        let fetch_refspec = format!("+refs/notes/ai:{}", tracking_ref);

        let mut fetch_before_push: Vec<String> = parsed_args.global_args.clone();
        fetch_before_push.push("-c".to_string());
        fetch_before_push.push("core.hooksPath=/dev/null".to_string());
        fetch_before_push.push("fetch".to_string());
        fetch_before_push.push("--no-tags".to_string());
        fetch_before_push.push("--recurse-submodules=no".to_string());
        fetch_before_push.push("--no-write-fetch-head".to_string());
        fetch_before_push.push("--no-write-commit-graph".to_string());
        fetch_before_push.push("--no-auto-maintenance".to_string());
        fetch_before_push.push(remote.clone());
        fetch_before_push.push(fetch_refspec);

        debug_log(&format!(
            "pre-push authorship fetch: {:?}",
            &fetch_before_push
        ));

        // Fetch is best-effort; if it fails (e.g., no remote notes yet), continue
        if exec_git(&fetch_before_push).is_ok() {
            // Merge fetched notes into local refs/notes/ai
            let local_notes_ref = "refs/notes/ai";

            if ref_exists(&repo, &tracking_ref) {
                if ref_exists(&repo, local_notes_ref) {
                    // Both exist - merge them
                    debug_log(&format!(
                        "pre-push: merging {} into {}",
                        tracking_ref, local_notes_ref
                    ));
                    if let Err(e) = merge_notes_from_ref(&repo, &tracking_ref) {
                        debug_log(&format!("pre-push notes merge failed: {}", e));
                    }
                } else {
                    // Only tracking ref exists - copy it to local
                    debug_log(&format!(
                        "pre-push: initializing {} from {}",
                        local_notes_ref, tracking_ref
                    ));
                    if let Err(e) = copy_ref(&repo, &tracking_ref, local_notes_ref) {
                        debug_log(&format!("pre-push notes copy failed: {}", e));
                    }
                }
            }
        }

        // STEP 2: Push notes without force (requires fast-forward)
        let mut push_authorship: Vec<String> = parsed_args.global_args.clone();
        push_authorship.push("-c".to_string());
        push_authorship.push("core.hooksPath=/dev/null".to_string());
        push_authorship.push("push".to_string());
        push_authorship.push("--quiet".to_string());
        push_authorship.push("--no-recurse-submodules".to_string());
        push_authorship.push("--no-verify".to_string());
        push_authorship.push(remote);
        push_authorship.push(AI_AUTHORSHIP_PUSH_REFSPEC.to_string());

        debug_log(&format!(
            "pushing authorship refs (no force): {:?}",
            &push_authorship
        ));
        if let Err(e) = exec_git(&push_authorship) {
            // Best-effort; don't fail user operation due to authorship sync issues
            debug_log(&format!("authorship push skipped due to error: {}", e));
        }
    } else {
        // No remotes configured; skip silently
        debug_log("no remotes found for authorship push; skipping");
    }
}

fn extract_remote_from_push_args(args: &[String], known_remotes: &[String]) -> Option<String> {
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if arg == "--" {
            return args.get(i + 1).cloned();
        }
        if arg.starts_with('-') {
            if let Some((flag, value)) = is_push_option_with_inline_value(arg) {
                if flag == "--repo" {
                    return Some(value.to_string());
                }
                i += 1;
                continue;
            }

            if option_consumes_separate_value(arg.as_str()) {
                if arg == "--repo" {
                    return args.get(i + 1).cloned();
                }
                i += 2;
                continue;
            }

            i += 1;
            continue;
        }
        return Some(arg.clone());
    }

    known_remotes
        .iter()
        .find(|r| args.iter().any(|arg| arg == *r))
        .cloned()
}

fn is_push_option_with_inline_value(arg: &str) -> Option<(&str, &str)> {
    if let Some((flag, value)) = arg.split_once('=') {
        Some((flag, value))
    } else if (arg.starts_with("-C") || arg.starts_with("-c")) && arg.len() > 2 {
        // Treat -C<path> or -c<name>=<value> as inline values
        let flag = &arg[..2];
        let value = &arg[2..];
        Some((flag, value))
    } else {
        None
    }
}

fn option_consumes_separate_value(arg: &str) -> bool {
    matches!(
        arg,
        "--repo" | "--receive-pack" | "--exec" | "-o" | "--push-option" | "-c" | "-C"
    )
}
