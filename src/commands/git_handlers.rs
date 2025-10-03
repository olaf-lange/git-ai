use crate::commands::commit_hooks;
use crate::commands::fetch_hooks;
use crate::commands::push_hooks;
use crate::config;
use crate::git::cli_parser::{ParsedGitInvocation, parse_git_cli_args};
use crate::utils::debug_log;
#[cfg(unix)]
use std::os::unix::process::CommandExt;
#[cfg(unix)]
use std::os::unix::process::ExitStatusExt;
use std::process::Command;
#[cfg(unix)]
use std::sync::atomic::{AtomicI32, Ordering};

#[cfg(unix)]
static CHILD_PGID: AtomicI32 = AtomicI32::new(0);

#[cfg(unix)]
extern "C" fn forward_signal_handler(sig: libc::c_int) {
    let pgid = CHILD_PGID.load(Ordering::Relaxed);
    if pgid > 0 {
        unsafe {
            // Send to the whole child process group
            let _ = libc::kill(-pgid, sig);
        }
    }
}

#[cfg(unix)]
fn install_forwarding_handlers() {
    unsafe {
        let handler = forward_signal_handler as usize;
        let _ = libc::signal(libc::SIGTERM, handler);
        let _ = libc::signal(libc::SIGINT, handler);
        let _ = libc::signal(libc::SIGHUP, handler);
        let _ = libc::signal(libc::SIGQUIT, handler);
    }
}

#[cfg(unix)]
fn uninstall_forwarding_handlers() {
    unsafe {
        let _ = libc::signal(libc::SIGTERM, libc::SIG_DFL);
        let _ = libc::signal(libc::SIGINT, libc::SIG_DFL);
        let _ = libc::signal(libc::SIGHUP, libc::SIG_DFL);
        let _ = libc::signal(libc::SIGQUIT, libc::SIG_DFL);
    }
}

struct CommandHooksContext {
    pre_commit_hook_result: Option<bool>,
}

/// Return the alias definition for a given command name (if any) by consulting
/// `git config alias.<name>` with the same global args as the invocation.
/// Returns `None` if no alias is configured.
fn get_alias_for_command(global_args: &[String], name: &str) -> Option<String> {
    // Build: <global_args> + ["config", "--get", format!("alias.{}", name)]
    let mut args: Vec<String> = Vec::with_capacity(global_args.len() + 4);
    args.extend(global_args.iter().cloned());
    args.push("config".to_string());
    args.push("--get".to_string());
    args.push(format!("alias.{}", name));

    match Command::new(config::Config::get().git_cmd()).args(&args).output() {
        Ok(output) if output.status.success() => {
            let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if s.is_empty() { None } else { Some(s) }
        }
        _ => None,
    }
}

/// Tokenize a git alias definition into argv-like tokens, handling simple
/// shell-style quotes and backslash escapes similarly to git's split_cmdline.
/// Returns None on unterminated quotes to avoid unsafe rewrites.
fn tokenize_alias(definition: &str) -> Option<Vec<String>> {
    let mut tokens: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut in_single = false;
    let mut in_double = false;
    let mut chars = definition.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            '\'' => {
                if !in_double { in_single = !in_single; } else { current.push(ch); }
            }
            '"' => {
                if !in_single { in_double = !in_double; } else { current.push(ch); }
            }
            '\\' => {
                if in_single {
                    // Backslash is literal inside single quotes
                    current.push('\\');
                } else {
                    if let Some(next) = chars.next() { current.push(next); } else { current.push('\\'); }
                }
            }
            c if c.is_whitespace() => {
                if in_single || in_double {
                    current.push(c);
                } else if !current.is_empty() {
                    tokens.push(current.clone());
                    current.clear();
                }
            }
            _ => current.push(ch),
        }
    }

    if in_single || in_double { return None; }
    if !current.is_empty() { tokens.push(current); }
    Some(tokens)
}

