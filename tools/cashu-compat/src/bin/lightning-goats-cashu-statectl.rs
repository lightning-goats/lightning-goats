//! Root-only normalization of protected database copies for the backup collector.
#![forbid(unsafe_code)]
use std::{path::PathBuf, process::ExitCode};
const USAGE: &str = "Usage: lightning-goats-cashu-statectl normalize-copy <private-stage> <raw-name.db> <normalized-name.db> <max-bytes>\n\
Copies only; no application startup, key loading, writer quiescence, coordinated verification or restore authority.";
fn main() -> ExitCode {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    if args.len() == 1 && (args[0] == "--help" || args[0] == "-h") {
        println!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    if args.len() != 5 || args[0] != "normalize-copy" {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    }
    let result = (|| {
        let input = args[2].to_str().ok_or(())?;
        let output = args[3].to_str().ok_or(())?;
        let maximum = args[4].to_str().ok_or(())?.parse::<u64>().map_err(|_| ())?;
        let receipt = lightning_goats_cashu_compat::backup_copy::normalize(
            &PathBuf::from(&args[1]),
            input,
            output,
            maximum,
        )
        .map_err(|_| ())?;
        serde_json::to_string(&receipt).map_err(|_| ())
    })();
    match result {
        Ok(receipt) => {
            println!("{receipt}");
            ExitCode::SUCCESS
        }
        Err(()) => {
            eprintln!(
                "Database copy normalization refused; preserve the incomplete private stage."
            );
            ExitCode::from(1)
        }
    }
}
