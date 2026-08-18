//! Argument parsing. Hand-written: the surface is small, and a CLI parser
//! dependency is a supply-chain cost this tool does not need to pay.

use std::path::PathBuf;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(PartialEq, Eq, Clone, Copy)]
pub enum Command {
    /// Load cost of one or more entry modules.
    Cost,
    /// Re-export files and what they amplify.
    Barrels,
    /// Import cycles, classified by what they do at runtime.
    Cycles,
    /// All three, in one pass.
    Check,
}

pub struct Options {
    pub command: Command,
    pub root: PathBuf,
    pub entries: Vec<String>,
    pub tsconfig: Option<PathBuf>,
    pub json: bool,
    pub no_color: bool,
    /// Exit 1 when a finding at or above this level is present.
    pub fail_on: FailOn,
    pub top: usize,
    pub min_amplification: f64,
    pub min_cost: usize,
}

#[derive(PartialEq, Eq, Clone, Copy)]
pub enum FailOn {
    /// Never exit non-zero for findings (default for `cost` and `barrels`).
    Never,
    /// Only a cycle that throws on the project's own evaluation order.
    Crash,
    /// Anything that misbehaves at load time: a throw, a silent `undefined`,
    /// or a crash waiting for someone to deep-import into the cycle.
    Hazard,
    /// Any cycle at all, and any reported barrel.
    Any,
}

pub enum ParseOutcome {
    Run(Box<Options>),
    Help,
    Version,
    Error(String),
}

const HELP: &str = "\
overpull — measures what your imports really load.

USAGE
  overpull <command> [options] [entries...]

COMMANDS
  cost <entries...>   What importing each entry loads: modules, bytes,
                      external packages, and which direct import brought
                      each part of it.
  barrels             Re-export files and how much they amplify: the cost
                      of importing the barrel versus importing a member.
  cycles              Import cycles, classified by what they do at run time
                      (crash / crash-if-loaded-first / undefined-read /
                      cjs-mixed / benign), with the edge to break.
  check               barrels + cycles over the whole project.

OPTIONS
  --root <dir>         Project root (default: current directory).
  --tsconfig <file>    tsconfig to read `paths` from (default: auto-discover).
  --json               Machine-readable output.
  --fail-on <level>    never | crash | hazard | any
                       crash  — throws on your own entry order (default for
                                `cycles` and `check`)
                       hazard — also silent undefined reads, and crashes that
                                need a deep import to trigger
                       any    — every cycle and every reported barrel
  --top <n>            Rows per section (default: 10).
  --min-amplification <n>  Barrel amplification floor (default: 4).
  --min-cost <n>       Barrel load-cost floor, in modules (default: 20).
  --no-color           Disable ANSI color (also honours NO_COLOR).
  -h, --help           Show this help.
  -V, --version        Show version.

EXAMPLES
  overpull cost src/index.ts
  overpull barrels --root packages/ui
  overpull cycles --fail-on crash
  overpull check --json > overpull.json

EXIT CODES
  0  no findings at or above --fail-on
  1  findings at or above --fail-on
  2  usage error or nothing to analyze
";

