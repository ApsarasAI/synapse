use std::{
    collections::BTreeMap,
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

use crate::{temp_path, Providers, SynapseError, SystemProviders};

const AUDIT_ROOT_ENV: &str = "SYNAPSE_AUDIT_ROOT";
const DEFAULT_AUDIT_DIR_NAME: &str = "synapse-audit";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AuditEventKind {
    RequestReceived,
    QuotaAccepted,
    QuotaRejected,
    SandboxPrepared,
    SandboxReset,
    SandboxDestroyed,
    CommandPrepared,
    ExecutionStarted,
    ExecutionFinished,
    LimitExceeded,
    PolicyBlocked,
    FileAccess,
    NetworkAttempt,
    ProcessSpawn,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuditEvent {
    pub request_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tenant_id: Option<String>,
    pub kind: AuditEventKind,
    pub message: String,
    pub timestamp_ms: u64,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub fields: BTreeMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct AuditLog {
    root: PathBuf,
}

impl Default for AuditLog {
    fn default() -> Self {
        Self::from_providers(&SystemProviders)
    }
}

impl AuditLog {
    pub fn from_providers(providers: &dyn Providers) -> Self {
        let root = providers
            .env_var(AUDIT_ROOT_ENV)
            .map(PathBuf::from)
            .unwrap_or_else(|| providers.temp_dir().join(DEFAULT_AUDIT_DIR_NAME));
        Self { root }
    }

    pub fn from_root(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn persist(&self, request_id: &str, events: &[AuditEvent]) -> Result<(), SynapseError> {
        validate_request_id(request_id)?;
        fs::create_dir_all(&self.root)?;
        let path = self.root.join(format!("{request_id}.json"));
        let bytes = serde_json::to_vec_pretty(events).map_err(|error| {
            SynapseError::Audit(format!("failed to serialize audit events: {error}"))
        })?;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|error| {
                if error.kind() == ErrorKind::AlreadyExists {
                    SynapseError::Audit(format!(
                        "audit record already exists for request_id {request_id}"
                    ))
                } else {
                    error.into()
                }
            })?;
        use std::io::Write as _;
        file.write_all(&bytes)?;
        Ok(())
    }

    pub fn load(&self, request_id: &str) -> Result<Vec<AuditEvent>, SynapseError> {
        validate_request_id(request_id)?;
        let path = self.root.join(format!("{request_id}.json"));
        let bytes = fs::read(path)?;
        serde_json::from_slice(&bytes)
            .map_err(|error| SynapseError::Audit(format!("failed to parse audit events: {error}")))
    }
}

pub fn validate_request_id(request_id: &str) -> Result<(), SynapseError> {
    let trimmed = request_id.trim();
    if trimmed.is_empty() {
        return Err(SynapseError::InvalidInput(
            "request_id cannot be empty".to_string(),
        ));
    }
    if trimmed.len() > 128 {
        return Err(SynapseError::InvalidInput(
            "request_id exceeds 128 characters".to_string(),
        ));
    }
    if !trimmed
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(SynapseError::InvalidInput(
            "request_id must use only ASCII letters, digits, '-' or '_'".to_string(),
        ));
    }
    Ok(())
}

pub fn new_request_id(providers: &dyn Providers) -> String {
    temp_path(providers, "synapse-request")
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "synapse-request-unknown".to_string())
}

pub fn audit_event(
    request_id: impl Into<String>,
    tenant_id: Option<&str>,
    kind: AuditEventKind,
    message: impl Into<String>,
) -> AuditEvent {
    AuditEvent {
        request_id: request_id.into(),
        tenant_id: tenant_id.map(str::to_string),
        kind,
        message: message.into(),
        timestamp_ms: now_unix_ms(),
        fields: BTreeMap::new(),
    }
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}

#[cfg(test)]
mod tests {
    use super::{audit_event, new_request_id, validate_request_id, AuditEventKind, AuditLog};
    use crate::{Providers, SystemProviders};
    use std::{env, ffi::OsString, path::PathBuf};

    #[derive(Debug)]
    struct FakeProviders;

    impl Providers for FakeProviders {
        fn env_var(&self, _key: &str) -> Option<String> {
            None
        }

        fn env_var_os(&self, _key: &str) -> Option<OsString> {
            None
        }

        fn temp_dir(&self) -> PathBuf {
            env::temp_dir()
        }

        fn process_id(&self) -> u32 {
            7
        }

        fn now_unix_nanos(&self) -> u128 {
            9
        }
    }

    #[test]
    fn request_ids_are_derived_from_temp_path() {
        assert_eq!(new_request_id(&FakeProviders), "synapse-request-7-9");
    }

    #[test]
    fn audit_log_round_trips_events() {
        let log = AuditLog::from_providers(&SystemProviders);
        let request_id = format!("audit-test-{}", std::process::id());
        let event = audit_event(
            request_id.clone(),
            Some("tenant-a"),
            AuditEventKind::RequestReceived,
            "accepted",
        );

        log.persist(&request_id, std::slice::from_ref(&event))
            .unwrap();
        let loaded = log.load(&request_id).unwrap();

        assert_eq!(loaded, vec![event]);
        let _ = std::fs::remove_file(log.root().join(format!("{request_id}.json")));
    }

    #[test]
    fn request_id_validation_rejects_path_traversal() {
        let error = validate_request_id("../../tmp/owned").unwrap_err();
        assert!(error.to_string().contains("request_id must use only"));
    }

    #[test]
    fn audit_log_rejects_overwrite_attempts() {
        let log = AuditLog::from_providers(&SystemProviders);
        let request_id = format!("audit-overwrite-test-{}", std::process::id());
        let event = audit_event(
            request_id.clone(),
            Some("tenant-a"),
            AuditEventKind::RequestReceived,
            "accepted",
        );

        log.persist(&request_id, std::slice::from_ref(&event))
            .unwrap();
        let error = log.persist(&request_id, &[event]).unwrap_err();

        assert!(error.to_string().contains("already exists"));
        let _ = std::fs::remove_file(log.root().join(format!("{request_id}.json")));
    }
}
