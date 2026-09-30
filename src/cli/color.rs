//! Resolve color settings and scan raw arguments for `--color` before clap parses them.

use std::ffi::OsString;

use clap::ColorChoice;

/// The outcome of resolving color inputs.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct ColorResolution {
    /// Selected mode; consumers resolve `Auto` using their own terminal detection.
    pub mode: ColorChoice,
    /// Warning for an invalid `BATFILES_COLOR` value.
    pub warning: Option<String>,
}

impl ColorResolution {
    /// Return whether diagnostics should be colored, using `stderr_is_terminal` for `Auto`.
    pub(crate) fn enabled(&self, stderr_is_terminal: bool) -> bool {
        match self.mode {
            ColorChoice::Always => true,
            ColorChoice::Never => false,
            ColorChoice::Auto => stderr_is_terminal,
        }
    }
}

/// Resolve `--color > BATFILES_COLOR > non-empty NO_COLOR > auto`.
///
/// Environment values must be decoded already. `None` means unset; `Some("")` means set but
/// empty. Invalid `BATFILES_COLOR` values warn and fall back.
pub(crate) fn resolve(
    choice: Option<ColorChoice>,
    batfiles_color: Option<&str>,
    no_color: Option<&str>,
) -> ColorResolution {
    let mut warning = None;

    let selected = choice.or_else(|| match batfiles_color {
        None | Some("") => None,
        Some(raw) => parse_choice(raw).or_else(|| {
            warning = Some(format!(
                "ignoring invalid BATFILES_COLOR value `{raw}`; expected auto, always, or never"
            ));
            None
        }),
    });

    let mode = selected.unwrap_or(match no_color {
        Some(value) if !value.is_empty() => ColorChoice::Never,
        _ => ColorChoice::Auto,
    });

    ColorResolution { mode, warning }
}

/// Find the last valid `--color` value before `--` in raw arguments. Ignore missing or invalid
/// values; clap reports them during parsing.
pub(crate) fn preparse_choice(args: &[OsString]) -> Option<ColorChoice> {
    let mut found = None;
    // Skip the program name.
    let mut args = args.iter().skip(1);

    while let Some(arg) = args.next() {
        let Some(arg) = arg.to_str() else { continue };
        if arg == "--" {
            break;
        }

        let value = match arg {
            "--color" => args.next().and_then(|value| value.to_str()),
            _ => arg.strip_prefix("--color="),
        };
        if let Some(choice) = value.and_then(parse_choice) {
            // Last one wins, matching how clap resolves a repeated option.
            found = Some(choice);
        }
    }

    found
}

/// Parse the case-sensitive values `auto`, `always`, and `never`.
fn parse_choice(raw: &str) -> Option<ColorChoice> {
    raw.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_selected_leaves_auto_unresolved() {
        assert_eq!(resolve(None, None, None).mode, ColorChoice::Auto);
    }

    #[test]
    fn the_option_outranks_the_environment() {
        assert_eq!(
            resolve(Some(ColorChoice::Always), Some("never"), Some("1")).mode,
            ColorChoice::Always
        );
    }

    #[test]
    fn batfiles_color_outranks_no_color() {
        assert_eq!(
            resolve(None, Some("always"), Some("1")).mode,
            ColorChoice::Always
        );
    }

    #[test]
    fn no_color_acts_as_never_only_when_non_empty() {
        assert_eq!(resolve(None, None, Some("1")).mode, ColorChoice::Never);
        assert_eq!(resolve(None, None, Some("")).mode, ColorChoice::Auto);
    }

    #[test]
    fn an_invalid_batfiles_color_warns_and_falls_back() {
        let resolution = resolve(None, Some("sometimes"), Some("1"));
        assert_eq!(
            resolution.mode,
            ColorChoice::Never,
            "falls through to NO_COLOR"
        );
        assert!(resolution.warning.is_some());
    }

    #[test]
    fn an_invalid_batfiles_color_falls_back_to_auto_without_no_color() {
        let resolution = resolve(None, Some("sometimes"), None);
        assert_eq!(resolution.mode, ColorChoice::Auto);
        assert!(resolution.warning.is_some());
    }

    #[test]
    fn an_empty_batfiles_color_is_unset_rather_than_invalid() {
        let resolution = resolve(None, Some(""), None);
        assert_eq!(resolution.mode, ColorChoice::Auto);
        assert!(
            resolution.warning.is_none(),
            "an empty value should not be reported"
        );

        assert_eq!(resolve(None, Some(""), Some("1")).mode, ColorChoice::Never);
    }

    fn preparse(args: &[&str]) -> Option<ColorChoice> {
        let args: Vec<OsString> = args.iter().map(OsString::from).collect();
        preparse_choice(&args)
    }

    #[test]
    fn preparse_finds_the_option_in_either_form() {
        assert_eq!(
            preparse(&["batfiles", "--color", "always", "sync"]),
            Some(ColorChoice::Always)
        );
        assert_eq!(
            preparse(&["batfiles", "--color=never", "sync"]),
            Some(ColorChoice::Never)
        );
    }

    #[test]
    fn preparse_finds_the_option_after_the_command() {
        assert_eq!(
            preparse(&["batfiles", "sync", "--color", "auto"]),
            Some(ColorChoice::Auto)
        );
    }

    #[test]
    fn preparse_takes_the_last_selection() {
        let args = &["batfiles", "--color=always", "sync", "--color", "never"];
        assert_eq!(preparse(args), Some(ColorChoice::Never));
    }

    #[test]
    fn preparse_leaves_anything_it_cannot_interpret_to_clap() {
        assert_eq!(preparse(&["batfiles", "sync"]), None);
        assert_eq!(preparse(&["batfiles", "--color"]), None);
        assert_eq!(preparse(&["batfiles", "--color", "sometimes"]), None);
        assert_eq!(preparse(&["batfiles", "--color=sometimes"]), None);
        assert_eq!(preparse(&["batfiles", "--colorize", "always"]), None);
    }

    #[test]
    fn preparse_ignores_the_program_name_and_values_after_a_double_dash() {
        assert_eq!(preparse(&["--color=always"]), None);
        assert_eq!(
            preparse(&["batfiles", "vars", "set", "k", "--", "--color=always"]),
            None
        );
    }

    #[test]
    fn a_valid_selection_produces_no_warning() {
        assert!(resolve(None, Some("never"), None).warning.is_none());
    }
}
