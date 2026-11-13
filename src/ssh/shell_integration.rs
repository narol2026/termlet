//! Best-effort remote shell-integration injection (OSC 7 + OSC 633;A/B/C/D/E).
//!
//! The terminal backend already parses OSC 7 / OSC 633 sequences emitted by a
//! shell, but over SSH nothing emits them unless the remote shell is configured
//! to. This module installs a small self-cleaning hook script per connection so
//! the user's real interactive shell reports its working directory (OSC 7), the
//! prompt/command boundary (OSC 633;B), the exact command text (OSC 633;E) and
//! exit codes (OSC 633;D) to termlet.
//!
//! It leaves no persistent side effects and needs no root: detect the remote
//! shell on a non-pty exec channel, write the hook to a 077-mode `mktemp` path
//! (a `mktemp -d` directory for zsh, so `ZDOTDIR` picks up its `.zshrc`) plus an
//! executable `$SHELL` trampoline for programs like btop, then `exec` the
//! interactive shell on the pty channel with an rcfile/ZDOTDIR override. The
//! script's `trap ... EXIT` removes the temp file when the shell exits.
//!
//! Any failure returns `Err`/`None` and the caller (`ssh/bridge.rs`) falls
//! back to a plain `request_shell` — injection must never break a connection.

#[cfg(unix)]
use std::collections::HashMap;
#[cfg(unix)]
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use russh::client;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::ssh::client::Client;

/// Per-step timeout for the prep channels, consistent with the bridge's
/// existing 10-second send timeout.
const PREP_TIMEOUT: Duration = Duration::from_secs(10);

/// Which interactive shell we detected and will inject for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RemoteShell {
    Bash,
    Zsh,
    Fish,
    Other,
}

/// Outcome of a successful prep: enough for the caller to `exec` the wrapper.
#[derive(Debug)]
pub(crate) struct ShellIntegration {
    pub(crate) kind: RemoteShell,
    /// Remote path of the hook file (bash/fish) or directory (zsh).
    pub(crate) path: String,
    /// Ready-to-exec wrapper command for the main pty channel.
    pub(crate) launch: String,
}

/// Classify a shell path (e.g. `/bin/bash`, `-zsh`) into our supported set.
pub(crate) fn classify_shell(shell: &str) -> RemoteShell {
    let name = shell
        .trim()
        .trim_start_matches('-')
        .rsplit('/')
        .next()
        .unwrap_or("");
    match name {
        "bash" => RemoteShell::Bash,
        "zsh" => RemoteShell::Zsh,
        "fish" => RemoteShell::Fish,
        _ => RemoteShell::Other,
    }
}

/// Non-pty exec command that prints the remote user's login shell.
pub(crate) const DETECT_SHELL_CMD: &str = "sh -c 'printf \"%s\" \"${SHELL:-}\"'";

/// Non-pty exec command that creates a 077-mode temp file, reads the hook
/// script from stdin, and prints the resulting path on stdout.
pub(crate) const WRITE_FILE_CMD: &str = "sh -c 'umask 077; f=$(mktemp /tmp/termlet.XXXXXX) || exit 1; cat > \"$f\"; printf \"%s\" \"$f\"'";

/// Same, but for zsh: create a temp *directory* whose `.zshrc` is the hook
/// script (so `ZDOTDIR=<dir> zsh` loads it), printing the dir path.
pub(crate) const WRITE_ZSH_DIR_CMD: &str = "sh -c 'umask 077; d=$(mktemp -d /tmp/termlet.XXXXXX) || exit 1; cat > \"$d/.zshrc\"; printf \"%s\" \"$d\"'";

