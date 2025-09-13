//! Integration test modules for the supervisor system

#[path = "restart_policy_tests.rs"]
mod restart_policy_tests;

#[path = "proxy_integration_tests.rs"]
mod proxy_integration_tests;
#[path = "supervisor_restart_tests.rs"]
mod supervisor_restart_tests;

// HTTP-based health integration tests removed
