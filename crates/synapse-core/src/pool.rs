use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicU64, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    thread,
    time::Duration,
};

use crate::{
    sandbox::{DefaultSandboxEngine, SandboxEngine, SandboxInstance},
    service, ExecuteRequest, ExecuteResponse, RuntimeRegistry, SynapseError,
};

const DEFAULT_POOL_SIZE: usize = 4;
const REPLENISH_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Clone, Debug)]
pub struct SandboxPool {
    inner: Arc<PoolInner>,
}

#[derive(Debug)]
struct PoolInner {
    configured_size: usize,
    engine: Arc<dyn SandboxEngine>,
    runtime_registry: RuntimeRegistry,
    slots: Mutex<VecDeque<PooledSandbox>>,
    next_slot_id: AtomicUsize,
    pooled_total: AtomicUsize,
    active: AtomicUsize,
    overflow_active: AtomicUsize,
    overflow_total: AtomicU64,
    poisoned_total: AtomicU64,
    requests_total: AtomicU64,
    completed_total: AtomicU64,
    failed_total: AtomicU64,
    timeouts_total: AtomicU64,
}

#[derive(Debug)]
struct PooledSandbox {
    #[allow(dead_code)]
    slot_id: usize,
    sandbox: Box<dyn SandboxInstance>,
}

#[derive(Debug)]
enum LeaseKind {
    Pooled(PooledSandbox),
    Overflow,
}

#[derive(Debug)]
pub struct SandboxLease {
    kind: Option<LeaseKind>,
    healthy: bool,
    inner: Arc<PoolInner>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PoolMetrics {
    pub configured_size: usize,
    pub available: usize,
    pub pooled_total: usize,
    pub active: usize,
    pub overflow_active: usize,
    pub overflow_total: u64,
    pub poisoned_total: u64,
    pub requests_total: u64,
    pub completed_total: u64,
    pub failed_total: u64,
    pub timeouts_total: u64,
}

impl SandboxPool {
    pub fn new(configured_size: usize) -> Self {
        Self::new_with_runtime_registry(configured_size, RuntimeRegistry::default())
    }

    pub fn new_with_runtime_registry(
        configured_size: usize,
        runtime_registry: RuntimeRegistry,
    ) -> Self {
        Self::new_with_engine_and_runtime_registry(
            configured_size,
            Arc::new(DefaultSandboxEngine),
            runtime_registry,
        )
    }

    pub fn new_with_engine_and_runtime_registry(
        configured_size: usize,
        engine: Arc<dyn SandboxEngine>,
        runtime_registry: RuntimeRegistry,
    ) -> Self {
        let configured_size = configured_size.max(1);
        let inner = Arc::new(PoolInner {
            configured_size,
            engine,
            runtime_registry,
            slots: Mutex::new(VecDeque::with_capacity(configured_size)),
            next_slot_id: AtomicUsize::new(0),
            pooled_total: AtomicUsize::new(0),
            active: AtomicUsize::new(0),
            overflow_active: AtomicUsize::new(0),
            overflow_total: AtomicU64::new(0),
            poisoned_total: AtomicU64::new(0),
            requests_total: AtomicU64::new(0),
            completed_total: AtomicU64::new(0),
            failed_total: AtomicU64::new(0),
            timeouts_total: AtomicU64::new(0),
        });

        let pool = Self {
            inner: Arc::clone(&inner),
        };

        for _ in 0..configured_size {
            if !inner.try_replenish_one() {
                break;
            }
        }

        spawn_replenisher(&inner);
        pool
    }

    pub fn default_sized() -> Self {
        Self::new(DEFAULT_POOL_SIZE)
    }

    pub fn acquire(&self) -> SandboxLease {
        self.inner.active.fetch_add(1, Ordering::Relaxed);

        let kind = {
            let mut slots = self
                .inner
                .slots
                .lock()
                .expect("sandbox pool mutex poisoned");
            slots.pop_front().map(LeaseKind::Pooled).unwrap_or_else(|| {
                self.inner.overflow_active.fetch_add(1, Ordering::Relaxed);
                self.inner.overflow_total.fetch_add(1, Ordering::Relaxed);
                LeaseKind::Overflow
            })
        };

        SandboxLease {
            kind: Some(kind),
            healthy: true,
            inner: Arc::clone(&self.inner),
        }
    }