/// Per-shell hook script body.
///
/// Emits `OSC 633;D` (exit code) and `OSC 633;A` (prompt start) at each
/// prompt, `OSC 633;B` (end of prompt / start of command) right after the
/// prompt text, `OSC 633;E` (exact command text) and `OSC 633;C` (command
/// executed) before each command runs, and `OSC 7` (working directory).  The
/// terminal records the `633;B` grid position as a fallback and prefers the
/// explicit `633;E` text; the app records that exact command.  Shells that
/// bypass injection emit none of this and are simply not tracked.
///
/// The scripts use `\033`/`\007` (not `\e`) so they parse under dash as well
/// as bash/zsh.  The cleanup trap references `$TERMLET_OSC633_FILE` (the env
/// var set by the launch wrapper), not the literal path, so the body can be
/// written before the temp path is known.
pub(crate) fn script_for(shell: RemoteShell) -> &'static str {
    match shell {
        RemoteShell::Bash => {
            r#"trap 'if [ -n "$TERMLET_OSC633_FILE" ]; then rm -f -- "$TERMLET_OSC633_FILE"; [ -n "$TERMLET_OSC633_WRAPPER" ] && rm -f -- "$TERMLET_OSC633_WRAPPER"; fi' EXIT

# Replicate login-shell rc loading (--noprofile --rcfile skips both).
if [ -f "$HOME/.bash_profile" ]; then
    . "$HOME/.bash_profile"
elif [ -f "$HOME/.bash_login" ]; then
    . "$HOME/.bash_login"
elif [ -f "$HOME/.profile" ]; then
    . "$HOME/.profile"
fi
if [ -f "$HOME/.bashrc" ]; then
    . "$HOME/.bashrc"
fi

_termlet_in_precmd=0

_termlet_precmd() {
    local code=$?
    _termlet_in_precmd=1
    printf '\033]633;D;%s\007' "$code"
    printf '\033]633;A\007'
    printf '\033]7;file://%s%s\007' "$HOSTNAME" "$PWD"
}
# Append OSC 633 ; B (end of prompt / start of command) to the prompt so the
# terminal records where the prompt ends. Runs LAST in PROMPT_COMMAND, after
# the user's own hooks, so a dynamically rebuilt PS1 (starship, p10k, ...) is
# wrapped after they set it. The sentinel makes the wrap idempotent: a prompt
# that already carries our marker is not wrapped again, so layers never
# accumulate.
_termlet_wrap_prompt() {
    if [ "$PS1" != "${_TERMLET_WRAPPED_PS1:-}" ]; then
        PS1="$PS1"'\[\e]633;B\a\]'
        _TERMLET_WRAPPED_PS1="$PS1"
    fi
    if [ "$PS2" != "${_TERMLET_WRAPPED_PS2:-}" ]; then
        PS2="$PS2"'\[\e]633;B\a\]'
        _TERMLET_WRAPPED_PS2="$PS2"
    fi
    _termlet_in_precmd=0
}
# Prepend so our marker runs first (exit code is still current), then the
# user's own PROMPT_COMMAND (starship, etc.) is preserved, and our prompt wrap
# runs last. Trim trailing spaces/semicolons from the existing value first: a
# user hook ending in `; ` would otherwise concatenate into `;;`, an empty
# command between separators that bash rejects as a syntax error.
_termlet_pc=$(printf '%s' "$PROMPT_COMMAND" | sed 's/[[:space:];]*$//')
PROMPT_COMMAND="_termlet_precmd${_termlet_pc:+;$_termlet_pc};_termlet_wrap_prompt"
# OSC 633 ; E (command line) and OSC 633 ; C (command executed) before each
# command runs.  Bash has no native preexec hook, so the DEBUG trap is used to
# report `$BASH_COMMAND`.  Skip the prompt-command bookkeeping itself so the
# markers are emitted once per interactive command rather than once per hook
# invocation.
_termlet_preexec() {
    case "$BASH_COMMAND" in
        _termlet_*) return ;;
    esac
    if [ "$_termlet_in_precmd" -eq 1 ]; then
        return
    fi
    printf '\033]633;E;%s\007' "$BASH_COMMAND"
    printf '\033]633;C\007'
}
# Only install our DEBUG hook when the user has no DEBUG trap of their own;
# replacing one would silently break interactive tooling that depends on it.
_termlet_debug_trap=$(trap -p DEBUG)
if [ -z "$_termlet_debug_trap" ]; then
    trap _termlet_preexec DEBUG
fi
"#
        }
        RemoteShell::Zsh => {
            r#"trap '[ -n "$TERMLET_OSC633_FILE" ] && rm -rf -- "$TERMLET_OSC633_FILE"' EXIT

autoload -Uz add-zsh-hook

# Replicate login-shell rc loading (ZDOTDIR override skips both).
[[ -f "$HOME/.zprofile" ]] && . "$HOME/.zprofile"
[[ -f "$HOME/.zshrc" ]] && . "$HOME/.zshrc"

_termlet_precmd() {
    local code=$?
    printf '\033]633;D;%s\007' "$code"
    printf '\033]633;A\007'
    printf '\033]7;file://%s%s\007' "${HOSTNAME:-$HOST}" "$PWD"
    # Append OSC 633 ; B (end of prompt / start of command) to the prompt.
    # Our precmd is registered after the user's .zshrc loads, so it runs last
    # and wraps the final PROMPT set by the theme (p10k, oh-my-zsh, starship).
    # The sentinel keeps the wrap to a single layer.
    if [ "$PROMPT" != "${_TERMLET_WRAPPED_PROMPT:-}" ]; then
        PROMPT="$PROMPT"$'%{\e]633;B\a%}'
        _TERMLET_WRAPPED_PROMPT="$PROMPT"
    fi
}
_termlet_preexec() {
    printf '\033]633;E;%s\007' "$1"
    printf '\033]633;C\007'
}
add-zsh-hook precmd _termlet_precmd
add-zsh-hook preexec _termlet_preexec
"#
        }
        RemoteShell::Fish => {
            r#"function _termlet_cleanup --on-event fish_exit
    if set -q TERMLET_OSC633_FILE
        rm -f -- $TERMLET_OSC633_FILE
    end
    if set -q TERMLET_OSC633_WRAPPER
        rm -f -- $TERMLET_OSC633_WRAPPER
    end
end

function _termlet_on_prompt --on-event fish_prompt
    set -l code $status
    printf '\033]633;D;%s\007' $code
    printf '\033]633;A\007'
    printf '\033]7;file://%s%s\007' (hostname) $PWD
end

function _termlet_on_preexec --on-event fish_preexec
    printf '\033]633;E;%s\007' "$argv[1]"
    printf '\033]633;C\007'
end

# Emit OSC 633 ; B (end of prompt / start of command) right after the prompt
# text: copy the user's fish_prompt and redefine it to append the marker.
if not functions -q _termlet_orig_prompt
    functions -c fish_prompt _termlet_orig_prompt
    function fish_prompt
        _termlet_orig_prompt
        printf '\033]633;B\007'
    end
end
"#
        }
        RemoteShell::Other => "",
    }
}

