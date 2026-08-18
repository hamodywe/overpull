use std::process::ExitCode;

use overpull::cli::{self, ParseOutcome, VERSION};
use overpull::run;

fn main() -> ExitCode {
    match cli::parse(std::env::args().skip(1)) {
        ParseOutcome::Help => {
            print!("{}", cli::help_text());
            ExitCode::SUCCESS
        }
        ParseOutcome::Version => {
            println!("overpull {VERSION}");
            ExitCode::SUCCESS
        }
        ParseOutcome::Error(message) => {
            eprintln!("overpull: {message}");
            ExitCode::from(2)
        }
        ParseOutcome::Run(options) => match run::run(&options) {
            Ok(outcome) => {
                print!("{}", outcome.output);
                if outcome.should_fail {
                    ExitCode::from(1)
                } else {
                    ExitCode::SUCCESS
                }
            }
            Err(message) => {
                eprintln!("overpull: {message}");
                ExitCode::from(2)
            }
        },
    }
}
