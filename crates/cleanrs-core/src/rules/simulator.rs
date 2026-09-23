use super::{command_available, command_output, home_path};
use crate::model::{Category, CleanMethod, CleanTarget, RiskLevel};
use crate::scanner::dir_size;
use crate::Cleaner;
use anyhow::{Context, Result};
use serde_json::Value;
use std::path::PathBuf;

pub struct SimulatorUnavailableCleaner;

impl Cleaner for SimulatorUnavailableCleaner {
    fn id(&self) -> &'static str {
        "simulator-unavailable"
    }

    fn display_name(&self) -> &'static str {
        "Unavailable simulator devices"
    }

    fn category(&self) -> Category {
        Category::Ide
    }

    fn risk_level(&self) -> RiskLevel {
        RiskLevel::Caution
    }

    fn is_available(&self) -> bool {
        command_available("xcrun") && devices_path().is_some_and(|path| path.is_dir())
    }

    fn scan(&self) -> Result<Vec<CleanTarget>> {
        let output = command_output(
            "xcrun",
            &["simctl", "list", "devices", "unavailable", "--json"],
        )?;
        let unavailable = parse_unavailable_devices(&output)?;
        let Some(root) = devices_path() else {
            return Ok(Vec::new());
        };

        let targets = unavailable
            .into_iter()
            .filter_map(|device| {
                let path = root.join(&device.udid);
                let size_bytes = dir_size(&path).ok()?;
                (size_bytes > 0).then_some(CleanTarget {
                    path,
                    size_bytes,
                    description: format!(
                        "Unavailable simulator: {} ({})",
                        device.name, device.udid
                    ),
                    method: CleanMethod::RunCommand(vec![
                        "xcrun".to_owned(),
                        "simctl".to_owned(),
                        "delete".to_owned(),
                        device.udid,
                    ]),
                })
            })
            .collect();
        Ok(targets)
    }
}

#[derive(Debug)]
struct UnavailableDevice {
    name: String,
    udid: String,
}

fn devices_path() -> Option<PathBuf> {
    home_path("Library/Developer/CoreSimulator/Devices")
}

fn parse_unavailable_devices(output: &str) -> Result<Vec<UnavailableDevice>> {
    let value: Value = serde_json::from_str(output).context("invalid simctl device JSON")?;
    let Some(devices) = value.get("devices").and_then(Value::as_object) else {
        return Ok(Vec::new());
    };

    Ok(devices
        .values()
        .filter_map(Value::as_array)
        .flat_map(|entries| entries.iter())
        .filter(|entry| entry.get("isAvailable") == Some(&Value::Bool(false)))
        .filter_map(|entry| {
            Some(UnavailableDevice {
                name: entry.get("name")?.as_str()?.to_owned(),
                udid: entry.get("udid")?.as_str()?.to_owned(),
            })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::parse_unavailable_devices;

    #[test]
    fn parses_only_unavailable_simulators() {
        let output = r#"
        {"devices": {
          "com.apple.CoreSimulator.SimRuntime.iOS-18-0": [
            {"name":"iPhone 15","udid":"ACTIVE","isAvailable":true},
            {"name":"Old Phone","udid":"OLD","isAvailable":false}
          ]
        }}
        "#;

        let devices = parse_unavailable_devices(output).expect("JSON should parse");
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].udid, "OLD");
    }
}
