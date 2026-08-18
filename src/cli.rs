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
    /// Why is this module in the graph at all?
    Why,
}

pub struct Options {
    pub command: Command,
    pub root: PathBuf,
    /// Positional arguments: entry files for `cost`, the target for `why`.
    pub entries: Vec<String>,
    /// `--entry` overrides: which modules count as real program starts.
    pub entry_files: Vec<String>,
    pub tsconfig: Option<PathBuf>,
    pub json: bool,
    pub sarif: bool,
    pub no_color: bool,
    /// Exit 1 when a finding at or above this level is present.
    pub fail_on: FailOn,
    pub top: usize,
    pub min_amplification: f64,
    pub min_cost: usize,
    /// Load-cost budget for `cost`, in modules.
    pub max_modules: Option<usize>,
    /// Load-cost budget for `cost`, in bytes of source.
    pub max_bytes: Option<u64>,
    /// Findings already recorded here are known, and are not reported again.
    pub baseline: Option<PathBuf>,
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
  overpull <command> [options] [arguments...]

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
  why <module>        Shortest import chain from each entry point to that
                      module — why it is in the graph at all.

OPTIONS
  --root <dir>         Project root (default: current directory).
  --entry <file>       Treat this file as a program entry point. Repeatable.
                       Without it, entry points come from package.json,
                       conventional paths (src/index.*), and modules nothing
                       imports.
  --tsconfig <file>    tsconfig to read `paths` from (default: auto-discover).
  --json               Machine-readable output.
  --sarif              SARIF 2.1.0 output, for code-scanning dashboards.
  --baseline <file>    Suppress findings already present in this file, so a
                       first run on a large codebase reports only what is new.
                       Create it with: overpull check --json > baseline.json
  --fail-on <level>    never | crash | hazard | any
                       crash  — throws on your own entry order (default for
                                `cycles` and `check`)
                       hazard — also silent undefined reads, and crashes that
                                need a deep import to trigger
                       any    — every cycle and every reported barrel
  --max-modules <n>    `cost` budget: exit 1 if an entry loads more than this.
  --max-bytes <n>      `cost` budget in bytes; accepts 900kb, 2mb.
  --top <n>            Rows per section (default: 10).
  --min-amplification <n>  Barrel amplification floor (default: 4).
  --min-cost <n>       Barrel load-cost floor, in modules (default: 20).
  --no-color           Disable ANSI color (also honours NO_COLOR).
  -h, --help           Show this help.
  -V, --version        Show version.

EXAMPLES
  overpull cost src/index.ts --max-modules 200
  overpull barrels --root packages/ui
  overpull cycles --fail-on crash --entry src/server.ts
  overpull why src/legacy/config.ts
  overpull check --json > overpull.json
  overpull check --baseline overpull.json --fail-on hazard

EXIT CODES
  0  no findings at or above --fail-on, and no budget exceeded
  1  findings at or above --fail-on, or a budget exceeded
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
        "why" => Command::Why,
        "-h" | "--help" | "help" => return ParseOutcome::Help,
        "-V" | "--version" | "version" => return ParseOutcome::Version,
        other => {
            return ParseOutcome::Error(format!(
                "unknown command `{other}`. Expected one of: cost, barrels, cycles, check, why.\n\
                 Run `overpull --help` for usage."
            ));
        }
    };

    let mut options = Options::defaults(command);
    match parse_flags(&mut options, args) {
        Err(message) => ParseOutcome::Error(message),
        Ok(Flags::Help) => ParseOutcome::Help,
        Ok(Flags::Version) => ParseOutcome::Version,
        Ok(Flags::Parsed) => match validate(&options) {
            Some(message) => ParseOutcome::Error(message),
            None => ParseOutcome::Run(Box::new(options)),
        },
    }
}

/// Whether the flag list asked for something other than a run.
enum Flags {
    Parsed,
    Help,
    Version,
}

