use crate::providers::Providers;

pub const DEFAULT_POOL_SIZE: usize = 4;
pub const POOL_SIZE_ENV: &str = "SYNAPSE_POOL_SIZE";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SynapseConfig {
    pub pool_size: usize,
}

impl Default for SynapseConfig {
    fn default() -> Self {
        Self {
            pool_size: DEFAULT_POOL_SIZE,
        }
    }
}

impl SynapseConfig {
    pub fn from_providers(providers: &dyn Providers) -> Self {
        let pool_size = providers
            .env_var(POOL_SIZE_ENV)
            .and_then(|value| value.parse().ok())
            .filter(|value: &usize| *value > 0)
            .unwrap_or(DEFAULT_POOL_SIZE);

        Self { pool_size }
    }
}

#[cfg(test)]
mod tests {
    use super::{SynapseConfig, DEFAULT_POOL_SIZE, POOL_SIZE_ENV};
    use crate::providers::Providers;
    use std::{collections::HashMap, ffi::OsString, path::PathBuf};

    #[derive(Debug, Default)]
    struct FakeProviders {
        env: HashMap<String, String>,
    }

    impl Providers for FakeProviders {
        fn env_var(&self, key: &str) -> Option<String> {
            self.env.get(key).cloned()
        }

        fn env_var_os(&self, key: &str) -> Option<OsString> {
            self.env.get(key).map(|v| OsString::from(v.clone()))
        }

        fn temp_dir(&self) -> PathBuf {
            PathBuf::from("/tmp")
        }

        fn process_id(&self) -> u32 {
            1
        }

        fn now_unix_nanos(&self) -> u128 {
            1
        }
    }

    #[test]
    fn defaults_when_env_missing() {
        let fake = FakeProviders::default();
        let config = SynapseConfig::from_providers(&fake);
        assert_eq!(config.pool_size, DEFAULT_POOL_SIZE);
    }

    #[test]
    fn defaults_when_env_invalid() {
        let mut fake = FakeProviders::default();
        fake.env
            .insert(POOL_SIZE_ENV.to_string(), "nope".to_string());
        let config = SynapseConfig::from_providers(&fake);
        assert_eq!(config.pool_size, DEFAULT_POOL_SIZE);
    }

    #[test]
    fn defaults_when_env_zero() {
        let mut fake = FakeProviders::default();
        fake.env.insert(POOL_SIZE_ENV.to_string(), "0".to_string());
        let config = SynapseConfig::from_providers(&fake);
        assert_eq!(config.pool_size, DEFAULT_POOL_SIZE);
    }

    #[test]
    fn reads_pool_size_from_env() {
        let mut fake = FakeProviders::default();
        fake.env.insert(POOL_SIZE_ENV.to_string(), "8".to_string());
        let config = SynapseConfig::from_providers(&fake);
        assert_eq!(config.pool_size, 8);
    }
}
