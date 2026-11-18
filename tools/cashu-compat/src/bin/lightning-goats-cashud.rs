//! Separate wallet process. No implicit initialization, key generation or public listener.
#![forbid(unsafe_code)]
use lightning_goats_cashu_compat::{
    descriptor::Encoding, service::Runtime, service_config::Config, writer_lock::RecoveryRequired,
};
use std::{ffi::OsString, path::PathBuf, process::ExitCode};

const USAGE: &str = "Usage: lightning-goats-cashud <check-config|initialize|run|status> <config.json>\n\
       lightning-goats-cashud reconcile-acquisition <config.json> <approval.json>\n\
       lightning-goats-cashud reconcile-treasury <config.json> <approval.json>\n\
       lightning-goats-cashud review-mint-policy <current.json> <candidate.json>\n\
       lightning-goats-cashud prepare-announcement <config.json> <operation-uuid>\n\
       lightning-goats-cashud prepare-request <config.json> <https-endpoint> <creqA|creqB>\n\
check-config  Validate protected configuration without opening wallet state or keys.\n\
initialize    Explicitly initialize empty wallet databases using provisioned keys.\n\
run           Open existing wallet state; serve private intake and configured acquisition.\n\
status        Read bounded private journal status without loading keys or changing work.\n\
reconcile-acquisition  Check one approved original acquisition; no swap/melt replay.\n\
reconcile-treasury  Check one original submitted melt and retain its wallet report.\n\
review-mint-policy  Review a protected mint-only change against the stopped wallet.\n\
prepare-announcement  Offline: retain and preview configured kind 10019; no workers started.\n\
prepare-request  Offline: preview a reusable request; no keys, state or network opened.\n\
Receiving, relay intake, acquisition, receipt delivery and treasury execution default off.\n\
Do not activate for real funds before separate operator acceptance.";

#[derive(Debug, PartialEq, Eq)]
enum Command {
    Help,
    Check(PathBuf),
    Initialize(PathBuf),
    Run(PathBuf),
    Status(PathBuf),
    ReconcileAcquisition(PathBuf, PathBuf),
    ReconcileTreasury(PathBuf, PathBuf),
    ReviewMintPolicy(PathBuf, PathBuf),
    PrepareAnnouncement(PathBuf, uuid::Uuid),
    PrepareRequest(PathBuf, String, Encoding),
}

fn parse(args: Vec<OsString>) -> Result<Command, ()> {
    if args.len() == 1 && (args[0] == "--help" || args[0] == "-h") {
        return Ok(Command::Help);
    }
    if args.len() == 3
        && args[0] == "review-mint-policy"
        && !args[1].is_empty()
        && !args[2].is_empty()
    {
        return Ok(Command::ReviewMintPolicy(
            PathBuf::from(&args[1]),
            PathBuf::from(&args[2]),
        ));
    }
    if args.len() == 3
        && args[0] == "reconcile-treasury"
        && !args[1].is_empty()
        && !args[2].is_empty()
    {
        return Ok(Command::ReconcileTreasury(
            PathBuf::from(&args[1]),
            PathBuf::from(&args[2]),
        ));
    }
    if args.len() == 3
        && args[0] == "reconcile-acquisition"
        && !args[1].is_empty()
        && !args[2].is_empty()
    {
        return Ok(Command::ReconcileAcquisition(
            PathBuf::from(&args[1]),
            PathBuf::from(&args[2]),
        ));
    }
    if args.len() == 3 && args[0] == "prepare-announcement" && !args[1].is_empty() {
        let id = uuid::Uuid::parse_str(args[2].to_str().ok_or(())?).map_err(|_| ())?;
        if id.is_nil() {
            return Err(());
        }
        return Ok(Command::PrepareAnnouncement(PathBuf::from(&args[1]), id));
    }
    if args.len() == 4 && args[0] == "prepare-request" && !args[1].is_empty() {
        let endpoint = args[2].to_str().ok_or(())?;
        if endpoint.is_empty() || endpoint.len() > 256 {
            return Err(());
        }
        let encoding = match args[3].to_str() {
            Some("creqA") => Encoding::CreqA,
            Some("creqB") => Encoding::CreqB,
            _ => return Err(()),
        };
        return Ok(Command::PrepareRequest(
            PathBuf::from(&args[1]),
            endpoint.into(),
            encoding,
        ));
    }
    if args.len() != 2 || args[1].is_empty() {
        return Err(());
    }
    let path = PathBuf::from(&args[1]);
    match args[0].to_str() {
        Some("check-config") => Ok(Command::Check(path)),
        Some("initialize") => Ok(Command::Initialize(path)),
        Some("run") => Ok(Command::Run(path)),
        Some("status") => Ok(Command::Status(path)),
        _ => Err(()),
    }
}

