//! Shell integration snippets.
//!
//! `esh --exec` runs commands in a child process, so commands that change the
//! shell's own state (`cd`, `export`, `source`, `alias`, ...) have no effect on
//! the shell you typed in. Sourcing the matching snippet defines an `esh`
//! function that instead evaluates `--exec` commands in the current shell.
//!
//! The snippet calls `esh` by absolute path (the binary that generated it), so
//! it also works before the binary is installed on `PATH`.

use std::path::Path;

use crate::error::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shell {
    Posix,
    Fish,
}

const POSIX: &str = r#"# esh shell integration for bash and zsh.
# Add this to your ~/.bashrc or ~/.zshrc:
#
#   eval "$(esh shell-init zsh)"
#
# With it loaded, `esh --exec` evaluates the command in the current shell, so
# builtins like `cd`, `export`, and `source` take effect as if you typed them.
esh() {
  local __esh_exec=0 __esh_yes=0 __esh_pass=0 __esh_have=0
  local __esh_arg __esh_first
  local -a __esh_args=()

  for __esh_arg in "$@"; do
    case "$__esh_arg" in
      -x|--exec) __esh_exec=1 ;;
      -y|--yes)  __esh_yes=1 ;;
      -h|--help|-V|--version) __esh_pass=1 ;;
      *)
        [ "$__esh_have" -eq 0 ] && __esh_first=$__esh_arg
        __esh_args+=("$__esh_arg")
        __esh_have=1
        ;;
    esac
  done

  # Subcommands and anything that is not an explicit, non-empty exec request
  # are passed straight through to the binary.
  case "$__esh_first" in
    setup|history|shell-init) __esh_pass=1 ;;
  esac
  if [ "$__esh_exec" -eq 0 ] || [ "$__esh_have" -eq 0 ] || [ "$__esh_pass" -eq 1 ]; then
    command __ESH_BINARY__ "$@"
    return $?
  fi

  local __esh_cmd
  __esh_cmd=$(command __ESH_BINARY__ "${__esh_args[@]}") || return $?
  printf '%s\n' "$__esh_cmd" >&2

  if [ "$__esh_yes" -eq 0 ]; then
    local __esh_ans
    printf 'Run this command? [y/N] ' >&2
    IFS= read -r __esh_ans || return 1
    case "$__esh_ans" in
      [yY]|[yY][eE][sS]) ;;
      *) printf 'esh: aborted\n' >&2; return 0 ;;
    esac
  fi

  eval "$__esh_cmd"
}
"#;

const FISH: &str = r#"# esh shell integration for fish.
# Add this to your ~/.config/fish/config.fish:
#
#   esh shell-init fish | source
#
# With it loaded, `esh --exec` evaluates the command in the current shell, so
# builtins like `cd`, `export`, and `source` take effect as if you typed them.
function esh
    set -l __esh_exec 0
    set -l __esh_yes 0
    set -l __esh_pass 0
    set -l __esh_first
    set -l __esh_args

    for __esh_arg in $argv
        switch $__esh_arg
            case -x --exec
                set __esh_exec 1
            case -y --yes
                set __esh_yes 1
            case -h --help -V --version
                set __esh_pass 1
            case '*'
                if test -z "$__esh_first"
                    set __esh_first $__esh_arg
                end
                set __esh_args $__esh_args $__esh_arg
        end
    end

    if contains -- $__esh_first setup history shell-init
        set __esh_pass 1
    end
    if test $__esh_exec -eq 0 -o (count $__esh_args) -eq 0 -o $__esh_pass -eq 1
        command __ESH_BINARY__ $argv
        return $status
    end

    set -l __esh_cmd (command __ESH_BINARY__ $__esh_args)
    or return $status
    echo $__esh_cmd >&2

    if test $__esh_yes -eq 0
        read -l -P "Run this command? [y/N] " __esh_ans
        or return 1
        string match -qr '^[yY]' -- $__esh_ans
        or begin
            echo 'esh: aborted' >&2
            return 0
        end
    end

    eval $__esh_cmd
end
"#;

/// Returns the integration snippet for `requested`, or for the shell named by
/// `$SHELL` when none is given.
pub fn snippet(requested: Option<&str>) -> Result<String> {
    let shell = shell_for(requested)?;
    let binary = std::env::current_exe().ok();
    Ok(render(shell, binary.as_deref()))
}

fn shell_for(requested: Option<&str>) -> Result<Shell> {
    match requested {
        Some(name) => match normalize(name).as_str() {
            "bash" | "zsh" | "sh" => Ok(Shell::Posix),
            "fish" => Ok(Shell::Fish),
            other => Err(Error::other(format!(
                "unsupported shell '{other}'; supported shells are bash, zsh, and fish"
            ))),
        },
        None => {
            if normalize(&std::env::var("SHELL").unwrap_or_default()) == "fish" {
                Ok(Shell::Fish)
            } else {
                Ok(Shell::Posix)
            }
        }
    }
}

fn render(shell: Shell, binary: Option<&Path>) -> String {
    let path = binary
        .and_then(Path::to_str)
        .unwrap_or("esh")
        .to_string();
    let quoted = shell_quote(&path);
    let template = match shell {
        Shell::Posix => POSIX,
        Shell::Fish => FISH,
    };
    template.replace("__ESH_BINARY__", &quoted)
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn normalize(name: &str) -> String {
    name.rsplit('/')
        .next()
        .unwrap_or(name)
        .trim_start_matches('-')
        .to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_known_shells_including_paths() {
        assert_eq!(shell_for(Some("bash")).unwrap(), Shell::Posix);
        assert_eq!(shell_for(Some("zsh")).unwrap(), Shell::Posix);
        assert_eq!(shell_for(Some("/bin/zsh")).unwrap(), Shell::Posix);
        assert_eq!(shell_for(Some("-zsh")).unwrap(), Shell::Posix);
        assert_eq!(shell_for(Some("fish")).unwrap(), Shell::Fish);
    }

    #[test]
    fn rejects_unknown_shells() {
        let error = shell_for(Some("powershell")).unwrap_err();
        assert!(error.to_string().contains("unsupported shell"), "was: {error}");
    }

    #[test]
    fn renders_the_binary_as_an_absolute_quoted_path() {
        let out = render(Shell::Posix, Some(Path::new("/opt/esh/bin/esh")));
        assert!(out.contains("command '/opt/esh/bin/esh'"), "was: {out}");
        assert!(out.contains("esh() {"));
        assert!(out.contains("eval "));
    }

    #[test]
    fn quotes_paths_containing_spaces() {
        let out = render(Shell::Posix, Some(Path::new("/my tools/esh")));
        assert!(out.contains("command '/my tools/esh'"), "was: {out}");
    }

    #[test]
    fn fish_variant_uses_the_binary_too() {
        let out = render(Shell::Fish, Some(Path::new("/opt/esh")));
        assert!(out.contains("command '/opt/esh'"), "was: {out}");
        assert!(out.contains("function esh"));
    }

    #[test]
    fn placeholder_is_always_substituted() {
        for shell in [Shell::Posix, Shell::Fish] {
            let out = render(shell, Some(Path::new("/opt/esh")));
            assert!(!out.contains("__ESH_BINARY__"));
        }
    }
}