pub fn handle_git(args: &[String]) {
    // If we're being invoked from a shell completion context, bypass git-ai logic
    // and delegate directly to the real git so existing completion scripts work.
    if in_shell_completion_context() {
        let orig_args: Vec<String> = std::env::args().skip(1).collect();
        proxy_to_git(&orig_args, true);
        return;
    }

    let mut command_hooks_context = CommandHooksContext { pre_commit_hook_result: None };

    // First parse of raw args (may contain an alias as the command token)
    let initial_parsed = parse_git_cli_args(args);

    // Single-pass alias expansion: if the command is an alias, expand it once.
    // For external aliases (starting with '!'), bypass hooks entirely and
    // delegate to git immediately with the original args.
    let parsed_args = if let Some(cmd) = initial_parsed.command.as_deref() {
        if let Some(alias_def) = get_alias_for_command(&initial_parsed.global_args, cmd) {
            let trimmed = alias_def.trim_start();
            if trimmed.starts_with('!') {
                // External command alias: run real git immediately, no hooks.
                debug_log("Detected external git alias; bypassing hooks and delegating to git");
                let orig = initial_parsed.to_invocation_vec();
                let status = proxy_to_git(&orig, false);
                exit_with_status(status);
            }
            // Tokenize alias and build a new argv: globals + [alias tokens] + original command args
            if let Some(mut alias_tokens) = tokenize_alias(trimmed) {
                if !alias_tokens.is_empty() {
                    let mut expanded: Vec<String> = Vec::with_capacity(
                        initial_parsed.global_args.len()
                            + usize::from(initial_parsed.saw_end_of_opts)
                            + alias_tokens.len()
                            + initial_parsed.command_args.len(),
                    );
                    expanded.extend(initial_parsed.global_args.iter().cloned());
                    if initial_parsed.saw_end_of_opts {
                        expanded.push("--".to_string());
                    }
                    expanded.append(&mut alias_tokens);
                    expanded.extend(initial_parsed.command_args.iter().cloned());
                    // Re-parse the expanded argv once; do not attempt to expand again.
                    parse_git_cli_args(&expanded)
                } else {
                    initial_parsed.clone()
                }
            } else {
                // Failed to safely tokenize; fall back to original to avoid incorrect behavior.
                initial_parsed.clone()
            }
        } else {
            initial_parsed.clone()
        }
    } else {
        initial_parsed.clone()
    };
    // println!("command: {:?}", parsed_args.command);
    // println!("global_args: {:?}", parsed_args.global_args);
    // println!("command_args: {:?}", parsed_args.command_args);
    // println!("to_invocation_vec: {:?}", parsed_args.to_invocation_vec());
    if !parsed_args.is_help {
        run_pre_command_hooks(&mut command_hooks_context, &parsed_args);
    }
    let exit_status = proxy_to_git(&parsed_args.to_invocation_vec(), false);
    if !parsed_args.is_help {
        run_post_command_hooks(&mut command_hooks_context, &parsed_args, exit_status);
    }
    exit_with_status(exit_status);
}

fn run_pre_command_hooks(
    command_hooks_context: &mut CommandHooksContext,
    parsed_args: &ParsedGitInvocation,
) {
    // Pre-command hooks
    match parsed_args.command.as_deref() {
        Some("commit") => {
            command_hooks_context.pre_commit_hook_result =
                Some(commit_hooks::commit_pre_command_hook(parsed_args));
        }
        _ => {}
    }
}

fn run_post_command_hooks(
    command_hooks_context: &mut CommandHooksContext,
    parsed_args: &ParsedGitInvocation,
    exit_status: std::process::ExitStatus,
) {
    // Post-command hooks
    match parsed_args.command.as_deref() {
        Some("commit") => {
            if let Some(pre_commit_hook_result) = command_hooks_context.pre_commit_hook_result {
                if !pre_commit_hook_result {
                    debug_log("Skipping git-ai post-commit hook because pre-commit hook failed");
                    return;
                }
            }
            commit_hooks::commit_post_command_hook(parsed_args, exit_status);
        }
        Some("fetch") => fetch_hooks::fetch_post_command_hook(parsed_args, exit_status),
        Some("push") => push_hooks::push_post_command_hook(parsed_args, exit_status),
        _ => {}
    }
}

