use clap::Parser;
use splice::cli::{Cli, SUBCOMMANDS};
use splice::error::SpliceError;
use splice::output::Report;

fn main() {
    let cli = match Cli::try_parse() {
        Ok(c) => c,
        Err(e) => {
            use clap::error::ErrorKind;
            if matches!(e.kind(), ErrorKind::DisplayHelp | ErrorKind::DisplayVersion) {
                e.exit();
            }
            let args: Vec<String> = std::env::args().skip(1).collect();
            let mut json = false;
            let mut command = String::new();
            let mut it = args.iter();
            while let Some(a) = it.next() {
                match a.as_str() {
                    "--" => break,
                    "-C" => {
                        it.next();
                    }
                    "--json" | "--json=true" => json = true,
                    x if command.is_empty() && SUBCOMMANDS.contains(&x) => {
                        command = x.to_string();
                    }
                    _ => {}
                }
            }
            if json {
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
    let command = cli.command.name();
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
                    let mut r = Report::new(command);
                    r.ok = false;
                    r.msg(other.to_string());
                    r
                }
            };
            report.print(json);
            std::process::exit(code);
        }
    }
}
