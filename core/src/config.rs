//! Configuration loading and validation for Canopus services
//!
//! This module parses a TOML configuration into `schema::ServiceSpec` values,
//! applies sane defaults (via serde defaults on schema types), and performs
//! strict validation with field-path error messages.

use crate::{CoreError, Result};
use schema::{HealthCheckType, ServiceSpec};
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

/// Alternate services file format: top-level tables keyed by service id
///
/// This allows writing:
/// [web]
/// name = "Web"
/// command = "npm"
///
/// [api]
/// name = "API"
/// command = "python3"
#[derive(Debug, Deserialize)]
struct ServicesMapFile {
    /// Map of `service_id` -> table of `ServiceSpec` fields (without id)
    #[serde(flatten)]
    services: HashMap<String, toml::Value>,
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
    /// Map of `service_id` -> runtime config
    #[serde(flatten)]
    pub services: HashMap<String, SimpleServiceConfig>,
}

impl SimpleServicesFile {
    /// Validate the simple configuration
    ///
    /// # Errors
    ///
    /// Returns an error if any service configuration value is invalid.
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
                        "service '{id}': hostname cannot be empty"
                    )));
                }
            }
            if let Some(p) = cfg.port {
                if p == 0 {
                    return Err(CoreError::ValidationError(format!(
                        "service '{id}': port must be 1..=65535"
                    )));
                }
            }
        }
        Ok(())
    }
}

/// Load simple services config from TOML file path
///
/// # Errors
///
/// Returns an error if the file cannot be read or parsed.
pub fn load_simple_services_from_toml_path(path: impl AsRef<Path>) -> Result<SimpleServicesFile> {
    let data = fs::read_to_string(&path).map_err(|e| {
        CoreError::ConfigurationError(format!(
            "Failed to read config {}: {e}",
            path.as_ref().display()
        ))
    })?;
    load_simple_services_from_toml_str(&data)
}

/// Load simple services config from a TOML string
///
/// # Errors
///
/// Returns an error if the input cannot be parsed or validated.
pub fn load_simple_services_from_toml_str(input: &str) -> Result<SimpleServicesFile> {
    let cfg: SimpleServicesFile = toml::from_str(input)
        .map_err(|e| CoreError::ConfigurationError(format!("TOML parse error: {e}")))?;
    cfg.validate()?;
    Ok(cfg)
}

impl ServicesFile {
    /// Validate the configuration and return `Result<()>` with field-path errors
    ///
    /// # Errors
    ///
    /// Returns an error if any service configuration value is invalid.
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
                    "services[{i}].id: cannot be empty"
                )));
            }
            if !seen.insert(svc.id.clone()) {
                return Err(CoreError::ValidationError(format!(
                    "services[{i}].id: duplicate id '{}'",
                    svc.id
                )));
            }
            // name
            if svc.name.trim().is_empty() {
                return Err(CoreError::ValidationError(format!(
                    "services[{i}].name: cannot be empty"
                )));
            }
            // command
            if svc.command.trim().is_empty() {
                return Err(CoreError::ValidationError(format!(
                    "services[{i}].command: cannot be empty"
                )));
            }

            // environment keys should be non-empty
            for k in svc.environment.keys() {
                if k.trim().is_empty() {
                    return Err(CoreError::ValidationError(format!(
                        "services[{i}].environment: keys cannot be empty"
                    )));
                }
            }

            // backoff config sanity
            let b = &svc.backoff_config;
            if b.base_delay_secs == 0 {
                return Err(CoreError::ValidationError(format!(
                    "services[{i}].backoffConfig.baseDelaySecs: must be > 0"
                )));
            }
            if !(b.jitter >= 0.0 && b.jitter <= 1.0) {
                return Err(CoreError::ValidationError(format!(
                    "services[{i}].backoffConfig.jitter: must be between 0.0 and 1.0"
                )));
            }
            if b.multiplier <= 0.0 {
                return Err(CoreError::ValidationError(format!(
                    "services[{i}].backoffConfig.multiplier: must be > 0"
                )));
            }

            // timeouts
            if svc.graceful_timeout_secs == 0 {
                return Err(CoreError::ValidationError(format!(
                    "services[{i}].gracefulTimeoutSecs: must be > 0"
                )));
            }
            if svc.startup_timeout_secs == 0 {
                return Err(CoreError::ValidationError(format!(
                    "services[{i}].startupTimeoutSecs: must be > 0"
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
            "services[{index}].{field}.intervalSecs: must be > 0"
        )));
    }
    if timeout_secs == 0 {
        return Err(CoreError::ValidationError(format!(
            "services[{index}].{field}.timeoutSecs: must be > 0"
        )));
    }
    if let Some((fail, succ)) = thresholds {
        if fail == 0 {
            return Err(CoreError::ValidationError(format!(
                "services[{index}].{field}.failureThreshold: must be > 0"
            )));
        }
        if succ == 0 {
            return Err(CoreError::ValidationError(format!(
                "services[{index}].{field}.successThreshold: must be > 0"
            )));
        }
    }

    match kind {
        HealthCheckType::Tcp { port } => {
            if *port == 0 {
                return Err(CoreError::ValidationError(format!(
                    "services[{index}].{field}.type[Tcp].port: must be 1..=65535"
                )));
            }
        }
        HealthCheckType::Exec { command, .. } => {
            if command.trim().is_empty() {
                return Err(CoreError::ValidationError(format!(
                    "services[{index}].{field}.type[Exec].command: cannot be empty"
                )));
            }
        }
    }

    Ok(())
}