impl Options {
    fn defaults(command: Command) -> Self {
        Self {
            command,
            root: PathBuf::from("."),
            entries: Vec::new(),
            entry_files: Vec::new(),
            tsconfig: None,
            json: false,
            sarif: false,
            no_color: false,
            fail_on: match command {
                Command::Cycles | Command::Check => FailOn::Crash,
                Command::Cost | Command::Barrels | Command::Why => FailOn::Never,
            },
            top: 10,
            min_amplification: 4.0,
            min_cost: 20,
            max_modules: None,
            max_bytes: None,
            baseline: None,
        }
    }
}

fn parse_flags<I: Iterator<Item = String>>(
    options: &mut Options,
    mut args: I,
) -> Result<Flags, String> {
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => return Ok(Flags::Help),
            "-V" | "--version" => return Ok(Flags::Version),
            "--json" => options.json = true,
            "--sarif" => options.sarif = true,
            "--no-color" => options.no_color = true,
            "--root" => options.root = PathBuf::from(value(&mut args, "--root", "a directory")?),
            "--entry" => options
                .entry_files
                .push(value(&mut args, "--entry", "a file path")?),
            "--tsconfig" => {
                options.tsconfig = Some(PathBuf::from(value(
                    &mut args,
                    "--tsconfig",
                    "a file path",
                )?));
            }
            "--baseline" => {
                options.baseline = Some(PathBuf::from(value(
                    &mut args,
                    "--baseline",
                    "a file path",
                )?));
            }
            "--fail-on" => {
                let level = value(&mut args, "--fail-on", "a level")?;
                options.fail_on = match level.as_str() {
                    "never" => FailOn::Never,
                    "crash" => FailOn::Crash,
                    "hazard" => FailOn::Hazard,
                    "any" => FailOn::Any,
                    other => {
                        return Err(format!(
                            "--fail-on expects never, crash, hazard or any (got `{other}`)"
                        ));
                    }
                };
            }
            "--top" => {
                options.top = positive(&mut args, "--top")?;
            }
            "--max-modules" => {
                options.max_modules = Some(positive(&mut args, "--max-modules")?);
            }
            "--max-bytes" => {
                let raw = value(&mut args, "--max-bytes", "a size")?;
                options.max_bytes = Some(parse_bytes(&raw).ok_or_else(|| {
                    format!("--max-bytes needs a size: 480000, 900kb, or 2mb (got `{raw}`)")
                })?);
            }
            "--min-amplification" => {
                let raw = value(&mut args, "--min-amplification", "a number")?;
                let parsed: f64 = raw.parse().map_err(|_| {
                    format!("--min-amplification needs a number of at least 1 (got `{raw}`)")
                })?;
                if !parsed.is_finite() || parsed < 1.0 {
                    return Err(format!(
                        "--min-amplification needs a number of at least 1 (got `{raw}`)"
                    ));
                }
                options.min_amplification = parsed;
            }
            "--min-cost" => {
                let raw = value(&mut args, "--min-cost", "a number")?;
                options.min_cost = raw
                    .parse()
                    .map_err(|_| format!("--min-cost needs a number (got `{raw}`)"))?;
            }
            other if other.starts_with('-') => {
                return Err(format!(
                    "unknown option `{other}`. Run `overpull --help` for usage."
                ));
            }
            entry => options.entries.push(entry.to_string()),
        }
    }
    Ok(Flags::Parsed)
}

fn value<I: Iterator<Item = String>>(
    args: &mut I,
    flag: &str,
    expected: &str,
) -> Result<String, String> {
    args.next()
        .ok_or_else(|| format!("{flag} needs {expected}"))
}

fn positive<I: Iterator<Item = String>>(args: &mut I, flag: &str) -> Result<usize, String> {
    let raw = value(args, flag, "a positive number")?;
    match raw.parse::<usize>() {
        Ok(n) if n > 0 => Ok(n),
        _ => Err(format!("{flag} needs a positive number (got `{raw}`)")),
    }
}