    pub async fn execute(&self, request: ExecuteRequest) -> Result<ExecuteResponse, SynapseError> {
        self.inner.requests_total.fetch_add(1, Ordering::Relaxed);
        let mut lease = self.acquire();
        let result = lease.execute(request).await;

        match &result {
            Ok(response) => {
                self.inner.completed_total.fetch_add(1, Ordering::Relaxed);
                if response.exit_code == -1 && response.stderr.contains("execution timed out") {
                    self.inner.timeouts_total.fetch_add(1, Ordering::Relaxed);
                }
            }
            Err(error) => {
                self.inner.failed_total.fetch_add(1, Ordering::Relaxed);
                if should_poison_sandbox(error) {
                    lease.mark_poisoned();
                }
            }
        }

        result
    }

    pub fn metrics(&self) -> PoolMetrics {
        let available = self
            .inner
            .slots
            .lock()
            .expect("sandbox pool mutex poisoned")
            .len();

        PoolMetrics {
            configured_size: self.inner.configured_size,
            available,
            pooled_total: self.inner.pooled_total.load(Ordering::Relaxed),
            active: self.inner.active.load(Ordering::Relaxed),
            overflow_active: self.inner.overflow_active.load(Ordering::Relaxed),
            overflow_total: self.inner.overflow_total.load(Ordering::Relaxed),
            poisoned_total: self.inner.poisoned_total.load(Ordering::Relaxed),
            requests_total: self.inner.requests_total.load(Ordering::Relaxed),
            completed_total: self.inner.completed_total.load(Ordering::Relaxed),
            failed_total: self.inner.failed_total.load(Ordering::Relaxed),
            timeouts_total: self.inner.timeouts_total.load(Ordering::Relaxed),
        }
    }
}

impl Default for SandboxPool {
    fn default() -> Self {
        Self::default_sized()
    }
}

impl SandboxLease {
    fn mark_poisoned(&mut self) {
        self.healthy = false;
    }

    async fn execute(&mut self, request: ExecuteRequest) -> Result<ExecuteResponse, SynapseError> {
        match self.kind.as_ref() {
            Some(LeaseKind::Pooled(slot)) => {
                service::execute_in_instance_with_registry(
                    slot.sandbox.as_ref(),
                    self.inner.engine.as_ref(),
                    &self.inner.runtime_registry,
                    request,
                )
                .await
            }
            Some(LeaseKind::Overflow) => {
                service::execute_with_engine_and_registry(
                    self.inner.engine.as_ref(),
                    &self.inner.runtime_registry,
                    request,
                )
                .await
            }
            None => Err(SynapseError::Execution(
                "sandbox lease was already released".to_string(),
            )),
        }
    }
}

impl Drop for SandboxLease {
    fn drop(&mut self) {
        if let Some(kind) = self.kind.take() {
            self.inner.active.fetch_sub(1, Ordering::Relaxed);
            match kind {
                LeaseKind::Pooled(slot) => {
                    if self.healthy {
                        let mut slots = self
                            .inner
                            .slots
                            .lock()
                            .expect("sandbox pool mutex poisoned");
                        slots.push_back(slot);
                    } else {
                        self.inner.poisoned_total.fetch_add(1, Ordering::Relaxed);
                        self.inner.pooled_total.fetch_sub(1, Ordering::Relaxed);
                        let _ = slot.sandbox.destroy_blocking();
                    }
                }
                LeaseKind::Overflow => {
                    self.inner.overflow_active.fetch_sub(1, Ordering::Relaxed);
                }
            }
        }
    }
}

impl PoolInner {
    fn try_replenish_one(&self) -> bool {
        if self.pooled_total.load(Ordering::Relaxed) >= self.configured_size {
            return false;
        }

        let Ok(sandbox) = self.engine.prepare_blocking() else {
            return false;
        };

        let slot = PooledSandbox {
            slot_id: self.next_slot_id.fetch_add(1, Ordering::Relaxed),
            sandbox,
        };

        let mut slots = self.slots.lock().expect("sandbox pool mutex poisoned");
        if self.pooled_total.load(Ordering::Relaxed) >= self.configured_size {
            drop(slots);
            let _ = slot.sandbox.destroy_blocking();
            return false;
        }

        slots.push_back(slot);
        self.pooled_total.fetch_add(1, Ordering::Relaxed);
        true
    }
}

fn spawn_replenisher(inner: &Arc<PoolInner>) {
    let weak = Arc::downgrade(inner);
    thread::spawn(move || {
        while let Some(inner) = weak.upgrade() {
            if inner.pooled_total.load(Ordering::Relaxed) < inner.configured_size
                && inner.try_replenish_one()
            {
                continue;
            }
            thread::sleep(REPLENISH_INTERVAL);
        }
    });
}

fn should_poison_sandbox(error: &SynapseError) -> bool {
    matches!(error, SynapseError::Execution(_) | SynapseError::Io(_))
}

#[cfg(test)]
mod tests {
    use std::{thread, time::Duration};

