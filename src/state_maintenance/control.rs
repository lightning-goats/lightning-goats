use super::Policy;
use anyhow::{Result, bail};
use std::collections::BTreeMap;
use std::time::Duration;

pub(super) enum Control {
    Systemd,
    #[cfg(test)]
    Fixture(std::sync::Arc<std::sync::atomic::AtomicBool>),
}
impl Control {
    pub(super) async fn check(&self, policy: &Policy) -> Result<()> {
        #[cfg(test)]
        if let Self::Fixture(ready) = self {
            if !ready.load(std::sync::atomic::Ordering::SeqCst) {
                bail!("fixture writer became active");
            }
            return Ok(());
        }
        for component in policy.components.values() {
            let result=tokio::time::timeout(Duration::from_secs(5),tokio::process::Command::new("/usr/bin/systemctl")
                .args(["show","--no-pager","--property=Id,LoadState,ActiveState,SubState,MainPID,UnitFileState,ControlGroup","--",&component.unit])
                .kill_on_drop(true).output()).await??;
            if !result.status.success() || result.stdout.len() > 16384 {
                bail!("cannot establish stopped service state");
            }
            let raw = std::str::from_utf8(&result.stdout)?;
            let values = validate_status(raw, &component.unit)?;
            if let Some(group) = values.get("ControlGroup").filter(|g| !g.is_empty()) {
                if !group.starts_with('/') || group.split('/').any(|p| p == "." || p == "..") {
                    bail!("invalid service control group");
                }
                let events = std::path::Path::new("/sys/fs/cgroup")
                    .join(group.trim_start_matches('/'))
                    .join("cgroup.events");
                match std::fs::read_to_string(events) {
                    Ok(v) if v.lines().any(|l| l == "populated 0") => {}
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    _ => bail!("service still has running control-group processes"),
                }
            }
        }
        Ok(())
    }
}

fn validate_status<'a>(raw: &'a str, unit: &str) -> Result<BTreeMap<&'a str, &'a str>> {
    let values = raw
        .lines()
        .map(|l| {
            l.split_once('=')
                .ok_or_else(|| anyhow::anyhow!("invalid systemd state"))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    if values.len() != 7
        || values.get("Id") != Some(&unit)
        || !matches!(values.get("LoadState"), Some(&"loaded") | Some(&"masked"))
        || values.get("ActiveState") != Some(&"inactive")
        || values.get("SubState") != Some(&"dead")
        || values.get("MainPID") != Some(&"0")
        || !matches!(
            values.get("UnitFileState"),
            Some(&"masked") | Some(&"masked-runtime")
        )
    {
        bail!("every journal writer must be inactive and masked");
    }

    Ok(values)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn service_quiescence_requires_exact_masked_inactive_zero_pid_and_complete_identity() {
        let good = "Id=example.service\nLoadState=masked\nActiveState=inactive\nSubState=dead\nMainPID=0\nUnitFileState=masked\nControlGroup=\n";
        validate_status(good, "example.service").unwrap();
        for (before, after) in [
            ("Id=example.service", "Id=other.service"),
            ("MainPID=0", "MainPID=7"),
            ("UnitFileState=masked", "UnitFileState=disabled"),
            ("ActiveState=inactive", "ActiveState=active"),
            ("LoadState=masked", "LoadState=not-found"),
        ] {
            assert!(validate_status(&good.replace(before, after), "example.service").is_err());
        }
        assert!(validate_status(&good.replace("ControlGroup=\n", ""), "example.service").is_err());
    }
}
