//! Root-only normalization of protected database copies for the backup collector.
#![forbid(unsafe_code)]
use std::{path::PathBuf, process::ExitCode};
const USAGE: &str = "Usage: lightning-goats-cashu-statectl normalize-copy <private-stage> <raw-name.db> <normalized-name.db> <max-bytes>\n\
       lightning-goats-cashu-statectl verify-recovery-keys <private-stage>\n\
       lightning-goats-cashu-statectl observe-restored-mint <selection> <selection-sha256> <mint-url> <output>\n\
       lightning-goats-cashu-statectl observe-restored-custody <selection> <selection-sha256> <mint-url> <output>\n\
Outputs private recovery evidence for separate operator review.";
fn main() -> ExitCode {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    if args.len() == 1 && (args[0] == "--help" || args[0] == "-h") {
        println!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    let key_mode = args.len() == 2 && args[0] == "verify-recovery-keys";
    let custody = args.len() == 5 && args[0] == "observe-restored-custody";
    if custody || args.len() == 5 && args[0] == "observe-restored-mint" {
        let result = (|| {
            let expected = args[2].to_str().ok_or(())?;
            let mint = args[3].to_str().ok_or(())?;
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|_| ())?;
            let selection = PathBuf::from(&args[1]);
            let output = PathBuf::from(&args[4]);
            use lightning_goats_cashu_compat::restore_mint;
            runtime
                .block_on(async {
                    if custody {
                        restore_mint::observe_custody(&selection, expected, mint, &output).await
                    } else {
                        restore_mint::observe(&selection, expected, mint, &output).await
                    }
                })
                .map_err(|_| ())
        })();
        return match result {
            Ok(()) => {
                let observed = if custody {
                    "CASHU_RESTORE_MINT_CUSTODY_OBSERVED"
                } else {
                    "CASHU_RESTORE_MINT_MELT_OBSERVED"
                };
                println!("{{\"result\":\"{observed}\",\"resume_authorized\":false}}");
                ExitCode::SUCCESS
            }
            Err(()) => {
                if custody {
                    eprintln!("Restored Cashu custody observation refused.");
                } else {
                    eprintln!("Restored Cashu mint observation refused.");
                }
                ExitCode::from(1)
            }
        };
    }
    if !key_mode && (args.len() != 5 || args[0] != "normalize-copy") {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    }
    let result = (|| {
        if key_mode {
            let receipt =
                lightning_goats_cashu_compat::backup_keys::verify(&PathBuf::from(&args[1]))
                    .map_err(|_| ())?;
            return serde_json::to_string(&receipt).map_err(|_| ());
        }
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
            if key_mode {
                eprintln!(
                    "Recovery key verification refused; preserve the incomplete private stage."
                );
                return ExitCode::from(1);
            }
            eprintln!(
                "Database copy normalization refused; preserve the incomplete private stage."
            );
            ExitCode::from(1)
        }
    }
}