/// Local/remote path of the `$SHELL` wrapper that makes child shells inherit
/// termlet's shell integration when a program such as btop launches `$SHELL`.
///
/// For zsh the hook path is already a directory (its `.zshrc` lives inside),
/// so the wrapper is also stored inside that directory.  For bash/fish the
/// hook is a regular file and the wrapper is a sibling `.wrapper` file.
pub(crate) fn shell_wrapper_path(shell: RemoteShell, hook_path: &str) -> String {
    match shell {
        RemoteShell::Zsh => format!("{hook_path}/shell"),
        RemoteShell::Bash | RemoteShell::Fish => format!("{hook_path}.wrapper"),
        RemoteShell::Other => hook_path.to_string(),
    }
}

/// Executable `#!/bin/sh` trampoline used as `SHELL` by the outer shell.  It
/// forwards all arguments to the real shell while keeping the hook/rcfile
/// override in place, so `$SHELL -c '...'` and interactive `$SHELL` both work.
pub(crate) fn shell_wrapper_script(shell: RemoteShell) -> &'static str {
    match shell {
        RemoteShell::Bash => {
            "#!/bin/sh\nexec bash --noprofile --rcfile \"$TERMLET_OSC633_FILE\" \"$@\"\n"
        }
        RemoteShell::Zsh => "#!/bin/sh\nexec env ZDOTDIR=\"$TERMLET_OSC633_FILE\" zsh \"$@\"\n",
        RemoteShell::Fish => {
            "#!/bin/sh\nexec fish --init-command \"source $TERMLET_OSC633_FILE\" \"$@\"\n"
        }
        RemoteShell::Other => "#!/bin/sh\nexec \"$@\"\n",
    }
}

/// Wrapper command that relaunches the detected shell under the hook script,
/// as a string for `channel.exec`.  Returns `None` for unsupported shells.
///
/// Uses `exec env VAR='P' cmd` rather than `VAR=P exec cmd`: the string is
/// parsed by the *user's login shell* (unknown — could be bash/zsh/fish/csh),
/// and `exec env A='B' cmd` is valid in all of them.  `exec` matters: without
/// it the channel's `ExitStatus` would report the outer shell, not the
/// interactive one.
pub(crate) fn launch_cmd(shell: RemoteShell, path: &str) -> Option<String> {
    let wrapper = shell_wrapper_path(shell, path);
    match shell {
        RemoteShell::Bash => Some(format!(
            "exec env TERMLET_OSC633_FILE='{path}' TERMLET_OSC633_WRAPPER='{wrapper}' SHELL='{wrapper}' bash --noprofile --rcfile '{path}'"
        )),
        RemoteShell::Zsh => Some(format!(
            "exec env TERMLET_OSC633_FILE='{path}' TERMLET_OSC633_WRAPPER='{wrapper}' SHELL='{wrapper}' ZDOTDIR='{path}' zsh"
        )),
        RemoteShell::Fish => Some(format!(
            "exec env TERMLET_OSC633_FILE='{path}' TERMLET_OSC633_WRAPPER='{wrapper}' SHELL='{wrapper}' fish --init-command 'source {path}'"
        )),
        RemoteShell::Other => None,
    }
}

#[cfg(unix)]
static HOOK_COUNTER: AtomicUsize = AtomicUsize::new(0);

/// Build program/args/env for a *local* (non-SSH) interactive shell so it emits
/// OSC 7 / 633, reusing the same hook scripts as the remote path.
///
/// Returns `None` on Windows (`cmd.exe`) or unsupported shells, in which case
/// the caller just spawns the plain shell.  The temp hook file is removed by
/// the shell's own EXIT trap; a PTY-spawn failure leaks a few hundred bytes in
/// the OS temp dir, which is acceptable.
#[cfg(unix)]
pub(crate) fn local_injection(
    shell: &str,
) -> Option<(String, Vec<String>, HashMap<String, String>)> {
    let kind = classify_shell(shell);
    if kind == RemoteShell::Other {
        return None;
    }
    let path = write_local_hook(kind, script_for(kind))?;
    let wrapper = write_local_wrapper(kind, &path)?;

    let mut env = HashMap::new();
    env.insert("TERMLET_OSC633_FILE".to_string(), path.clone());
    env.insert("TERMLET_OSC633_WRAPPER".to_string(), wrapper.clone());
    env.insert("SHELL".to_string(), wrapper);
    let (program, args) = match kind {
        RemoteShell::Bash => (
            "bash".to_string(),
            vec!["--noprofile".to_string(), "--rcfile".to_string(), path],
        ),
        RemoteShell::Zsh => {
            env.insert("ZDOTDIR".to_string(), path);
            ("zsh".to_string(), vec![])
        }
        RemoteShell::Fish => (
            "fish".to_string(),
            vec!["--init-command".to_string(), format!("source {path}")],
        ),
        RemoteShell::Other => return None,
    };
    Some((program, args, env))
}

