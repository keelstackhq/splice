use clap::Parser;
use splice::cli::{Cli, SUBCOMMANDS};
use splice::error::SpliceError;
use splice::output::Report;

fn main() {
    let cli = match Cli::try_parse() {
        Ok(c) => c,
        Err(e) => {
            let args: Vec<String> = std::env::args().skip(1).collect();
            if args.iter().any(|a| a == "--json") {
                let command = args
                    .iter()
                    .find(|a| !a.starts_with('-') && SUBCOMMANDS.contains(&a.as_str()))
                    .cloned()
                    .unwrap_or_default();
                let mut r = Report::new(&command);
                r.ok = false;
                r.msg(e.render().to_string().trim().to_string());
                r.print(true);
                std::process::exit(2);
            }
            e.exit();
        }
    };
    let json = cli.global.json;
    let command = format!("{:?}", cli.command);
    let command = command
        .split(['(', ' '])
        .next()
        .unwrap_or("")
        .to_lowercase();
    match splice::run(cli) {
        Ok(report) => report.print(json),
        Err(e) => {
            let code = e.exit_code();
            let report = match e {
                SpliceError::Conflict(mut r) | SpliceError::Drift(mut r) => {
                    r.ok = false;
                    r
                }
                other => {
                    let mut r = Report::new(&command);
                    r.ok = false;
                    r.msg(other.to_string());
                    r
                }
            };
            if json {
                report.print(true);
            } else {
                report.print(false);
                eprintln!("error: {}", report.messages.join("; "));
            }
            std::process::exit(code);
        }
    }
}