pub fn parse<I: IntoIterator<Item = String>>(args: I) -> ParseOutcome {
    let mut args = args.into_iter();
    let Some(first) = args.next() else {
        return ParseOutcome::Help;
    };

    let command = match first.as_str() {
        "cost" => Command::Cost,
        "barrels" => Command::Barrels,
        "cycles" => Command::Cycles,
        "check" => Command::Check,
        "-h" | "--help" | "help" => return ParseOutcome::Help,
        "-V" | "--version" | "version" => return ParseOutcome::Version,
        other => {
            return ParseOutcome::Error(format!(
                "unknown command `{other}`. Expected one of: cost, barrels, cycles, check.\n\
                 Run `overpull --help` for usage."
            ));
        }
    };

    let mut options = Options {
        command,
        root: PathBuf::from("."),
        entries: Vec::new(),
        tsconfig: None,
        json: false,
        no_color: false,
        fail_on: match command {
            Command::Cycles | Command::Check => FailOn::Crash,
            Command::Cost | Command::Barrels => FailOn::Never,
        },
        top: 10,
        min_amplification: 4.0,
        min_cost: 20,
    };

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => return ParseOutcome::Help,
            "-V" | "--version" => return ParseOutcome::Version,
            "--json" => options.json = true,
            "--no-color" => options.no_color = true,
            "--root" => match args.next() {
                Some(value) => options.root = PathBuf::from(value),
                None => return ParseOutcome::Error("--root needs a directory".into()),
            },
            "--tsconfig" => match args.next() {
                Some(value) => options.tsconfig = Some(PathBuf::from(value)),
                None => return ParseOutcome::Error("--tsconfig needs a file path".into()),
            },
            "--fail-on" => match args.next().as_deref() {
                Some("never") => options.fail_on = FailOn::Never,
                Some("crash") => options.fail_on = FailOn::Crash,
                Some("hazard") => options.fail_on = FailOn::Hazard,
                Some("any") => options.fail_on = FailOn::Any,
                Some(other) => {
                    return ParseOutcome::Error(format!(
                        "--fail-on expects never, crash, hazard or any (got `{other}`)"
                    ));
                }
                None => return ParseOutcome::Error("--fail-on needs a level".into()),
            },
            "--top" => match args.next().map(|v| v.parse::<usize>()) {
                Some(Ok(n)) if n > 0 => options.top = n,
                _ => return ParseOutcome::Error("--top needs a positive number".into()),
            },
            "--min-amplification" => match args.next().map(|v| v.parse::<f64>()) {
                Some(Ok(n)) if n.is_finite() && n >= 1.0 => options.min_amplification = n,
                _ => {
                    return ParseOutcome::Error(
                        "--min-amplification needs a number of at least 1".into(),
                    );
                }
            },
            "--min-cost" => match args.next().map(|v| v.parse::<usize>()) {
                Some(Ok(n)) => options.min_cost = n,
                _ => return ParseOutcome::Error("--min-cost needs a number".into()),
            },
            other if other.starts_with('-') => {
                return ParseOutcome::Error(format!(
                    "unknown option `{other}`. Run `overpull --help` for usage."
                ));
            }
            entry => options.entries.push(entry.to_string()),
        }
    }

    if command == Command::Cost && options.entries.is_empty() {
        return ParseOutcome::Error(
            "`overpull cost` needs at least one entry file.\n\
             Example: overpull cost src/index.ts"
                .into(),
        );
    }

    ParseOutcome::Run(Box::new(options))
}

pub fn help_text() -> &'static str {
    HELP
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_args(args: &[&str]) -> ParseOutcome {
        parse(args.iter().map(|s| (*s).to_string()))
    }

    #[test]
    fn parses_cost_with_entry() {
        let ParseOutcome::Run(options) = parse_args(&["cost", "src/index.ts"]) else {
            panic!("expected run");
        };
        assert!(options.command == Command::Cost);
        assert_eq!(options.entries, vec!["src/index.ts"]);
        assert!(options.fail_on == FailOn::Never);
    }

    #[test]
    fn cost_without_entry_is_an_error() {
        assert!(matches!(parse_args(&["cost"]), ParseOutcome::Error(_)));
    }

    #[test]
    fn cycles_defaults_to_failing_on_crash() {
        let ParseOutcome::Run(options) = parse_args(&["cycles"]) else {
            panic!()
        };
        assert!(options.fail_on == FailOn::Crash);
    }

    #[test]
    fn rejects_unknown_option_and_command() {
        assert!(matches!(
            parse_args(&["cycles", "--nope"]),
            ParseOutcome::Error(_)
        ));
        assert!(matches!(
            parse_args(&["frobnicate"]),
            ParseOutcome::Error(_)
        ));
    }

    #[test]
    fn no_args_shows_help() {
        assert!(matches!(parse(Vec::<String>::new()), ParseOutcome::Help));
    }

    #[test]
    fn rejects_out_of_range_numbers() {
        assert!(matches!(
            parse_args(&["barrels", "--top", "0"]),
            ParseOutcome::Error(_)
        ));
        assert!(matches!(
            parse_args(&["barrels", "--min-amplification", "0.5"]),
            ParseOutcome::Error(_)
        ));
    }
}