async fn execute(command: Command) -> Result<(), &'static str> {
    match command {
        Command::Help => println!("{USAGE}"),
        Command::Check(path) => {
            Config::load(&path).map_err(|_| "wallet configuration rejected")?;
            println!("Wallet configuration valid; state and keys not opened.");
        }
        Command::PrepareRequest(path, endpoint, encoding) => {
            let config = Config::load(&path).map_err(|_| "wallet configuration rejected")?;
            let request = config
                .prepare_request(&endpoint, encoding)
                .map_err(|_| "payment request preparation rejected")?;
            println!("{}", serde_json::to_string(&serde_json::json!({
                "version": 1, "id": request.id(), "request": request.encoded(),
                "encoding": match encoding { Encoding::CreqA => "creqA", Encoding::CreqB => "creqB" },
                "status": "prepared", "service_started": false,
                "descriptor_registered": config.descriptors.iter().any(|id| id == request.id())
            })).map_err(|_| "payment request preview encoding failed")?);
        }
        Command::Status(path) => {
            let config = Config::load(&path).map_err(|_| "wallet configuration rejected")?;
            let status = lightning_goats_cashu_compat::status::Status::read(&config)
                .map_err(|_| "wallet status unavailable; state or registration check failed")?;
            println!(
                "{}",
                serde_json::to_string(&status).map_err(|_| "wallet status encoding failed")?
            );
        }
        Command::ReconcileAcquisition(path, approval) => {
            let decision =
                lightning_goats_cashu_compat::operator_reconciliation::Approval::load(&approval)
                    .map_err(|_| "ownership approval rejected")?;
            let config = decision
                .load_config(&path)
                .map_err(|_| "ownership configuration rejected")?;
            let runtime = Runtime::open(config).await.map_err(
                |_| "ownership check requires offline registered state and matching keys",
            )?;
            let outcome = decision
                .reconcile(&runtime)
                .await
                .map_err(|_| "ownership check rejected; original operation retained")?;
            println!(
                "{}",
                serde_json::to_string(&outcome).map_err(|_| "ownership outcome encoding failed")?
            );
        }
        Command::Initialize(path) => {
            let config = Config::load(&path).map_err(|_| "wallet configuration rejected")?;
            let runtime = Runtime::initialize(config).await.map_err(
                |_| "wallet initialization failed; retained state requires inspection before retry",
            )?;
            drop(runtime);
            println!("Wallet databases initialized; no service started.");
        }
        Command::ReviewMintPolicy(current, candidate) => {
            let review = lightning_goats_cashu_compat::operator_mint_policy::review(&current, &candidate)
                .map_err(|_| "mint policy review rejected; check configuration scope and retained wallet state")?;
            println!(
                "{}",
                serde_json::to_string(&review).map_err(|_| "mint policy review encoding failed")?
            );
        }
        Command::ReconcileTreasury(path, approval) => {
            use lightning_goats_cashu_compat::operator_treasury::Approval;
            let approval = Approval::load(&approval)
                .map_err(|_| "treasury reconciliation approval rejected")?;
            let config = approval
                .load_config(&path)
                .map_err(|_| "treasury reconciliation configuration rejected")?;
            let runtime = Runtime::open(config).await.map_err(|_| "treasury reconciliation requires offline registered wallet state and matching keys")?;
            let result = approval
                .reconcile(&runtime)
                .await
                .map_err(|_| "treasury reconciliation failed; original operation retained")?;
            println!(
                "{}",
                serde_json::to_string(&result)
                    .map_err(|_| "treasury reconciliation outcome encoding failed")?
            );
        }
        Command::PrepareAnnouncement(path, id) => {
            let config = Config::load(&path).map_err(|_| "wallet configuration rejected")?;
            let runtime=Runtime::open(config).await.map_err(|_|"announcement preparation requires offline registered wallet state and matching keys")?;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|_| "announcement clock unavailable")?
                .as_secs();
            let intent=runtime.prepare_announcement(id,now).map_err(|_|"announcement preparation rejected; inspect receiving policy and retained operation")?;
            println!("{}",serde_json::to_string(&serde_json::json!({"operation_id":id,"intent":intent,"status":"retained","service_started":false})).map_err(|_|"announcement preview encoding failed")?);
        }
        Command::Run(path) => {
            let config = Config::load(&path).map_err(|_| "wallet configuration rejected")?;
            let mut terminate =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                    .map_err(|_| "wallet termination handler unavailable")?;
            let mut interrupt =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())
                    .map_err(|_| "wallet interrupt handler unavailable")?;
            let runtime = Runtime::open(config).await.map_err(|error| {
                if error.is::<RecoveryRequired>() {
                    RecoveryRequired::MESSAGE
                } else {
                    "wallet startup rejected; configuration, keys and retained state require inspection"
                }
            })?;
            runtime.serve(async move {
                tokio::select! { _ = terminate.recv() => {}, _ = interrupt.recv() => {} }
            }).await.map_err(|_| "wallet service stopped after an internal error; retained state requires inspection")?;
        }
    }
    Ok(())
}

