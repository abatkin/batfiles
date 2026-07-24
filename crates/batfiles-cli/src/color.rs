//! Color selection, as specified by `docs/environment.md#color`.
//!
//! Color is presentation-only, so unlike the other environment inputs it is
//! resolved directly by the CLI and never passed into domain logic.

use std::ffi::OsString;

use clap::ColorChoice;

// The two color inputs arrive as `Option<&str>` pulled from the captured
// `Environment`, so this module never touches `OsString`. `OsString` survives
// only in `preparse_choice`, which scans the raw process arguments.

/// The outcome of resolving color inputs.
#[derive(Debug, PartialEq, Eq)]
pub struct ColorResolution {
    /// The selected mode. `Auto` is deliberately left unresolved so that each
    /// consumer can apply its own terminal detection.
    pub mode: ColorChoice,
    /// A diagnostic for an invalid `BATFILES_COLOR`, which falls back rather
    /// than silently selecting a different mode.
    pub warning: Option<String>,
}

impl ColorResolution {
    /// Whether batfiles' own diagnostics should be colored. `auto` follows
    /// stdout, per the specification; the caller supplies the answer so this
    /// stays testable.
    ///
    /// The mode itself is handed to clap untouched, leaving clap's terminal
    /// detection in charge of `auto` for the output clap renders.
    pub fn enabled(&self, stdout_is_terminal: bool) -> bool {
        match self.mode {
            ColorChoice::Always => true,
            ColorChoice::Never => false,
            ColorChoice::Auto => stdout_is_terminal,
        }
    }
}

/// Resolve `--color > BATFILES_COLOR > non-empty NO_COLOR > auto`.
///
/// The two environment values are pulled from the captured `Environment` and
/// passed in already decoded. "Set but empty" survives as `Some("")` and "unset"
/// as `None`; a value batfiles cannot interpret was lossily decoded upstream and
/// simply lands on the invalid branch, warning and falling back.
pub fn resolve(
    choice: Option<ColorChoice>,
    batfiles_color: Option<&str>,
    no_color: Option<&str>,
) -> ColorResolution {
    let mut warning = None;

    let selected = choice.or_else(|| match batfiles_color {
        // An absent or empty value is treated as unset, as it is for the
        // location variables, rather than as an invalid mode.
        None | Some("") => None,
        // Anything else that is not one of the three modes is invalid.
        Some(raw) => parse_choice(raw).or_else(|| {
            warning = Some(format!(
                "ignoring invalid BATFILES_COLOR value `{raw}`; expected auto, always, or never"
            ));
            None
        }),
    });

    // NO_COLOR follows the cross-tool convention: presence alone is not enough,
    // but any non-empty value counts.
    let mode = selected.unwrap_or(match no_color {
        Some(value) if !value.is_empty() => ColorChoice::Never,
        _ => ColorChoice::Auto,
    });

    ColorResolution { mode, warning }
}

/// Recover `--color` from the raw arguments before clap parses them.
///
/// Presentation has to be settled before clap can render `--help`, `--version`,
/// or a usage error, so the option is located here rather than read off the
/// parsed `Cli`. This scan is deliberately forgiving: anything it cannot
/// interpret — a missing value, an unrecognized value, a value after `--` — is
/// left to clap, which reports it properly.
pub fn preparse_choice(args: &[OsString]) -> Option<ColorChoice> {
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

/// `ColorChoice`'s own case-sensitive parse of `auto`, `always`, and `never`,
/// so the option and `BATFILES_COLOR` accept exactly the same spellings.
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
    fn auto_follows_stdout_for_our_own_output() {
        let resolution = resolve(None, None, None);
        assert!(resolution.enabled(true));
        assert!(!resolution.enabled(false));
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

        // Being unset, it leaves the next input in precedence to decide.
        assert_eq!(resolve(None, Some(""), Some("1")).mode, ColorChoice::Never);
    }

    #[test]
    fn an_explicit_mode_ignores_the_terminal() {
        let always = resolve(Some(ColorChoice::Always), None, None);
        assert!(always.enabled(false));

        let never = resolve(Some(ColorChoice::Never), None, None);
        assert!(!never.enabled(true));
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
