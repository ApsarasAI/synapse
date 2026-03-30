use std::{
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::{validate_request_id, ErrorCode, Providers, SynapseError, SystemProviders};

const REQUEST_SUMMARY_ROOT_ENV: &str = "SYNAPSE_REQUEST_SUMMARY_ROOT";
const DEFAULT_REQUEST_SUMMARY_DIR_NAME: &str = "synapse-request-summaries";
const DEFAULT_QUERY_LIMIT: usize = 50;
const MAX_QUERY_LIMIT: usize = 200;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RequestStatus {
    Success,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RequestSummary {
    pub request_id: String,
    pub tenant_id: String,
    pub language: String,
    pub status: RequestStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code: Option<ErrorCode>,
    pub created_at_ms: u64,
    pub completed_at_ms: u64,
    pub duration_ms: u64,
    pub queue_wait_ms: u64,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime_language: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime_version: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RequestSummaryQuery {
    pub request_id: Option<String>,
    pub tenant_id: Option<String>,
    pub status: Option<RequestStatus>,
    pub error_code: Option<ErrorCode>,
    pub language: Option<String>,
    pub from_created_at_ms: Option<u64>,
    pub to_created_at_ms: Option<u64>,
    pub allowed_tenants: Option<Vec<String>>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct RequestSummaryStore {
    root: PathBuf,
}

impl Default for RequestSummaryStore {
    fn default() -> Self {
        Self::from_providers(&SystemProviders)
    }
}

impl RequestSummaryStore {
    pub fn from_providers(providers: &dyn Providers) -> Self {
        let root = providers
            .env_var(REQUEST_SUMMARY_ROOT_ENV)
            .map(PathBuf::from)
            .unwrap_or_else(|| providers.temp_dir().join(DEFAULT_REQUEST_SUMMARY_DIR_NAME));
        Self { root }
    }

    pub fn from_root(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn persist(&self, summary: &RequestSummary) -> Result<(), SynapseError> {
        validate_request_id(&summary.request_id)?;
        fs::create_dir_all(&self.root)?;
        let path = self.path_for(&summary.request_id);
        let bytes = serde_json::to_vec_pretty(summary).map_err(|error| {
            SynapseError::Audit(format!("failed to serialize request summary: {error}"))
        })?;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|error| {
                if error.kind() == ErrorKind::AlreadyExists {
                    SynapseError::Audit(format!(
                        "request summary already exists for request_id {}",
                        summary.request_id
                    ))
                } else {
                    error.into()
                }
            })?;
        use std::io::Write as _;
        file.write_all(&bytes)?;
        Ok(())
    }

    pub fn load(&self, request_id: &str) -> Result<RequestSummary, SynapseError> {
        validate_request_id(request_id)?;
        let bytes = fs::read(self.path_for(request_id))?;
        serde_json::from_slice(&bytes).map_err(|error| {
            SynapseError::Audit(format!("failed to parse request summary: {error}"))
        })
    }

    pub fn list(&self, query: &RequestSummaryQuery) -> Result<Vec<RequestSummary>, SynapseError> {
        let mut items = Vec::new();
        let limit = query
            .limit
            .unwrap_or(DEFAULT_QUERY_LIMIT)
            .clamp(1, MAX_QUERY_LIMIT);
        let entries = match fs::read_dir(&self.root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(items),
            Err(error) => return Err(error.into()),
        };

        for entry in entries {
            let entry = entry?;
            if entry.path().extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }

            let bytes = fs::read(entry.path())?;
            let summary: RequestSummary = serde_json::from_slice(&bytes).map_err(|error| {
                SynapseError::Audit(format!("failed to parse request summary: {error}"))
            })?;

            if query.matches(&summary) {
                items.push(summary);
            }
        }

        items.sort_by(|left, right| {
            right
                .created_at_ms
                .cmp(&left.created_at_ms)
                .then_with(|| right.request_id.cmp(&left.request_id))
        });
        items.truncate(limit);
        Ok(items)
    }

    fn path_for(&self, request_id: &str) -> PathBuf {
        self.root.join(format!("{request_id}.json"))
    }
}

impl RequestSummaryQuery {
    fn matches(&self, summary: &RequestSummary) -> bool {
        if let Some(request_id) = self.request_id.as_deref() {
            if summary.request_id != request_id {
                return false;
            }
        }
        if let Some(tenant_id) = self.tenant_id.as_deref() {
            if summary.tenant_id != tenant_id {
                return false;
            }
        }
        if let Some(status) = self.status {
            if summary.status != status {
                return false;
            }
        }
        if let Some(error_code) = self.error_code {
            if summary.error_code != Some(error_code) {
                return false;
            }
        }
        if let Some(language) = self.language.as_deref() {
            if summary.language != language {
                return false;
            }
        }
        if let Some(from_created_at_ms) = self.from_created_at_ms {
            if summary.created_at_ms < from_created_at_ms {
                return false;
            }
        }
        if let Some(to_created_at_ms) = self.to_created_at_ms {
            if summary.created_at_ms > to_created_at_ms {
                return false;
            }
        }
        if let Some(allowed_tenants) = self.allowed_tenants.as_ref() {
            if !allowed_tenants
                .iter()
                .any(|tenant| tenant == &summary.tenant_id)
            {
                return false;
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::{RequestStatus, RequestSummary, RequestSummaryQuery, RequestSummaryStore};
    use crate::ErrorCode;
    use std::{
        env, fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn unique_root(prefix: &str) -> PathBuf {
        let path = env::temp_dir().join(format!(
            "{prefix}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&path);
        path
    }

    fn summary(request_id: &str, tenant_id: &str, created_at_ms: u64) -> RequestSummary {
        RequestSummary {
            request_id: request_id.to_string(),
            tenant_id: tenant_id.to_string(),
            language: "python".to_string(),
            status: RequestStatus::Error,
            error_code: Some(ErrorCode::WallTimeout),
            created_at_ms,
            completed_at_ms: created_at_ms + 10,
            duration_ms: 10,
            queue_wait_ms: 0,
            stdout_truncated: false,
            stderr_truncated: true,
            runtime_language: Some("python".to_string()),
            runtime_version: Some("system".to_string()),
        }
    }

    #[test]
    fn request_summary_store_round_trips_records() {
        let root = unique_root("synapse-request-summary-round-trip");
        let store = RequestSummaryStore::from_root(&root);
        let item = summary("req_round_trip", "tenant-a", 100);

        store.persist(&item).unwrap();
        let loaded = store.load("req_round_trip").unwrap();

        assert_eq!(loaded, item);
    }

    #[test]
    fn request_summary_query_filters_and_sorts() {
        let root = unique_root("synapse-request-summary-query");
        let store = RequestSummaryStore::from_root(&root);
        store.persist(&summary("req_old", "tenant-a", 100)).unwrap();
        let mut success = summary("req_new", "tenant-b", 300);
        success.status = RequestStatus::Success;
        success.error_code = None;
        success.stderr_truncated = false;
        store.persist(&success).unwrap();
        store.persist(&summary("req_mid", "tenant-a", 200)).unwrap();

        let items = store
            .list(&RequestSummaryQuery {
                tenant_id: Some("tenant-a".to_string()),
                allowed_tenants: Some(vec!["tenant-a".to_string()]),
                limit: Some(10),
                ..RequestSummaryQuery::default()
            })
            .unwrap();

        assert_eq!(items.len(), 2);
        assert_eq!(items[0].request_id, "req_mid");
        assert_eq!(items[1].request_id, "req_old");
    }

    #[test]
    fn request_summary_query_caps_limit() {
        let root = unique_root("synapse-request-summary-limit");
        let store = RequestSummaryStore::from_root(&root);
        for index in 0..3 {
            store
                .persist(&summary(&format!("req_{index}"), "tenant-a", index))
                .unwrap();
        }

        let items = store
            .list(&RequestSummaryQuery {
                limit: Some(1),
                ..RequestSummaryQuery::default()
            })
            .unwrap();

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].request_id, "req_2");
    }

    #[test]
    fn request_summary_store_rejects_overwrite_attempts() {
        let root = unique_root("synapse-request-summary-overwrite");
        let store = RequestSummaryStore::from_root(&root);
        let item = summary("req_duplicate", "tenant-a", 100);

        store.persist(&item).unwrap();
        let error = store.persist(&item).unwrap_err();

        assert!(error.to_string().contains("request summary already exists"));
    }
}