    use super::SandboxPool;

    #[test]
    fn pooled_slots_return_to_pool_on_drop() {
        let pool = SandboxPool::new(2);
        let first = pool.acquire();
        let second = pool.acquire();

        let metrics = pool.metrics();
        assert_eq!(metrics.available, 0);
        assert_eq!(metrics.pooled_total, 2);
        assert_eq!(metrics.active, 2);
        assert_eq!(metrics.overflow_active, 0);

        drop(first);
        drop(second);

        let metrics = pool.metrics();
        assert_eq!(metrics.available, 2);
        assert_eq!(metrics.pooled_total, 2);
        assert_eq!(metrics.active, 0);
    }

    #[test]
    fn exhausted_pool_degrades_to_overflow_execution() {
        let pool = SandboxPool::new(1);
        let pooled = pool.acquire();
        let overflow = pool.acquire();

        let metrics = pool.metrics();
        assert_eq!(metrics.available, 0);
        assert_eq!(metrics.pooled_total, 1);
        assert_eq!(metrics.active, 2);
        assert_eq!(metrics.overflow_active, 1);
        assert_eq!(metrics.overflow_total, 1);

        drop(overflow);
        drop(pooled);

        let metrics = pool.metrics();
        assert_eq!(metrics.available, 1);
        assert_eq!(metrics.pooled_total, 1);
        assert_eq!(metrics.active, 0);
        assert_eq!(metrics.overflow_active, 0);
        assert_eq!(metrics.overflow_total, 1);
    }

    #[test]
    fn poisoned_slots_are_replenished_in_background() {
        let pool = SandboxPool::new(1);
        let mut lease = pool.acquire();
        lease.mark_poisoned();
        drop(lease);

        for _ in 0..20 {
            let metrics = pool.metrics();
            if metrics.available == 1 && metrics.pooled_total == 1 {
                assert_eq!(metrics.poisoned_total, 1);
                return;
            }
            thread::sleep(Duration::from_millis(50));
        }

        let metrics = pool.metrics();
        assert_eq!(metrics.poisoned_total, 1);
        assert_eq!(metrics.available, 1);
        assert_eq!(metrics.pooled_total, 1);
    }

    #[test]
    fn pooled_sandboxes_have_unique_slot_ids() {
        let pool = SandboxPool::new(2);
        let first = pool.acquire();
        let second = pool.acquire();

        let ids = [
            match first.kind.as_ref().unwrap() {
                super::LeaseKind::Pooled(slot) => slot.slot_id,
                super::LeaseKind::Overflow => usize::MAX,
            },
            match second.kind.as_ref().unwrap() {
                super::LeaseKind::Pooled(slot) => slot.slot_id,
                super::LeaseKind::Overflow => usize::MAX,
            },
        ];

        assert_ne!(ids[0], ids[1]);
    }
}