#[tokio::main]
async fn main() -> ExitCode {
    let command = match parse(std::env::args_os().skip(1).collect()) {
        Ok(command) => command,
        Err(()) => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    match execute(command).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }
    #[test]
    fn commands_require_explicit_mode_and_exact_arguments() {
        assert_eq!(parse(args(&["--help"])), Ok(Command::Help));
        assert_eq!(
            parse(args(&[
                "reconcile-treasury",
                "/fixture/config.json",
                "/fixture/approval.json"
            ])),
            Ok(Command::ReconcileTreasury(
                "/fixture/config.json".into(),
                "/fixture/approval.json".into()
            ))
        );
        assert!(parse(args(&["reconcile-treasury", "/fixture/config.json"])).is_err());
        for (name, encoding) in [("creqA", Encoding::CreqA), ("creqB", Encoding::CreqB)] {
            assert_eq!(
                parse(args(&[
                    "prepare-request",
                    "/fixture/config.json",
                    "https://example.com/c",
                    name
                ])),
                Ok(Command::PrepareRequest(
                    "/fixture/config.json".into(),
                    "https://example.com/c".into(),
                    encoding
                ))
            );
        }
        assert_eq!(
            parse(args(&[
                "reconcile-acquisition",
                "/fixture/config.json",
                "/fixture/approval.json"
            ])),
            Ok(Command::ReconcileAcquisition(
                "/fixture/config.json".into(),
                "/fixture/approval.json".into()
            ))
        );
        let id = uuid::Uuid::from_u128(1);
        assert_eq!(
            parse(args(&[
                "prepare-announcement",
                "/fixture/config.json",
                &id.to_string()
            ])),
            Ok(Command::PrepareAnnouncement(
                "/fixture/config.json".into(),
                id
            ))
        );
        assert_eq!(
            parse(args(&["run", "/fixture/config.json"])),
            Ok(Command::Run("/fixture/config.json".into()))
        );
        assert_eq!(
            parse(args(&["check-config", "/fixture/config.json"])),
            Ok(Command::Check("/fixture/config.json".into()))
        );
        assert_eq!(
            parse(args(&["initialize", "/fixture/config.json"])),
            Ok(Command::Initialize("/fixture/config.json".into()))
        );
        for values in [
            vec![],
            vec!["run"],
            vec!["reconcile-acquisition", "file"],
            vec!["reconcile-acquisition", "file", ""],
            vec!["reconcile-acquisition", "file", "approval", "--clear-hold"],
            vec!["initialize", ""],
            vec!["--help", "extra"],
            vec!["run", "file", "extra"],
            vec!["pay", "invoice"],
            vec!["prepare-request", "file", "https://example.com/c"],
            vec![
                "prepare-request",
                "file",
                "https://example.com/c",
                "automatic",
            ],
            vec!["prepare-request", "file", "", "creqA"],
            vec![
                "prepare-request",
                "file",
                "https://example.com/c",
                "creqA",
                "publish",
            ],
            vec!["prepare-announcement", "file"],
            vec!["prepare-announcement", "file", "invalid"],
            vec![
                "prepare-announcement",
                "file",
                "00000000-0000-0000-0000-000000000000",
            ],
        ] {
            assert!(parse(args(&values)).is_err());
        }
    }
    #[tokio::test]
    async fn invalid_configuration_errors_never_echo_paths_or_credentials() {
        let private_path = PathBuf::from("/missing/private-token-fixture");
        assert_eq!(
            execute(Command::PrepareRequest(
                private_path.clone(),
                "https://secret-user:secret-pass@example.com/c".into(),
                Encoding::CreqA
            ))
            .await,
            Err("wallet configuration rejected")
        );
        assert_eq!(
            execute(Command::PrepareAnnouncement(
                private_path.clone(),
                uuid::Uuid::from_u128(1)
            ))
            .await,
            Err("wallet configuration rejected")
        );
        assert_eq!(
            execute(Command::Check(private_path.clone())).await,
            Err("wallet configuration rejected")
        );
        assert_eq!(
            execute(Command::Initialize(private_path.clone())).await,
            Err("wallet configuration rejected")
        );
        assert_eq!(
            execute(Command::Run(private_path)).await,
            Err("wallet configuration rejected")
        );
    }
}