fn validate(options: &Options) -> Option<String> {
    if options.json && options.sarif {
        return Some("--json and --sarif produce different documents; pick one".into());
    }
    match options.command {
        Command::Cost if options.entries.is_empty() => Some(
            "`overpull cost` needs at least one entry file.\n\
             Example: overpull cost src/index.ts"
                .into(),
        ),
        Command::Why if options.entries.len() != 1 => Some(
            "`overpull why` needs exactly one module.\n\
             Example: overpull why src/legacy/config.ts"
                .into(),
        ),
        _ if (options.max_modules.is_some() || options.max_bytes.is_some())
            && options.command != Command::Cost =>
        {
            Some("--max-modules and --max-bytes apply to `overpull cost`".into())
        }
        // A flag that silently does nothing is worse than one that is not
        // accepted: `cost` and `why` report no findings to gate on.
        Command::Cost if options.fail_on != FailOn::Never => Some(
            "`overpull cost` gates on a budget, not on --fail-on.
             Use --max-modules or --max-bytes."
                .into(),
        ),
        Command::Why if options.fail_on != FailOn::Never => {
            Some("`overpull why` answers a question; it has nothing to gate on".into())
        }
        _ => None,
    }
}

/// `480000`, `900kb`, `2mb`, `1.5MB` — decimal units, because that is how
/// every bundle-size budget people already write one is expressed.
fn parse_bytes(text: &str) -> Option<u64> {
    let text = text.trim();
    let lower = text.to_ascii_lowercase();
    let (number, multiplier) = if let Some(rest) = lower.strip_suffix("gb") {
        (rest, 1_000_000_000.0)
    } else if let Some(rest) = lower.strip_suffix("mb") {
        (rest, 1_000_000.0)
    } else if let Some(rest) = lower.strip_suffix("kb") {
        (rest, 1_000.0)
    } else if let Some(rest) = lower.strip_suffix('b') {
        (rest, 1.0)
    } else {
        (lower.as_str(), 1.0)
    };
    let value: f64 = number.trim().parse().ok()?;
    if !value.is_finite() || value <= 0.0 {
        return None;
    }
    let bytes = value * multiplier;
    if bytes > 1e18 {
        return None;
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    Some(bytes.round() as u64)
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

    fn run_options(args: &[&str]) -> Box<Options> {
        match parse_args(args) {
            ParseOutcome::Run(options) => options,
            _ => panic!("expected a run for {args:?}"),
        }
    }

    #[test]
    fn parses_cost_with_entry() {
        let options = run_options(&["cost", "src/index.ts"]);
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
        let options = run_options(&["cycles"]);
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

    #[test]
    fn why_needs_exactly_one_module() {
        assert!(matches!(parse_args(&["why"]), ParseOutcome::Error(_)));
        assert!(matches!(
            parse_args(&["why", "a.ts", "b.ts"]),
            ParseOutcome::Error(_)
        ));
        assert!(run_options(&["why", "src/a.ts"]).command == Command::Why);
    }

    #[test]
    fn entry_overrides_accumulate() {
        let options = run_options(&["cycles", "--entry", "src/a.ts", "--entry", "src/b.ts"]);
        assert_eq!(options.entry_files, vec!["src/a.ts", "src/b.ts"]);
    }

    #[test]
    fn byte_budgets_accept_units() {
        assert_eq!(parse_bytes("480000"), Some(480_000));
        assert_eq!(parse_bytes("900kb"), Some(900_000));
        assert_eq!(parse_bytes("2MB"), Some(2_000_000));
        assert_eq!(parse_bytes("1.5mb"), Some(1_500_000));
        assert_eq!(parse_bytes("0"), None);
        assert_eq!(parse_bytes("big"), None);
    }

    #[test]
    fn budgets_belong_to_cost_only() {
        assert!(matches!(
            parse_args(&["check", "--max-modules", "10"]),
            ParseOutcome::Error(_)
        ));
        assert_eq!(
            run_options(&["cost", "a.ts", "--max-modules", "10"]).max_modules,
            Some(10)
        );
    }

    #[test]
    fn json_and_sarif_are_mutually_exclusive() {
        assert!(matches!(
            parse_args(&["check", "--json", "--sarif"]),
            ParseOutcome::Error(_)
        ));
    }
}
