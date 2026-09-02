//! Argv parsing. We hand-extract global flags and identify subcommand+rest,
//! then defer command-specific flag/arg parsing to the strict parsers in each
//! built-in module or to the manifest-driven parser in `runner.rs`.

#[derive(Debug, Clone, Default)]
pub struct Globals {
    pub json: bool,
    pub quiet: bool,
    pub clipboard: bool,
}

#[derive(Debug, Clone)]
pub struct Parsed {
    pub rest: Vec<String>,
    pub globals: Globals,
    pub argv0: String,
    /// Errors recognized while extracting globals. Keeping them separate from
    /// `rest` ensures removed global options do not become command names and
    /// accidentally take an Unknown/127 or destructive fallback path.
    pub errors: Vec<String>,
}

pub fn parse(argv: &[String]) -> Parsed {
    let argv0 = argv
        .first()
        .cloned()
        .unwrap_or_else(|| "notemd".to_string());
    let mut globals = Globals {
        clipboard: true, // default-on; --no-clipboard flips it
        ..Default::default()
    };
    let mut rest = Vec::with_capacity(argv.len().saturating_sub(1));
    let mut errors = Vec::new();
    let mut i = 1;
    let mut positional_only = false;
    while i < argv.len() {
        let a = &argv[i];
        if positional_only {
            rest.push(a.clone());
            i += 1;
            continue;
        }
        match a.as_str() {
            // Preserve the separator for the command-specific strict parser and
            // stop treating later tokens as global flags. This is the escape
            // hatch for a query/task/path literally named `--json` or `-q`.
            "--" => {
                positional_only = true;
                rest.push(a.clone());
            }
            "--cli" => { /* consumed by mode dispatch; drop */ }
            "--json" => globals.json = true,
            "-q" | "--quiet" => globals.quiet = true,
            "--no-clipboard" => globals.clipboard = false,
            "-y" | "--yes" => errors.push(format!("unsupported global option '{a}'")),
            _ => rest.push(a.clone()),
        }
        i += 1;
    }
    Parsed {
        rest,
        globals,
        argv0,
        errors,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn s(args: &[&str]) -> Parsed {
        parse(&args.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }
    #[test]
    fn strips_globals_keeps_subcommand_and_args() {
        let p = s(&["notemd", "--json", "share", "draft.md", "-q"]);
        assert_eq!(p.rest, vec!["share".to_string(), "draft.md".to_string()]);
        assert!(p.globals.json);
        assert!(p.globals.quiet);
    }
    #[test]
    fn alias_short_flag_survives() {
        let p = s(&["notemd", "-s", "x.md"]);
        assert_eq!(p.rest, vec!["-s".to_string(), "x.md".to_string()]);
        assert!(!p.globals.json);
    }
    #[test]
    fn clipboard_defaults_on() {
        let p = s(&["notemd", "help"]);
        assert!(p.globals.clipboard);
    }
    #[test]
    fn no_clipboard_flips_it() {
        let p = s(&["notemd", "--no-clipboard", "share", "x.md"]);
        assert!(!p.globals.clipboard);
    }
    #[test]
    fn cli_flag_is_dropped() {
        let p = s(&["notemd", "--cli", "help"]);
        assert_eq!(p.rest, vec!["help".to_string()]);
    }

    #[test]
    fn double_dash_preserves_later_global_looking_tokens() {
        let p = s(&["notemd", "search", "--", "--json", "-q", "--cli"]);
        assert_eq!(p.rest, vec!["search", "--", "--json", "-q", "--cli"]);
        assert!(!p.globals.json);
        assert!(!p.globals.quiet);
    }

    #[test]
    fn removed_yes_option_is_an_explicit_argument_error() {
        for option in ["-y", "--yes"] {
            let p = s(&["notemd", option, "plugin", "remove", "x"]);
            assert_eq!(p.rest, vec!["plugin", "remove", "x"]);
            assert_eq!(
                p.errors,
                vec![format!("unsupported global option '{option}'")]
            );
        }
    }
}