fn proxy_to_git(args: &[String], exit_on_completion: bool) -> std::process::ExitStatus {
    // debug_log(&format!("proxying to git with args: {:?}", args));
    // debug_log(&format!("prepended global args: {:?}", prepend_global(args)));
    // Use spawn for interactive commands
    let child = {
        #[cfg(unix)]
        {
            // Only create a new process group for non-interactive runs.
            // If stdin is a TTY, the child must remain in the foreground
            // terminal process group to avoid SIGTTIN/SIGTTOU hangs.
            let is_interactive = unsafe { libc::isatty(libc::STDIN_FILENO) == 1 };
            let should_setpgid = !is_interactive;

            let mut cmd = Command::new(config::Config::get().git_cmd());
            cmd.args(args);
            unsafe {
                let setpgid_flag = should_setpgid;
                cmd.pre_exec(move || {
                    if setpgid_flag {
                        // Make the child its own process group leader so we can signal the group
                        let _ = libc::setpgid(0, 0);
                    }
                    Ok(())
                });
            }
            // We return both the spawned child and whether we changed PGID
            match cmd.spawn() {
                Ok(child) => Ok((child, should_setpgid)),
                Err(e) => Err(e),
            }
        }
        #[cfg(not(unix))]
        {
            Command::new(config::Config::get().git_cmd())
                .args(args)
                .spawn()
        }
    };

    #[cfg(unix)]
    match child {
        Ok((mut child, setpgid)) => {
            #[cfg(unix)]
            {
                if setpgid {
                    // Record the child's process group id (same as its pid after setpgid)
                    let pgid: i32 = child.id() as i32;
                    CHILD_PGID.store(pgid, Ordering::Relaxed);
                    install_forwarding_handlers();
                }
            }
            let status = child.wait();
            match status {
                Ok(status) => {
                    #[cfg(unix)]
                    {
                        if setpgid {
                            CHILD_PGID.store(0, Ordering::Relaxed);
                            uninstall_forwarding_handlers();
                        }
                    }
                    if exit_on_completion {
                        exit_with_status(status);
                    }
                    return status;
                }
                Err(e) => {
                    #[cfg(unix)]
                    {
                        if setpgid {
                            CHILD_PGID.store(0, Ordering::Relaxed);
                            uninstall_forwarding_handlers();
                        }
                    }
                    eprintln!("Failed to wait for git process: {}", e);
                    std::process::exit(1);
                }
            }
        }
        Err(e) => {
            eprintln!("Failed to execute git command: {}", e);
            std::process::exit(1);
        }
    }

    #[cfg(not(unix))]
    match child {
        Ok(mut child) => {
            let status = child.wait();
            match status {
                Ok(status) => {
                    if exit_on_completion {
                        exit_with_status(status);
                    }
                    return status;
                }
                Err(e) => {
                    eprintln!("Failed to wait for git process: {}", e);
                    std::process::exit(1);
                }
            }
        }
        Err(e) => {
            eprintln!("Failed to execute git command: {}", e);
            std::process::exit(1);
        }
    }
}

// Exit mirroring the child's termination: same signal if signaled, else exit code
fn exit_with_status(status: std::process::ExitStatus) -> ! {
    #[cfg(unix)]
    {
        if let Some(sig) = status.signal() {
            unsafe {
                libc::signal(sig, libc::SIG_DFL);
                libc::raise(sig);
            }
            // Should not return
            unreachable!();
        }
    }
    std::process::exit(status.code().unwrap_or(1));
}

// Detect if current process invocation is coming from shell completion machinery
// (bash, zsh via bashcompinit). If so, we should proxy directly to the real git
// without any extra behavior that could interfere with completion scripts.
fn in_shell_completion_context() -> bool {
    std::env::var("COMP_LINE").is_ok()
        || std::env::var("COMP_POINT").is_ok()
        || std::env::var("COMP_TYPE").is_ok()
}