/// Write the hook script to a unique temp file (zsh: a directory whose
/// `.zshrc` is the script, so `ZDOTDIR` picks it up).  The pid+counter name
/// keeps concurrently opened local terminals from colliding.
#[cfg(unix)]
fn write_local_hook(kind: RemoteShell, script: &str) -> Option<String> {
    let n = HOOK_COUNTER.fetch_add(1, Ordering::Relaxed);
    let base = std::env::temp_dir().join(format!("termlet-osc633-{}-{n}", std::process::id()));
    if kind == RemoteShell::Zsh {
        std::fs::create_dir_all(&base).ok()?;
        std::fs::write(base.join(".zshrc"), script).ok()?;
    } else {
        std::fs::write(&base, script).ok()?;
    }
    Some(base.to_string_lossy().into_owned())
}

/// Write the executable `$SHELL` trampoline next to (or inside, for zsh) the
/// local hook.  Used by child shells launched from programs such as btop.
#[cfg(unix)]
fn write_local_wrapper(kind: RemoteShell, hook_path: &str) -> Option<String> {
    let wrapper = shell_wrapper_path(kind, hook_path);
    let wrapper_path = std::path::Path::new(&wrapper);
    std::fs::write(wrapper_path, shell_wrapper_script(kind)).ok()?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = wrapper_path.metadata().ok()?.permissions();
        permissions.set_mode(0o700);
        std::fs::set_permissions(wrapper_path, permissions).ok()?;
    };

    Some(wrapper)
}

/// Remote prep command that writes and chmods the `$SHELL` trampoline at an
/// already-validated temp path.  The path is interpolated only after
/// [`is_safe_temp_path`] accepts it.
fn write_remote_wrapper_cmd(wrapper_path: &str) -> String {
    format!("sh -c 'umask 077; cat > \"$1\"; chmod 700 \"$1\"' -- '{wrapper_path}'")
}

/// Reject any remote-echoed path that could inject into the wrapper command
/// line.  The path is controlled by the remote `mktemp`, so treat it as
/// untrusted before interpolating it into a shell string.
fn is_safe_temp_path(p: &str) -> bool {
    p.starts_with("/tmp/termlet.")
        && p.len() <= 64
        && p.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'/' || b == b'.' || b == b'-')
}

/// Run one non-pty prep command: exec, optionally feed stdin, then read all
/// stdout.  Times out after [`PREP_TIMEOUT`].
async fn prep_channel(
    session: &client::Handle<Client>,
    cmd: &str,
    input: Option<&[u8]>,
) -> Result<String, String> {
    tokio::time::timeout(PREP_TIMEOUT, async {
        let channel = session
            .channel_open_session()
            .await
            .map_err(|e| format!("open prep channel failed: {e}"))?;
        channel
            .exec(true, cmd.as_bytes())
            .await
            .map_err(|e| format!("prep exec failed: {e}"))?;

        let mut stream = channel.into_stream();
        if let Some(bytes) = input {
            stream
                .write_all(bytes)
                .await
                .map_err(|e| format!("prep write failed: {e}"))?;
            // shutdown() sends ChannelMsg::Eof so the remote `cat` sees EOF.
            stream
                .shutdown()
                .await
                .map_err(|e| format!("prep eof failed: {e}"))?;
        }

        let mut out = Vec::new();
        stream
            .read_to_end(&mut out)
            .await
            .map_err(|e| format!("prep read failed: {e}"))?;
        String::from_utf8(out).map_err(|_error| "prep output not UTF-8".to_string())
    })
    .await
    .map_err(|_error| format!("prep step timed out after {PREP_TIMEOUT:?}"))?
}

