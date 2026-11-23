//! Protected local approval for an exact financial release. No wallet/provider API.
use anyhow::{Result, ensure};
use serde::Deserialize;
use std::{io::Read, path::Path, time::Duration};
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Approval {
    pub version: u8,
    pub epoch: Uuid,
    pub plan_id: Uuid,
    pub threshold_sats: u64,
    pub reviewed_financial_release: bool,
    pub approved_at: i64,
    pub expires_at: i64,
}
impl Approval {
    pub fn load(path: &Path) -> Result<Self> {
        crate::private_ipc::protected_config(path)?;
        let mut bytes = Vec::new();
        std::fs::File::open(path)?
            .take(4097)
            .read_to_end(&mut bytes)?;
        ensure!(
            !bytes.is_empty() && bytes.len() <= 4096,
            "release approval exceeds bound"
        );
        let decision: Self = serde_json::from_slice(&bytes)
            .map_err(|_| anyhow::anyhow!("invalid financial release approval"))?;
        decision.require_current()?;
        Ok(decision)
    }
    pub fn require_current(&self) -> Result<()> {
        let now = crate::gateway::now_epoch()?;
        ensure!(
            self.version == 1
                && !self.epoch.is_nil()
                && !self.plan_id.is_nil()
                && self.threshold_sats > 0
                && self.threshold_sats <= i64::MAX as u64
                && self.reviewed_financial_release
                && self.approved_at >= 0
                && self.expires_at > self.approved_at
                && self
                    .expires_at
                    .checked_sub(self.approved_at)
                    .is_some_and(|window| window <= 86400)
                && now >= self.approved_at
                && now <= self.expires_at,
            "financial release approval is not current or valid"
        );
        Ok(())
    }
}

pub async fn require_stopped_core() -> Result<()> {
    let result = tokio::time::timeout(
        Duration::from_secs(5),
        tokio::process::Command::new("/usr/bin/systemctl")
            .env_clear()
            .env("LC_ALL", "C")
            .args([
                "show",
                "--no-pager",
                "--property=Id,LoadState,ActiveState,SubState,MainPID,ControlGroup",
                "--",
                "lightning-goats-receive.service",
            ])
            .kill_on_drop(true)
            .output(),
    )
    .await??;
    ensure!(
        result.status.success(),
        "cannot inspect stopped core service"
    );
    validate_stopped_core(std::str::from_utf8(&result.stdout)?)
}
fn validate_stopped_core(text: &str) -> Result<()> {
    let mut fields = std::collections::BTreeMap::new();
    for line in text.lines() {
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| anyhow::anyhow!("invalid core service state"))?;
        ensure!(
            fields.insert(key, value).is_none(),
            "duplicate service property"
        );
    }
    ensure!(
        fields.len() == 6
            && fields.get("Id") == Some(&"lightning-goats-receive.service")
            && fields
                .get("LoadState")
                .is_some_and(|state| ["loaded", "masked"].contains(state))
            && fields.get("ActiveState") == Some(&"inactive")
            && fields.get("SubState") == Some(&"dead")
            && fields.get("MainPID") == Some(&"0")
            && fields.get("ControlGroup") == Some(&""),
        "core must be fully stopped before financial release"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample() -> serde_json::Value {
        let now = crate::gateway::now_epoch().unwrap();
        serde_json::json!({"version":1,"epoch":Uuid::new_v4(),"plan_id":Uuid::new_v4(),
            "threshold_sats":1000,"reviewed_financial_release":true,"approved_at":now,"expires_at":now+3600})
    }
    #[test]
    fn approval_scope_time_and_review_are_required() {
        let value = sample();
        assert!(
            serde_json::from_value::<Approval>(value.clone())
                .unwrap()
                .require_current()
                .is_ok()
        );
        for (key, replacement) in [
            ("version", serde_json::json!(2)),
            ("epoch", serde_json::json!(Uuid::nil())),
            ("plan_id", serde_json::json!(Uuid::nil())),
            ("threshold_sats", serde_json::json!(0)),
            ("reviewed_financial_release", serde_json::json!(false)),
            (
                "approved_at",
                serde_json::json!(crate::gateway::now_epoch().unwrap() + 60),
            ),
            (
                "expires_at",
                serde_json::json!(crate::gateway::now_epoch().unwrap() - 60),
            ),
            (
                "expires_at",
                serde_json::json!(crate::gateway::now_epoch().unwrap() + 86401),
            ),
        ] {
            let mut changed = value.clone();
            changed[key] = replacement;
            assert!(
                serde_json::from_value::<Approval>(changed)
                    .unwrap()
                    .require_current()
                    .is_err()
            );
        }
        let mut changed = value;
        changed["api_key"] = serde_json::json!("forbidden");
        assert!(serde_json::from_value::<Approval>(changed).is_err());
    }
    #[tokio::test]
    async fn developer_owned_or_aliased_or_writable_approval_is_not_authority() -> Result<()> {
        use std::os::unix::fs::PermissionsExt;
        if crate::private_ipc::uid()? != 0 {
            let directory = tempfile::tempdir()?;
            let path = directory.path().join("approval.json");
            std::fs::write(&path, serde_json::to_vec(&sample())?)?;
            assert!(Approval::load(&path).is_err());
            return Ok(());
        }
        let directory = tempfile::Builder::new()
            .prefix("lg-financial-release-")
            .tempdir_in("/run")?;
        let path = directory.path().join("approval.json");
        std::fs::write(&path, serde_json::to_vec(&sample())?)?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640))?;
        Approval::load(&path)?;
        let alias = directory.path().join("alias.json");
        std::os::unix::fs::symlink(&path, &alias)?;
        assert!(Approval::load(&alias).is_err());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o660))?;
        assert!(Approval::load(&path).is_err());
        println!("FINANCIAL_RELEASE_PROTECTED_APPROVAL_VERIFIED");
        Ok(())
    }
    #[test]
    fn incomplete_active_or_unknown_service_state_cannot_authorize_release() {
        let valid = "Id=lightning-goats-receive.service\nLoadState=loaded\nActiveState=inactive\nSubState=dead\nMainPID=0\nControlGroup=\n";
        assert!(validate_stopped_core(valid).is_ok());
        for (from, to) in [
            ("MainPID=0", "MainPID=10"),
            ("ActiveState=inactive", "ActiveState=active"),
            ("ControlGroup=\n", "ControlGroup=/system.slice/core\n"),
            ("LoadState=loaded", "LoadState=not-found"),
            ("Id=lightning-goats-receive.service", "Id=another.service"),
        ] {
            assert!(validate_stopped_core(&valid.replace(from, to)).is_err());
        }
        assert!(validate_stopped_core(&(valid.to_owned() + "MainPID=0\n")).is_err());
        assert!(validate_stopped_core("").is_err());
    }
}
