//! Separate Cashu NIP-46 client process; no wallet keys or public listener.
#![forbid(unsafe_code)]
use lightning_goats_cashu_compat::signing_service::Config;
use std::{ffi::OsString, path::Path, process::ExitCode};
const USAGE: &str = "Usage: lightning-goats-cashu-signerd <check-config|initialize|run> <config.json>\n\
check-config  Validate protected configuration without state, credentials or network.\n\
initialize    Explicitly initialize an empty broker journal; no signing or key generation.\n\
run           Open existing state and serve the registered wallet on a private Unix socket.\n\
Signing permissions default off. Run loads only the systemd cashu-nostr-client\n\
credential, a dedicated NIP-46 client key, never the project Nostr private key.";
async fn execute(args: &[OsString]) -> Result<(), (u8, &'static str)> {
    if args.len() == 1 && (args[0] == "--help" || args[0] == "-h") {
        println!("{USAGE}");
        return Ok(());
    }
    if args.len() != 2
        || args[1].is_empty()
        || !matches!(
            args[0].to_str(),
            Some("check-config" | "initialize" | "run")
        )
    {
        return Err((2, USAGE));
    }
    let config =
        Config::load(Path::new(&args[1])).map_err(|_| (1, "signer configuration rejected"))?;
    match args[0].to_str() {
        Some("check-config") => {
            println!("Signer configuration valid; state and credentials not opened.")
        }
        Some("initialize") => {
            config.initialize().map_err(|_| {
                (
                    1,
                    "signer initialization failed; inspect retained state before retry",
                )
            })?;
            println!("Signer journal initialized; no service started.");
        }
        Some("run") => {
            let mut terminate =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                    .map_err(|_| (1, "signer termination handler unavailable"))?;
            let mut interrupt =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())
                    .map_err(|_| (1, "signer interrupt handler unavailable"))?;
            let runtime = config.open().map_err(|_| (1,"signer startup rejected; inspect configuration, credentials and retained state"))?;
            runtime
                .serve(async move {
                    tokio::select! { _=terminate.recv()=>{}, _=interrupt.recv()=>{} }
                })
                .await
                .map_err(|_| {
                    (
                        1,
                        "signer service stopped after an internal error; inspect retained state",
                    )
                })?;
        }
        _ => unreachable!(),
    }
    Ok(())
}
#[tokio::main]
async fn main() -> ExitCode {
    match execute(&std::env::args_os().skip(1).collect::<Vec<_>>()).await {
        Ok(()) => ExitCode::SUCCESS,
        Err((code, message)) => {
            eprintln!("{message}");
            ExitCode::from(code)
        }
    }
}
