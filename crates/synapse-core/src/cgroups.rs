use std::{ffi::OsString, path::PathBuf};

pub use synapse_engine::cgroups::CgroupSupport;

use crate::{Providers, SynapseError};

pub fn probe_support(providers: &dyn Providers) -> Result<CgroupSupport, SynapseError> {
    synapse_engine::cgroups::probe_support(&EngineProviders { inner: providers })
        .map_err(SynapseError::from)
}

struct EngineProviders<'a> {
    inner: &'a dyn Providers,
}

impl synapse_engine::Providers for EngineProviders<'_> {
    fn env_var(&self, key: &str) -> Option<String> {
        self.inner.env_var(key)
    }

    fn env_var_os(&self, key: &str) -> Option<OsString> {
        self.inner.env_var_os(key)
    }

    fn temp_dir(&self) -> PathBuf {
        self.inner.temp_dir()
    }

    fn process_id(&self) -> u32 {
        self.inner.process_id()
    }

    fn now_unix_nanos(&self) -> u128 {
        self.inner.now_unix_nanos()
    }
}
