//! The `man` page renderer (§16.2 shell integration): `halyard man`
//! renders the `halyard(1)` page, `halyard man <command>` one
//! subcommand's page — roff, deterministic (I3), derived from the clap
//! definition itself (the single source of truth the completions
//! renderer uses), so a flag added to the parser appears on the page
//! without a second edit. Pages print to stdout (the `schema` and
//! `completions` convention) and are byte-stable across runs.

use std::fmt::Write as _;

use clap::Command as ClapCommand;

/// Escapes a roff text fragment: a literal `-` starts an option or a
/// control line in roff, so every dash renders `\-` (the mangen
/// convention — prose hyphens included, which groff renders as a
/// hyphen), and a backslash doubles.
fn esc(text: &str) -> String {
    text.replace('\\', "\\\\").replace('-', "\\-")
}

/// The one-line summary of an argument for an OPTIONS entry: `.B
/// \-\-flag \fIVALUE\fR` for options, `\fIVALUE\fR` for positionals.
fn arg_entry_line(arg: &clap::Arg) -> String {
    match arg.get_long() {
        Some(long) => {
            let flag = esc(&format!("--{long}"));
            match arg.get_value_names().and_then(|names| names.first()) {
                Some(value) => format!(".B {flag} \\fI{}\\fR", esc(value.as_str())),
                None => format!(".B {flag}"),
            }
        }
        None => match arg.get_value_names().and_then(|names| names.first()) {
            Some(value) => format!(".B \\fI{}\\fR", esc(value.as_str())),
            None => String::new(),
        },
    }
}

/// The plain synopsis fragment for one argument — no `.B` macro (the
/// usage line is regular type): `[\-\-flag \fIVALUE\fR]` for options,
/// `\fIVALUE\fR` for positionals.
fn arg_fragment(arg: &clap::Arg) -> String {
    match arg.get_long() {
        Some(long) => {
            let flag = esc(&format!("--{long}"));
            match arg.get_value_names().and_then(|names| names.first()) {
                Some(value) => format!("[{flag} \\fI{}\\fR]", esc(value.as_str())),
                None => format!("[{flag}]"),
            }
        }
        None => match arg.get_value_names().and_then(|names| names.first()) {
            Some(value) => format!("\\fI{}\\fR", esc(value.as_str())),
            None => String::new(),
        },
    }
}

/// One `\.TP`-separated entry: the argument line plus its help.
fn arg_entry(arg: &clap::Arg) -> String {
    let mut out = arg_entry_line(arg);
    out.push('\n');
    if let Some(help) = arg.get_help().or_else(|| arg.get_long_help()) {
        out.push_str(&esc(&help.to_string()));
        out.push('\n');
    }
    out
}

/// The arguments worth listing. The root page lists everything (its own
/// args are the globals — the OPTIONS section is their only home); a
/// subcommand page lists its own flags only, the propagated global
/// copies staying on the top page's OPTIONS section.
fn own_args(cmd: &ClapCommand, is_root: bool) -> Vec<&clap::Arg> {
    cmd.get_arguments()
        .filter(|arg| (is_root || !arg.is_global_set()) && !arg.is_hide_set())
        .collect()
}

