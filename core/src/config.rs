//! Configuration loading and validation for Canopus services
//!
//! This module parses a TOML configuration into `schema::ServiceSpec` values,
//! applies sane defaults (via serde defaults on schema types), and performs
//! strict validation with field-path error messages.

use crate::{CoreError, Result};
use schema::*;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

/// Top-level TOML structure for services configuration
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServicesFile {
    /// List of services to supervise
    pub services: Vec<ServiceSpec>,
}

/// Simple per-service runtime configuration (hostname/port)
#[derive(Debug, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SimpleServiceConfig {
    /// Optional hostname alias (e.g. test.dev)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hostname: Option<String>,
    /// Optional fixed port to run on; if omitted a free port will be allocated
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
}

/// Top-level wrapper that flattens service IDs into a map
#[derive(Debug, Deserialize, Clone, PartialEq, Eq)]
pub struct SimpleServicesFile {
    /// Map of service_id -> runtime config
    #[serde(flatten)]
    pub services: HashMap<String, SimpleServiceConfig>,
}

impl SimpleServicesFile {
    /// Validate the simple configuration
    pub fn validate(&self) -> Result<()> {
        if self.services.is_empty() {
            return Err(CoreError::ValidationError(
                "config must contain at least one service section".to_string(),
            ));
        }
        for (id, cfg) in &self.services {
            if id.trim().is_empty() {
                return Err(CoreError::ValidationError(
                    "service id (table name) cannot be empty".to_string(),
                ));
            }
            if let Some(hn) = &cfg.hostname {
                if hn.trim().is_empty() {
                    return Err(CoreError::ValidationError(format!(
                        "service '{}': hostname cannot be empty",
                        id
                    )));
                }
            }
            if let Some(p) = cfg.port {
                if p == 0 {
                    return Err(CoreError::ValidationError(format!(
                        "service '{}': port must be 1..=65535",
                        id
                    )));
                }
            }
        }
        Ok(())
    }
}

/// Load simple services config from TOML file path
pub fn load_simple_services_from_toml_path(path: impl AsRef<Path>) -> Result<SimpleServicesFile> {
    let data = fs::read_to_string(&path).map_err(|e| {
        CoreError::ConfigurationError(format!("Failed to read config {:?}: {}", path.as_ref(), e))
    })?;
    load_simple_services_from_toml_str(&data)
}

/// Load simple services config from a TOML string
pub fn load_simple_services_from_toml_str(input: &str) -> Result<SimpleServicesFile> {
    let cfg: SimpleServicesFile = toml::from_str(input)
        .map_err(|e| CoreError::ConfigurationError(format!("TOML parse error: {}", e)))?;
    cfg.validate()?;
    Ok(cfg)
}

impl ServicesFile {
    /// Validate the configuration and return `Result<()>` with field-path errors
    pub fn validate(&self) -> Result<()> {
        if self.services.is_empty() {
            return Err(CoreError::ValidationError(
                "services: must contain at least one service".to_string(),
            ));
        }

        // Ensure unique IDs
        let mut seen = HashSet::new();
        for (i, svc) in self.services.iter().enumerate() {
            // id
            if svc.id.trim().is_empty() {
                return Err(CoreError::ValidationError(format!(
                    "services[{}].id: cannot be empty",
                    i
                )));
            }
            if !seen.insert(svc.id.clone()) {
                return Err(CoreError::ValidationError(format!(
                    "services[{}].id: duplicate id '{}'",
                    i, svc.id
                )));
            }
            // name
            if svc.name.trim().is_empty() {
                return Err(CoreError::ValidationError(format!(
                    "services[{}].name: cannot be empty",
                    i
                )));
            }
            // command
            if svc.command.trim().is_empty() {
                return Err(CoreError::ValidationError(format!(
                    "services[{}].command: cannot be empty",
                    i
                )));
            }

            // environment keys should be non-empty
            for (k, _v) in svc.environment.iter() {
                if k.trim().is_empty() {
                    return Err(CoreError::ValidationError(format!(
                        "services[{}].environment: keys cannot be empty",
                        i
                    )));
                }
            }

            // backoff config sanity
            let b = &svc.backoff_config;
            if b.base_delay_secs == 0 {
                return Err(CoreError::ValidationError(format!(
                    "services[{}].backoffConfig.baseDelaySecs: must be > 0",
                    i
                )));
            }
            if !(b.jitter >= 0.0 && b.jitter <= 1.0) {
                return Err(CoreError::ValidationError(format!(
                    "services[{}].backoffConfig.jitter: must be between 0.0 and 1.0",
                    i
                )));
            }
            if b.multiplier <= 0.0 {
                return Err(CoreError::ValidationError(format!(
                    "services[{}].backoffConfig.multiplier: must be > 0",
                    i
                )));
            }

            // timeouts
            if svc.graceful_timeout_secs == 0 {
                return Err(CoreError::ValidationError(format!(
                    "services[{}].gracefulTimeoutSecs: must be > 0",
                    i
                )));
            }
            if svc.startup_timeout_secs == 0 {
                return Err(CoreError::ValidationError(format!(
                    "services[{}].startupTimeoutSecs: must be > 0",
                    i
                )));
            }

            // health/readiness checks
            if let Some(h) = &svc.health_check {
                validate_probe(
                    i,
                    "healthCheck",
                    &h.check_type,
                    h.interval_secs,
                    h.timeout_secs,
                    Some((h.failure_threshold, h.success_threshold)),
                )?;
            }
            if let Some(r) = &svc.readiness_check {
                validate_probe(
                    i,
                    "readinessCheck",
                    &r.check_type,
                    r.interval_secs,
                    r.timeout_secs,
                    Some((1, r.success_threshold)),
                )?;
            }
        }
        Ok(())
    }
}