/// Load services from a TOML file path
///
/// # Errors
///
/// Returns an error if the file cannot be read or parsed.
pub fn load_services_from_toml_path(path: impl AsRef<Path>) -> Result<ServicesFile> {
    let data = fs::read_to_string(&path).map_err(|e| {
        CoreError::ConfigurationError(format!(
            "Failed to read config {}: {e}",
            path.as_ref().display()
        ))
    })?;
    load_services_from_toml_str(&data)
}

/// Load services from a TOML string
///
/// # Errors
///
/// Returns an error if the input cannot be parsed or validated.
pub fn load_services_from_toml_str(input: &str) -> Result<ServicesFile> {
    // First try the canonical [[services]] array format
    match toml::from_str::<ServicesFile>(input) {
        Ok(cfg) => {
            cfg.validate()?;
            Ok(cfg)
        }
        Err(_e) => {
            // Fall back to keyed-by-id tables format
            let map: ServicesMapFile = toml::from_str(input)
                .map_err(|e| CoreError::ConfigurationError(format!("TOML parse error: {e}")))?;

            // Convert each table into a ServiceSpec by injecting the id field
            let mut services: Vec<ServiceSpec> = Vec::with_capacity(map.services.len());
            for (id, value) in map.services {
                let table = match value {
                    toml::Value::Table(t) => t,
                    other => {
                        return Err(CoreError::ConfigurationError(format!(
                            "Service '{id}' must be a table, found {:?}",
                            other.type_str()
                        )));
                    }
                };

                // Insert id into the table if not already present
                let mut table_with_id = table;
                table_with_id
                    .entry("id".to_string())
                    .or_insert(toml::Value::String(id.clone()));

                // Deserialize into ServiceSpec using the schema serde config (handles defaults and aliases)
                let spec: ServiceSpec = table_with_id.try_into().map_err(|e| {
                    CoreError::ConfigurationError(format!("Failed to parse service '{id}': {e}"))
                })?;
                services.push(spec);
            }

            let cfg = ServicesFile { services };
            cfg.validate()?;
            Ok(cfg)
        }
    }
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
        assert!(format!("{err}").contains("services: must contain at least one service"));
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
        assert!(format!("{err}").contains("duplicate id"));
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
        assert!(format!("{err}").contains("type[Tcp].port"));
    }

    #[test]
    fn errors_on_empty_id() {
        let input = r#"
        [[services]]
        id = "   "
        name = "A"
        command = "echo"
        "#;
        let err = load_services_from_toml_str(input).unwrap_err();
        assert!(format!("{err}").contains(".id: cannot be empty"));
    }

    #[test]
    fn errors_on_empty_name() {
        let input = r#"
        [[services]]
        id = "s"
        name = ""
        command = "echo"
        "#;
        let err = load_services_from_toml_str(input).unwrap_err();
        assert!(format!("{err}").contains(".name: cannot be empty"));
    }

    #[test]
    fn errors_on_empty_command() {
        let input = r#"
        [[services]]
        id = "s"
        name = "S"
        command = "   "
        "#;
        let err = load_services_from_toml_str(input).unwrap_err();
        assert!(format!("{err}").contains(".command: cannot be empty"));
    }

    #[test]
    fn errors_on_empty_environment_key() {
        let input = r#"
        [[services]]
        id = "s"
        name = "S"
        command = "echo"
        [services.environment]
        "" = "value"
        "#;
        let err = load_services_from_toml_str(input).unwrap_err();
        assert!(format!("{err}").contains(".environment: keys cannot be empty"));
    }

    #[test]
    fn errors_on_backoff_base_delay_zero() {
        let input = r#"
        [[services]]
        id = "s"
        name = "S"
        command = "echo"
        [services.backoffConfig]
        baseDelaySecs = 0
        multiplier = 2.0
        maxDelaySecs = 60
        jitter = 0.0
        failureWindowSecs = 300
        "#;
        let err = load_services_from_toml_str(input).unwrap_err();
        assert!(format!("{err}").contains("backoffConfig.baseDelaySecs: must be > 0"));
    }

    #[test]
    fn errors_on_backoff_jitter_out_of_range() {
        let input = r#"
        [[services]]
        id = "s"
        name = "S"
        command = "echo"
        [services.backoffConfig]
        baseDelaySecs = 1
        multiplier = 2.0
        maxDelaySecs = 60
        jitter = 1.5
        failureWindowSecs = 300
        "#;
        let err = load_services_from_toml_str(input).unwrap_err();
        assert!(format!("{err}").contains("backoffConfig.jitter: must be between 0.0 and 1.0"));
    }

    #[test]
    fn errors_on_backoff_multiplier_zero() {
        let input = r#"
        [[services]]
        id = "s"
        name = "S"
        command = "echo"
        [services.backoffConfig]
        baseDelaySecs = 1
        multiplier = 0.0
        maxDelaySecs = 60
        jitter = 0.0
        failureWindowSecs = 300
        "#;
        let err = load_services_from_toml_str(input).unwrap_err();
        assert!(format!("{err}").contains("backoffConfig.multiplier: must be > 0"));
    }

    #[test]
    fn errors_on_graceful_timeout_zero() {
        let input = r#"
        [[services]]
        id = "s"
        name = "S"
        command = "echo"
        gracefulTimeoutSecs = 0
        "#;
        let err = load_services_from_toml_str(input).unwrap_err();
        assert!(format!("{err}").contains("gracefulTimeoutSecs: must be > 0"));
    }

    #[test]
    fn errors_on_startup_timeout_zero() {
        let input = r#"
        [[services]]
        id = "s"
        name = "S"
        command = "echo"
        startupTimeoutSecs = 0
        "#;
        let err = load_services_from_toml_str(input).unwrap_err();
        assert!(format!("{err}").contains("startupTimeoutSecs: must be > 0"));
    }

    #[test]
    fn errors_on_health_check_interval_zero() {
        let input = r#"
        [[services]]
        id = "s"
        name = "S"
        command = "echo"
        [services.healthCheck]
        intervalSecs = 0
        timeoutSecs = 5
        failureThreshold = 3
        successThreshold = 1
        [services.healthCheck.checkType]
        type = "tcp"
        port = 8080
        "#;
        let err = load_services_from_toml_str(input).unwrap_err();
        assert!(format!("{err}").contains("healthCheck.intervalSecs: must be > 0"));
    }

    #[test]
    fn errors_on_health_check_timeout_zero() {
        let input = r#"
        [[services]]
        id = "s"
        name = "S"
        command = "echo"
        [services.healthCheck]
        intervalSecs = 10
        timeoutSecs = 0
        failureThreshold = 3
        successThreshold = 1
        [services.healthCheck.checkType]
        type = "tcp"
        port = 8080
        "#;
        let err = load_services_from_toml_str(input).unwrap_err();
        assert!(format!("{err}").contains("healthCheck.timeoutSecs: must be > 0"));
    }

    #[test]
    fn errors_on_health_check_failure_threshold_zero() {
        let input = r#"
        [[services]]
        id = "s"
        name = "S"
        command = "echo"
        [services.healthCheck]
        intervalSecs = 10
        timeoutSecs = 5
        failureThreshold = 0
        successThreshold = 1
        [services.healthCheck.checkType]
        type = "tcp"
        port = 8080
        "#;
        let err = load_services_from_toml_str(input).unwrap_err();
        assert!(format!("{err}").contains("healthCheck.failureThreshold: must be > 0"));
    }

    #[test]
    fn errors_on_exec_check_empty_command() {
        let input = r#"
        [[services]]
        id = "s"
        name = "S"
        command = "echo"
        [services.healthCheck]
        intervalSecs = 10
        timeoutSecs = 5
        failureThreshold = 3
        successThreshold = 1
        [services.healthCheck.checkType]
        type = "exec"
        command = "   "
        "#;
        let err = load_services_from_toml_str(input).unwrap_err();
        assert!(format!("{err}").contains("type[Exec].command: cannot be empty"));
    }

    #[test]
    fn accepts_valid_exec_health_check() {
        let input = r#"
        [[services]]
        id = "s"
        name = "S"
        command = "echo"
        [services.healthCheck]
        intervalSecs = 10
        timeoutSecs = 5
        failureThreshold = 3
        successThreshold = 1
        [services.healthCheck.checkType]
        type = "exec"
        command = "curl -f http://localhost:8080/health"
        "#;
        let cfg = load_services_from_toml_str(input).expect("should parse exec health check");
        assert_eq!(cfg.services.len(), 1);
    }

    #[test]
    fn errors_on_readiness_check_interval_zero() {
        let input = r#"
        [[services]]
        id = "s"
        name = "S"
        command = "echo"
        [services.readinessCheck]
        intervalSecs = 0
        timeoutSecs = 5
        successThreshold = 1
        [services.readinessCheck.checkType]
        type = "tcp"
        port = 8080
        "#;
        let err = load_services_from_toml_str(input).unwrap_err();
        assert!(format!("{err}").contains("readinessCheck.intervalSecs: must be > 0"));
    }

    #[test]
    fn parses_keyed_by_id_format() {
        let input = r#"
        [svc1]
        name = "Service One"
        command = "echo"
        args = ["hello"]

        [svc2]
        name = "Service Two"
        command = "sh"
        "#;
        let cfg = load_services_from_toml_str(input).expect("should parse keyed format");
        assert_eq!(cfg.services.len(), 2);
        // IDs should be injected from table keys
        let ids: std::collections::HashSet<_> = cfg.services.iter().map(|s| s.id.as_str()).collect();
        assert!(ids.contains("svc1"));
        assert!(ids.contains("svc2"));
    }

    // SimpleServicesFile tests
    #[test]
    fn simple_services_errors_on_empty() {
        let err = load_simple_services_from_toml_str("").unwrap_err();
        assert!(format!("{err}").contains("at least one service"));
    }

    #[test]
    fn simple_services_errors_on_empty_hostname() {
        let input = r#"
        [web]
        hostname = "   "
        "#;
        let err = load_simple_services_from_toml_str(input).unwrap_err();
        assert!(format!("{err}").contains("hostname cannot be empty"));
    }

    #[test]
    fn simple_services_errors_on_port_zero() {
        let input = r#"
        [web]
        port = 0
        "#;
        let err = load_simple_services_from_toml_str(input).unwrap_err();
        assert!(format!("{err}").contains("port must be 1..=65535"));
    }

    #[test]
    fn simple_services_accepts_valid_config() {
        let input = r#"
        [web]
        hostname = "app.dev"
        port = 3000

        [api]
        port = 4000
        "#;
        let cfg = load_simple_services_from_toml_str(input).expect("should parse");
        assert_eq!(cfg.services.len(), 2);
        assert_eq!(cfg.services["web"].hostname.as_deref(), Some("app.dev"));
        assert_eq!(cfg.services["web"].port, Some(3000));
        assert_eq!(cfg.services["api"].hostname, None);
    }

    #[test]
    fn simple_services_accepts_minimal_config() {
        // Service with no hostname or port is valid
        let input = r#"
        [svc]
        "#;
        let cfg = load_simple_services_from_toml_str(input).expect("should parse minimal");
        assert_eq!(cfg.services.len(), 1);
        assert!(cfg.services["svc"].hostname.is_none());
        assert!(cfg.services["svc"].port.is_none());
    }

    #[test]
    fn errors_on_health_check_success_threshold_zero() {
        let input = r#"
        [[services]]
        id = "s"
        name = "S"
        command = "echo"
        [services.healthCheck]
        intervalSecs = 10
        timeoutSecs = 5
        failureThreshold = 3
        successThreshold = 0
        [services.healthCheck.checkType]
        type = "tcp"
        port = 8080
        "#;
        let err = load_services_from_toml_str(input).unwrap_err();
        assert!(format!("{err}").contains("healthCheck.successThreshold: must be > 0"));
    }

    #[test]
    fn errors_on_readiness_check_timeout_zero() {
        let input = r#"
        [[services]]
        id = "s"
        name = "S"
        command = "echo"
        [services.readinessCheck]
        intervalSecs = 10
        timeoutSecs = 0
        successThreshold = 1
        [services.readinessCheck.checkType]
        type = "tcp"
        port = 8080
        "#;
        let err = load_services_from_toml_str(input).unwrap_err();
        assert!(format!("{err}").contains("readinessCheck.timeoutSecs: must be > 0"));
    }

    #[test]
    fn errors_on_readiness_check_success_threshold_zero() {
        let input = r#"
        [[services]]
        id = "s"
        name = "S"
        command = "echo"
        [services.readinessCheck]
        intervalSecs = 10
        timeoutSecs = 5
        successThreshold = 0
        [services.readinessCheck.checkType]
        type = "tcp"
        port = 8080
        "#;
        let err = load_services_from_toml_str(input).unwrap_err();
        assert!(format!("{err}").contains("readinessCheck.successThreshold: must be > 0"));
    }

    #[test]
    fn load_services_from_file_path_missing_returns_error() {
        use std::path::PathBuf;
        let result =
            load_services_from_toml_path(PathBuf::from("/nonexistent/services.toml"));
        assert!(result.is_err());
        assert!(format!("{}", result.unwrap_err()).contains("Failed to read config"));
    }

    #[test]
    fn load_simple_services_from_file_path_missing_returns_error() {
        use std::path::PathBuf;
        let result =
            load_simple_services_from_toml_path(PathBuf::from("/nonexistent/runtime.toml"));
        assert!(result.is_err());
    }
}
