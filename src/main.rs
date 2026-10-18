use std::process::ExitCode;

use clap::Parser;
use splice::cli::{Cli, Command};
use splice::error::SpliceError;
use splice::output::Report;

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().collect();
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(e) => {
            use clap::error::ErrorKind;
            let wants_json = argv.iter().skip(1).any(|a| a == "--json");
            if wants_json && !matches!(e.kind(), ErrorKind::DisplayHelp | ErrorKind::DisplayVersion)
            {
                let mut words = argv
                    .iter()
                    .skip(1)
                    .map(String::as_str)
                    .filter(|a| !a.starts_with('-'))
                    .skip_while(|a| !Command::NAMES.contains(a));
                let command = match (words.next(), words.next()) {
                    (Some("channel"), Some(sub @ ("add" | "remove"))) => {
                        format!("channel {sub}")
                    }
                    (Some(name), _) => name.to_string(),
                    _ => String::new(),
                };
                let mut report = Report::new(&command);
                report.ok = false;
                report.msg(e.render().to_string().trim_end());
                report.print(true);
                return ExitCode::from(2);
            }
            e.exit();
        }
    };

    let json = cli.global.json;
    let command = cli.command.name();
    match splice::run(cli) {
        Ok(report) => {
            report.print(json);
            ExitCode::SUCCESS
        }
        Err(e) => {
            let code = e.exit_code();
            let report = match e {
                SpliceError::Conflict(mut r)
                | SpliceError::Drift(mut r)
                | SpliceError::Applied(mut r) => {
                    r.ok = false;
                    r
                }
                other => {
                    let mut r = Report::new(command);
                    r.ok = false;
                    r.msg(other.to_string());
                    r
                }
            };
            report.print(json);
            ExitCode::from(code as u8)
        }
    }
}