/// The synopsis fragment for one command: its own arguments in
/// declaration order, bracketed like the usage string, one line.
fn synopsis_line(cmd: &ClapCommand, is_root: bool) -> String {
    let fragments: String = own_args(cmd, is_root)
        .iter()
        .filter_map(|arg| {
            let fragment = arg_fragment(arg);
            if fragment.is_empty() {
                None
            } else {
                Some(fragment)
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    if fragments.is_empty() {
        String::new()
    } else {
        let mut out = fragments;
        out.push('\n');
        out
    }
}

/// The `.SH COMMANDS` body: one `.TP` entry per subcommand.
fn commands_section(cmd: &ClapCommand) -> String {
    let mut out = String::from(".SH COMMANDS\n");
    for sub in cmd.get_subcommands() {
        out.push_str(".TP\n");
        // The subcommand name is a parser identifier, not prose: it is
        // not escaped (a `\B` macro argument never starts a line).
        let _ = writeln!(out, ".B {}", sub.get_name());
        if let Some(about) = sub.get_about().or_else(|| sub.get_long_about()) {
            out.push_str(&esc(&about.to_string()));
            out.push('\n');
        }
    }
    out
}

/// The `.SH OPTIONS` body: one `.TP` entry per own argument; an empty
/// section is omitted (a page never renders a bare heading).
fn options_section(cmd: &ClapCommand, is_root: bool) -> String {
    let args = own_args(cmd, is_root);
    if args.is_empty() {
        return String::new();
    }
    let mut out = String::from(".SH OPTIONS\n");
    for arg in args {
        out.push_str(".TP\n");
        out.push_str(&arg_entry(arg));
    }
    out
}

/// The exit-code table (§15.4) — the top page's contract with CI.
const EXIT_CODES: [(&str, &str); 6] = [
    ("0", "ok / nothing to do"),
    ("1", "generic failure"),
    ("2", "usage / configuration"),
    ("3", "plan has a pending release (`check --exit-code`)"),
    ("4", "partial publish (`publish`, retryable)"),
    ("5", "forge rate-limited (retryable)"),
];

/// Renders one page over the given command: `.TH`/NAME/SYNOPSIS/
/// DESCRIPTION, then COMMANDS (when the command has subcommands) and
/// OPTIONS (when it has own arguments). The root page carries the
/// §15.4 exit-code table.
fn render_page(cmd: &ClapCommand, is_root: bool) -> String {
    let title = if is_root {
        String::from(".TH HALYARD 1\n")
    } else {
        let upper = cmd.get_name().to_uppercase();
        format!(".TH HALYARD-{} 1\n", esc(&upper))
    };
    let mut out = title;
    out.push_str(".SH NAME\n");
    let name_line = if is_root {
        format!(
            "{} \\- {}",
            esc(cmd.get_name()),
            esc(cmd
                .get_about()
                .map(ToString::to_string)
                .as_deref()
                .unwrap_or("the halyard CLI"))
        )
    } else {
        let about = cmd
            .get_about()
            .or_else(|| cmd.get_long_about())
            .map(ToString::to_string)
            .unwrap_or_default();
        format!("halyard\\-{} \\- {}", esc(cmd.get_name()), esc(&about))
    };
    out.push_str(&name_line);
    out.push('\n');
    out.push_str(".SH SYNOPSIS\n");
    out.push_str(".B ");
    if is_root {
        out.push_str("halyard");
    } else {
        let _ = write!(out, "halyard {}", esc(cmd.get_name()));
    }
    out.push('\n');
    let synopsis = synopsis_line(cmd, is_root);
    if !synopsis.is_empty() {
        out.push_str(&synopsis);
    }
    if is_root {
        out.push_str("<COMMAND>\n");
    }
    out.push_str(".SH DESCRIPTION\n");
    let description = cmd
        .get_long_about()
        .or_else(|| cmd.get_about())
        .map(ToString::to_string)
        .unwrap_or_default();
    out.push_str(&esc(&description));
    out.push('\n');
    if is_root && cmd.has_subcommands() {
        out.push_str(&commands_section(cmd));
    }
    out.push_str(&options_section(cmd, is_root));
    if is_root {
        out.push_str(".SH EXIT CODES\n");
        for (code, meaning) in EXIT_CODES {
            out.push_str(".TP\n");
            let _ = writeln!(out, ".B {code}");
            out.push_str(&esc(meaning));
            out.push('\n');
        }
    }
    out
}

/// The `halyard(1)` top page over the built command tree.
pub fn top_page(cmd: &ClapCommand) -> String {
    render_page(cmd, true)
}

/// One subcommand's page (`halyard man <name>`); `None` for a name with
/// no subcommand — unreachable through the CLI's value enum, kept
/// explicit so the renderer stays total over any string.
pub fn command_page(cmd: &ClapCommand, name: &str) -> Option<String> {
    cmd.find_subcommand(name).map(|sub| render_page(sub, false))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A mirror of the CLI tree for the unit tests (the renderer is a
    /// pure function of the tree; the real tree comes from the bin's
    /// derive, which a unit test of the lib target cannot see).
    fn mirror() -> ClapCommand {
        ClapCommand::new("halyard")
            .about("the halyard CLI")
            .subcommand(ClapCommand::new("plan").about("Compute the plan"))
            .subcommand(
                ClapCommand::new("affected")
                    .about("The affected-package selector")
                    .arg(clap::Arg::new("base").long("base").value_name("REF")),
            )
    }

    #[test]
    fn top_page_names_every_subcommand() {
        let page = top_page(&mirror());
        assert!(page.starts_with(".TH HALYARD 1\n"));
        assert!(page.contains(".SH COMMANDS"));
        assert!(page.contains(".B plan\n"));
        assert!(page.contains(".B affected\n"));
    }

    #[test]
    fn command_pages_escape_dashes() {
        let root = mirror();
        let page = command_page(&root, "affected").expect("affected exists");
        assert!(page.contains(".SH NAME"));
        assert!(page.contains("halyard\\-affected"));
        assert!(page.contains("\\-\\-base"));
        assert!(page.contains(".B halyard affected"));
    }

    #[test]
    fn unknown_names_are_none() {
        assert!(command_page(&mirror(), "does-not-exist").is_none());
    }

    #[test]
    fn pages_are_deterministic() {
        let root = mirror();
        assert_eq!(top_page(&root), top_page(&root));
        assert_eq!(
            command_page(&root, "plan").expect("plan exists"),
            command_page(&root, "plan").expect("plan exists")
        );
    }
}
