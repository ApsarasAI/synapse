use synapse_core::{SandboxPool, SynapseConfig, SystemProviders};

#[derive(Clone, Debug)]
pub struct AppState {
    pool: SandboxPool,
}

impl AppState {
    pub fn new(pool: SandboxPool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &SandboxPool {
        &self.pool
    }
}

pub fn default_state() -> AppState {
    let config = SynapseConfig::from_providers(&SystemProviders);
    AppState::new(SandboxPool::new(config.pool_size))
}