fn validate_probe(
    index: usize,
    field: &str,
    kind: &HealthCheckType,
    interval_secs: u64,
    timeout_secs: u64,
    thresholds: Option<(u32, u32)>,
) -> Result<()> {
    if interval_secs == 0 {
        return Err(CoreError::ValidationError(format!(
            "services[{}].{}.intervalSecs: must be > 0",
            index, field
        )));
    }
    if timeout_secs == 0 {
        return Err(CoreError::ValidationError(format!(
            "services[{}].{}.timeoutSecs: must be > 0",
            index, field
        )));
    }
    if let Some((fail, succ)) = thresholds {
        if fail == 0 {
            return Err(CoreError::ValidationError(format!(
                "services[{}].{}.failureThreshold: must be > 0",
                index, field
            )));
        }
        if succ == 0 {
            return Err(CoreError::ValidationError(format!(
                "services[{}].{}.successThreshold: must be > 0",
                index, field
            )));
        }
    }

    match kind {
        HealthCheckType::Tcp { port } => {
            if *port == 0 {
                return Err(CoreError::ValidationError(format!(
                    "services[{}].{}.type[Tcp].port: must be 1..=65535",
                    index, field
                )));
            }
        }
        HealthCheckType::Exec { command, .. } => {
            if command.trim().is_empty() {
                return Err(CoreError::ValidationError(format!(
                    "services[{}].{}.type[Exec].command: cannot be empty",
                    index, field
                )));
            }
        }
    }

    Ok(())
}

/// Load services from a TOML file path
pub fn load_services_from_toml_path(path: impl AsRef<Path>) -> Result<ServicesFile> {
    let data = fs::read_to_string(&path).map_err(|e| {
        CoreError::ConfigurationError(format!("Failed to read config {:?}: {}", path.as_ref(), e))
    })?;
    load_services_from_toml_str(&data)
}

/// Load services from a TOML string
pub fn load_services_from_toml_str(input: &str) -> Result<ServicesFile> {
    let cfg: ServicesFile = toml::from_str(input)
        .map_err(|e| CoreError::ConfigurationError(format!("TOML parse error: {}", e)))?;
    cfg.validate()?;
    Ok(cfg)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_config() -> String {
        r#"
        [[services]]
        id = "svc1"
        name = "Service One"
        command = "echo"
        args = ["hello"]

        [services.healthCheck.checkType]
        type = "tcp"
        port = 8080

        [[services]]
        id = "svc2"
        name = "Service Two"
        command = "sh"
        args = ["-c", "exit 0"]
        "#
        .to_string()
    }

    #[test]
    fn parses_and_validates_valid_config() {
        let cfg = load_services_from_toml_str(&valid_config()).expect("should parse");
        assert_eq!(cfg.services.len(), 2);
        assert_eq!(cfg.services[0].id, "svc1");
        assert_eq!(cfg.services[1].id, "svc2");
    }

    #[test]
    fn errors_on_empty_services() {
        let err = load_services_from_toml_str("services = []").unwrap_err();
        assert!(format!("{}", err).contains("services: must contain at least one service"));
    }

    #[test]
    fn errors_on_duplicate_ids() {
        let input = r#"
        [[services]]
        id = "dup"
        name = "A"
        command = "echo"
        [[services]]
        id = "dup"
        name = "B"
        command = "echo"
        "#;
        let err = load_services_from_toml_str(input).unwrap_err();
        assert!(format!("{}", err).contains("duplicate id"));
    }

    #[test]
    fn errors_on_tcp_bad_port() {
        let input = r#"
        [[services]]
        id = "s"
        name = "S"
        command = "echo"
        [services.healthCheck.checkType]
        type = "tcp"
        port = 0
        "#;
        let err = load_services_from_toml_str(input).unwrap_err();
        assert!(format!("{}", err).contains("type[Tcp].port"));
    }
}
