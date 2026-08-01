//! Argv parsing. We hand-extract global flags and identify subcommand+rest,
//! then defer flag/arg parsing for plugin subcommands to a clap Command built
//! dynamically from the manifest's `cli` entry inside `runner.rs`.

#[derive(Debug, Clone, Default)]
pub struct Globals {
    pub json: bool,
    pub quiet: bool,
    pub clipboard: bool,
    pub yes: bool,
}

#[derive(Debug, Clone)]
pub struct Parsed {
    pub rest: Vec<String>,
    pub globals: Globals,
    pub argv0: String,
}

pub fn parse(argv: &[String]) -> Parsed {
    let argv0 = argv.first().cloned().unwrap_or_else(|| "notemd".to_string());
    let mut globals = Globals {
        clipboard: true,        // default-on; --no-clipboard flips it
        ..Default::default()
    };
    let mut rest = Vec::with_capacity(argv.len().saturating_sub(1));
    let mut i = 1;
    while i < argv.len() {
        let a = &argv[i];
        match a.as_str() {
            "--cli" => { /* consumed by mode dispatch; drop */ }
            "--json" => globals.json = true,
            "-q" | "--quiet" => globals.quiet = true,
            "--no-clipboard" => globals.clipboard = false,
            "-y" | "--yes" => globals.yes = true,
            _ => rest.push(a.clone()),
        }
        i += 1;
    }
    Parsed { rest, globals, argv0 }
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
}