/// Detect the remote shell, install the hook script, and build the launch
/// wrapper.  Returns `Ok(None)` when there is nothing to inject (no/unsupported
/// shell); `Err` on any failure, so the caller can fall back to a plain shell.
pub(crate) async fn prepare_shell_integration(
    session: &client::Handle<Client>,
) -> Result<Option<ShellIntegration>, String> {
    // 1) Detect the shell.
    let shell = prep_channel(session, DETECT_SHELL_CMD, None)
        .await?
        .trim()
        .to_string();
    let kind = classify_shell(&shell);
    if shell.is_empty() || kind == RemoteShell::Other {
        return Ok(None);
    }

    // 2) Create the temp path and write the hook script.
    let (write_cmd, script) = match kind {
        RemoteShell::Zsh => (WRITE_ZSH_DIR_CMD, script_for(kind)),
        _ => (WRITE_FILE_CMD, script_for(kind)),
    };
    let path = prep_channel(session, write_cmd, Some(script.as_bytes()))
        .await?
        .trim()
        .to_string();
    if !is_safe_temp_path(&path) {
        return Err(format!("unexpected remote temp path: {path:?}"));
    }

    // 3) Install the executable `$SHELL` trampoline so child shells spawned by
    // programs like btop inherit the same OSC 7 / OSC 633 hooks.
    let wrapper = shell_wrapper_path(kind, &path);
    prep_channel(
        session,
        &write_remote_wrapper_cmd(&wrapper),
        Some(shell_wrapper_script(kind).as_bytes()),
    )
    .await?;

    // 4) Build the exec wrapper for the main pty channel.
    let launch = launch_cmd(kind, &path).ok_or_else(|| "unsupported shell".to_string())?;
    Ok(Some(ShellIntegration { kind, path, launch }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_shell_paths() {
        assert_eq!(classify_shell("/bin/bash"), RemoteShell::Bash);
        assert_eq!(classify_shell("bash"), RemoteShell::Bash);
        assert_eq!(classify_shell("-zsh"), RemoteShell::Zsh);
        assert_eq!(classify_shell("/usr/local/bin/fish"), RemoteShell::Fish);
        assert_eq!(classify_shell("/bin/sh"), RemoteShell::Other);
        assert_eq!(classify_shell(""), RemoteShell::Other);
    }

    #[test]
    fn bash_script_contains_expected_hooks() {
        let s = script_for(RemoteShell::Bash);
        assert!(s.contains(r"\033]633;D;%s\007"));
        assert!(s.contains(r"\033]633;A\007"));
        assert!(s.contains(r"\033]7;file://%s%s\007"));
        // 633;E then 633;C via a DEBUG-trap preexec, and 633;B appended to
        // PS1/PS2.
        assert!(s.contains(r"\033]633;E;%s\007"));
        assert!(s.contains(r"\033]633;C\007"));
        assert!(s.contains("trap _termlet_preexec DEBUG"));
        assert!(s.contains("_termlet_preexec"));
        assert!(s.contains(r"\[\e]633;B\a\]"));
        assert!(s.contains(r"_termlet_wrap_prompt"));
        assert!(s.contains("trap"));
        assert!(s.contains("$TERMLET_OSC633_FILE"));
        assert!(s.contains(r"sed 's/[[:space:];]*$//'"));
        assert!(s.contains(
            r#"PROMPT_COMMAND="_termlet_precmd${_termlet_pc:+;$_termlet_pc};_termlet_wrap_prompt""#
        ));
        assert!(s.contains(r#". "$HOME/.bashrc""#));
        assert!(s.contains(r#". "$HOME/.bash_profile""#));
    }

    /// The exact join logic from the generated bash script must tolerate a user
    /// `PROMPT_COMMAND` that already ends in `; ` (the bug that produced a `;;`
    /// syntax error on login).  Runs the *generated* trim+assign lines through
    /// bash to keep the test honest.
    #[test]
    fn bash_prompt_command_join_handles_trailing_semicolon() {
        use std::process::Command;

        let s = script_for(RemoteShell::Bash);
        let frag: Vec<&str> = s
            .lines()
            .filter(|l| l.starts_with("_termlet_pc=") || l.starts_with("PROMPT_COMMAND="))
            .collect();
        assert_eq!(
            frag.len(),
            2,
            "expected trim+assign lines in script: {frag:?}"
        );

        let src = format!(
            "set -e\n_termlet_precmd() {{ :; }}\n_termlet_wrap_prompt() {{ :; }}\nPROMPT_COMMAND='history -a; '\n{}\nprintf '%s\\n' \"$PROMPT_COMMAND\"\n",
            frag.join("\n")
        );
        let out = Command::new("bash").arg("-c").arg(&src).output();
        let Ok(out) = out else { return };
        assert!(
            out.status.success(),
            "bash rejected PROMPT_COMMAND: {:?}",
            String::from_utf8_lossy(&out.stderr)
        );
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(
            stdout.starts_with("_termlet_precmd;history -a;_termlet_wrap_prompt\n"),
            "unexpected join: {stdout:?}"
        );
    }

    #[test]
    fn zsh_script_contains_expected_hooks() {
        let s = script_for(RemoteShell::Zsh);
        assert!(s.contains("add-zsh-hook precmd _termlet_precmd"));
        assert!(s.contains("add-zsh-hook preexec _termlet_preexec"));
        assert!(s.contains(r"\033]633;E;%s\007"));
        assert!(s.contains(r"\033]633;C\007"));
        // 633;B appended to PROMPT as a zero-width zsh escape.
        assert!(s.contains(r"%{\e]633;B\a%}"));
        assert!(s.contains("autoload -Uz add-zsh-hook"));
        assert!(s.contains("rm -rf -- \"$TERMLET_OSC633_FILE\""));
        assert!(s.contains(r#". "$HOME/.zshrc""#));
        // ZDOTDIR is set by the launch wrapper, never assigned in the script.
        assert!(!s.contains("ZDOTDIR="));
    }

    #[test]
    fn fish_script_contains_expected_hooks() {
        let s = script_for(RemoteShell::Fish);
        assert!(s.contains("--on-event fish_prompt"));
        assert!(s.contains("--on-event fish_preexec"));
        assert!(s.contains("--on-event fish_exit"));
        assert!(s.contains(r"\033]633;E;%s\007"));
        assert!(s.contains(r"\033]633;C\007"));
        // 633;B appended after the user's prompt output.
        assert!(s.contains(r"\033]633;B\007"));
        assert!(s.contains("functions -c fish_prompt _termlet_orig_prompt"));
        assert!(s.contains("set -l code $status"));
        assert!(s.contains("rm -f -- $TERMLET_OSC633_FILE"));
        assert!(s.contains("rm -f -- $TERMLET_OSC633_WRAPPER"));
    }

    #[test]
    fn launch_wrappers() {
        let p = "/tmp/termlet.A1b2C3";
        let bash_wrapper = shell_wrapper_path(RemoteShell::Bash, p);
        let zsh_wrapper = shell_wrapper_path(RemoteShell::Zsh, p);
        let fish_wrapper = shell_wrapper_path(RemoteShell::Fish, p);
        assert_eq!(
            launch_cmd(RemoteShell::Bash, p),
            Some(format!(
                "exec env TERMLET_OSC633_FILE='{p}' TERMLET_OSC633_WRAPPER='{bash_wrapper}' SHELL='{bash_wrapper}' bash --noprofile --rcfile '{p}'"
            ))
        );
        assert!(launch_cmd(RemoteShell::Zsh, p).unwrap().contains(&format!(
            "TERMLET_OSC633_WRAPPER='{zsh_wrapper}' SHELL='{zsh_wrapper}' ZDOTDIR='{p}'"
        )));
        assert!(
            launch_cmd(RemoteShell::Fish, p)
                .unwrap()
                .contains(&format!(
                    "TERMLET_OSC633_WRAPPER='{fish_wrapper}' SHELL='{fish_wrapper}' fish --init-command 'source {p}'"
                ))
        );
        assert_eq!(launch_cmd(RemoteShell::Other, p), None);
    }

    #[test]
    fn write_commands_keep_mktemp_template() {
        assert_eq!(
            WRITE_FILE_CMD,
            "sh -c 'umask 077; f=$(mktemp /tmp/termlet.XXXXXX) || exit 1; cat > \"$f\"; printf \"%s\" \"$f\"'"
        );
        assert_eq!(
            WRITE_ZSH_DIR_CMD,
            "sh -c 'umask 077; d=$(mktemp -d /tmp/termlet.XXXXXX) || exit 1; cat > \"$d/.zshrc\"; printf \"%s\" \"$d\"'"
        );
    }

    #[cfg(unix)]
    #[test]
    fn local_injection_builds_launch() {
        // zsh → ZDOTDIR env pointing at a dir whose .zshrc is the hook.
        let (prog, args, env) = local_injection("/bin/zsh").expect("zsh injection");
        assert_eq!(prog, "zsh");
        assert!(args.is_empty());
        let zpath = env["ZDOTDIR"].as_str();
        assert!(zpath.starts_with(&std::env::temp_dir().to_string_lossy().to_string()));
        assert!(std::path::Path::new(zpath).join(".zshrc").exists());
        assert_eq!(env["TERMLET_OSC633_FILE"], zpath);
        assert_eq!(env["SHELL"], format!("{zpath}/shell"));
        assert!(std::path::Path::new(&env["SHELL"]).exists());
        let _ = std::fs::remove_dir_all(zpath);

        // bash → --noprofile --rcfile <file>.
        let (prog, args, env) = local_injection("/bin/bash").expect("bash injection");
        assert_eq!(prog, "bash");
        let bpath = env["TERMLET_OSC633_FILE"].as_str();
        assert!(
            args.windows(2)
                .any(|w| w == ["--rcfile".to_string(), bpath.to_string()]),
            "expected --rcfile {bpath}: {args:?}"
        );
        assert!(std::path::Path::new(bpath).exists());
        assert!(!env.contains_key("ZDOTDIR"));
        assert_eq!(env["TERMLET_OSC633_WRAPPER"], format!("{bpath}.wrapper"));
        assert_eq!(env["SHELL"], format!("{bpath}.wrapper"));
        assert!(std::path::Path::new(&env["SHELL"]).exists());
        let _ = std::fs::remove_file(bpath);
        let _ = std::fs::remove_file(&env["SHELL"]);

        // fish → --init-command 'source <file>'.
        let (prog, args, env) = local_injection("/usr/bin/fish").expect("fish injection");
        assert_eq!(prog, "fish");
        assert!(args[0] == "--init-command" && args[1].starts_with("source "));
        assert_eq!(
            env["TERMLET_OSC633_WRAPPER"],
            format!("{}.wrapper", env["TERMLET_OSC633_FILE"])
        );
        assert!(std::path::Path::new(&env["SHELL"]).exists());
        let _ = std::fs::remove_file(&env["TERMLET_OSC633_FILE"]);
        let _ = std::fs::remove_file(&env["SHELL"]);

        // Unsupported/absent shells get no injection.
        assert!(local_injection("/bin/sh").is_none());
        assert!(local_injection("cmd.exe").is_none());
        assert!(local_injection("").is_none());
    }

    #[test]
    fn safe_temp_path_validation() {
        assert!(is_safe_temp_path("/tmp/termlet.A1b2C3"));
        assert!(is_safe_temp_path("/tmp/termlet.ab-cd.12"));
        assert!(!is_safe_temp_path("/tmp/evil; rm -rf /"));
        assert!(!is_safe_temp_path("/tmp/termlet.a'bash"));
        assert!(!is_safe_temp_path("/etc/passwd"));
        assert!(!is_safe_temp_path("/tmp/termlet.a b"));
        assert!(!is_safe_temp_path(
            "/tmp/termlet.abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyz"
        ));
    }

    /// End-to-end check against a local sshd.
    ///
    /// Run explicitly with `cargo test -- --ignored ssh::shell_integration`.
    /// Requires the current user's SSH key authorized for 127.0.0.1 and the
    /// host key in `known_hosts`:
    ///
    ///   ssh-keyscan -p 22 127.0.0.1 >> ~/.`ssh/known_hosts`
    ///   cat ~/.`ssh/id_ecdsa.pub` >> ~/.`ssh/authorized_keys`
    ///
    /// Skips (does not fail) when localhost SSH is unavailable, so it is safe
    /// to leave in the suite.
    #[tokio::test]
    #[ignore = "requires a local sshd"]
    async fn e2e_localhost_injection() {
        use crate::core::data::{AuthType, Host, HostId, OsType, Protocol};
        use russh::Disconnect;

        let Some(home) = std::env::var_os("HOME") else {
            return;
        };
        let username = std::env::var("USER").unwrap_or_else(|_| "root".into());
        // Pin the fingerprint to the presented key so the local known_hosts
        // ordering (multiple keys per host) can't cause a mismatch.
        let Ok(key) = crate::ssh::client::probe_host_key("127.0.0.1".into(), 22).await else {
            eprintln!("skipping e2e (no local sshd)");
            return;
        };
        let host = Host {
            id: HostId(0),
            name: "localhost".into(),
            hostname: "127.0.0.1".into(),
            port: 22,
            protocol: Protocol::Ssh,
            username,
            auth_type: AuthType::Key,
            jump_host_id: None,
            identity_id: None,
            key_path: Some(
                std::path::Path::new(&home)
                    .join(".ssh/id_ecdsa")
                    .to_string_lossy()
                    .into(),
            ),
            key_passphrase: None,
            password: None,
            os_type: OsType::Unknown,
            tags: vec![],
            expected_fingerprint: Some(key.fingerprint),
        };

        let session = match crate::ssh::open_authenticated_session(&host).await {
            Ok(s) => s,
            Err(e) => {
                eprintln!("skipping e2e (no local sshd/auth): {e}");
                return;
            }
        };

        let si = match prepare_shell_integration(&session).await {
            Ok(Some(si)) => si,
            other => {
                eprintln!("skipping e2e (injection skipped/failed): {other:?}");
                let _ = session
                    .disconnect(Disconnect::ByApplication, "", "English")
                    .await;
                return;
            }
        };
        assert_ne!(si.kind, RemoteShell::Other, "unexpected unsupported shell");
        eprintln!(
            "injected: kind={:?} path={} launch={:?}",
            si.kind, si.path, si.launch
        );

        run_shell_and_verify_osc(&session, &si.launch).await;

        let check = crate::ssh::client::ssh_exec_command(
            host,
            format!("test -e '{}' && echo STILL-EXISTS || echo GONE", si.path),
        )
        .await
        .unwrap();
        assert!(check.contains("GONE"), "temp file not cleaned: {check:?}");
    }

    /// Explicitly exercise the **bash** path (the most common remote shell).
    /// Prepares a bash hook script manually (the default localhost shell here
    /// is zsh, so `prepare_shell_integration` would not pick bash).
    #[tokio::test]
    #[ignore = "requires a local sshd"]
    async fn e2e_localhost_bash_injection() {
        use crate::core::data::{AuthType, Host, HostId, OsType, Protocol};
        use russh::Disconnect;

        let Some(home) = std::env::var_os("HOME") else {
            return;
        };
        let username = std::env::var("USER").unwrap_or_else(|_| "root".into());
        let Ok(key) = crate::ssh::client::probe_host_key("127.0.0.1".into(), 22).await else {
            eprintln!("skipping e2e (no local sshd)");
            return;
        };
        let host = Host {
            id: HostId(0),
            name: "localhost".into(),
            hostname: "127.0.0.1".into(),
            port: 22,
            protocol: Protocol::Ssh,
            username,
            auth_type: AuthType::Key,
            jump_host_id: None,
            identity_id: None,
            key_path: Some(
                std::path::Path::new(&home)
                    .join(".ssh/id_ecdsa")
                    .to_string_lossy()
                    .into(),
            ),
            key_passphrase: None,
            password: None,
            os_type: OsType::Unknown,
            tags: vec![],
            expected_fingerprint: Some(key.fingerprint),
        };
        let session = match crate::ssh::open_authenticated_session(&host).await {
            Ok(s) => s,
            Err(e) => {
                eprintln!("skipping e2e (no local sshd/auth): {e}");
                return;
            }
        };

        let path = match prep_channel(
            &session,
            WRITE_FILE_CMD,
            Some(script_for(RemoteShell::Bash).as_bytes()),
        )
        .await
        .map(|p| p.trim().to_string())
        {
            Ok(p) if is_safe_temp_path(&p) => p,
            other => {
                eprintln!("skipping e2e (bash prep failed): {other:?}");
                let _ = session
                    .disconnect(Disconnect::ByApplication, "", "English")
                    .await;
                return;
            }
        };
        let launch = launch_cmd(RemoteShell::Bash, &path).unwrap();
        eprintln!("bash injected: path={path} launch={launch:?}");

        run_shell_and_verify_osc(&session, &launch).await;

        let check = crate::ssh::client::ssh_exec_command(
            host,
            format!("test -e '{path}' && echo STILL-EXISTS || echo GONE"),
        )
        .await
        .unwrap();
        assert!(
            check.contains("GONE"),
            "bash temp file not cleaned: {check:?}"
        );
    }

    /// Shared E2E body: exec a shell-under-integration on a pty, exercise
    /// `cd`/`pwd` and `false`, and assert the OSC 7 / 633;D / 633;A output.
    async fn run_shell_and_verify_osc(session: &client::Handle<Client>, launch: &str) {
        let ch = session.channel_open_session().await.unwrap();
        ch.request_pty(false, "xterm-256color", 80, 24, 0, 0, &[])
            .await
            .unwrap();
        ch.exec(false, launch.as_bytes()).await.unwrap();
        let mut stream = ch.into_stream();

        let mut buf = [0u8; 4096];
        let prompt_marker: &[u8] = b"\x1b]633;A";
        // Wait until the first prompt marker arrives (shell is up).
        let mut boot: Vec<u8> = Vec::new();
        let _ = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                match stream.read(&mut buf).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        boot.extend_from_slice(&buf[..n]);
                        if boot
                            .windows(prompt_marker.len())
                            .any(|w| w == prompt_marker)
                        {
                            break;
                        }
                    }
                }
            }
        })
        .await;
        eprintln!(
            "boot: prompt marker seen = {}",
            boot.windows(prompt_marker.len())
                .any(|w| w == prompt_marker)
        );

        // Exercise working-directory and exit-code reporting.
        stream.write_all(b"cd /tmp && pwd\n").await.unwrap();
        stream.write_all(b"false\n").await.unwrap();

        let mut extra: Vec<u8> = Vec::new();
        let _ = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                match stream.read(&mut buf).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => extra.extend_from_slice(&buf[..n]),
                }
            }
        })
        .await;
        let extra_text = String::from_utf8_lossy(&extra);
        eprintln!("command output: {extra_text:?}");
        assert!(
            extra_text.contains("\x1b]633;A"),
            "no 633;A: {extra_text:?}"
        );
        assert!(
            extra_text.contains("\x1b]633;B"),
            "no 633;B: {extra_text:?}"
        );
        assert!(
            extra_text.contains("\x1b]633;E"),
            "no 633;E: {extra_text:?}"
        );
        assert!(
            extra_text.contains("\x1b]633;C"),
            "no 633;C: {extra_text:?}"
        );
        assert!(
            extra_text.contains("\x1b]7;file://"),
            "no OSC7: {extra_text:?}"
        );
        assert!(
            extra_text.contains("\x1b]633;D;0"),
            "no 633;D;0 (cd/pwd): {extra_text:?}"
        );
        assert!(
            extra_text.contains("\x1b]633;D;1"),
            "no 633;D;1 (false): {extra_text:?}"
        );

        // Exit and wait for the channel to close (trap cleanup runs on EXIT).
        stream.write_all(b"exit\n").await.unwrap();
        let _ = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                match stream.read(&mut buf).await {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {}
                }
            }
        })
        .await;
        drop(stream);
    }
}
