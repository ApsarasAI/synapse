use criterion::{criterion_group, criterion_main, Criterion};
use synapse_core::{
    execute_in_prepared, prepare_sandbox_blocking, ExecuteRequest, RuntimeRegistry, SandboxPool,
};

fn bench_pool_acquire(c: &mut Criterion) {
    let pool = SandboxPool::new(4);
    c.bench_function("pool_acquire", |b| {
        b.iter(|| {
            let lease = pool.acquire();
            drop(lease);
        });
    });
}

fn bench_sandbox_create(c: &mut Criterion) {
    c.bench_function("sandbox_create", |b| {
        b.iter(|| {
            let sandbox = prepare_sandbox_blocking().expect("sandbox should be created");
            sandbox
                .destroy_blocking()
                .expect("sandbox should be cleaned up");
        });
    });
}

fn bench_execute_hello(c: &mut Criterion) {
    let runtime = tokio::runtime::Runtime::new().expect("runtime should be created");
    if !runtime.block_on(python3_available()) {
        return;
    }

    let registry = RuntimeRegistry::default();
    if registry.verify("python", None).is_err() {
        return;
    }
    let sandbox = prepare_sandbox_blocking().expect("sandbox should be created");

    c.bench_function("execute_fast_path", |b| {
        b.to_async(&runtime).iter(|| async {
            let response = execute_in_prepared(&sandbox, request())
                .await
                .expect("execution should succeed");
            assert_eq!(response.stdout, "hello\n");
        });
    });

    sandbox
        .destroy_blocking()
        .expect("sandbox should be cleaned up");
}

fn bench_sandbox_recycle(c: &mut Criterion) {
    let sandbox = prepare_sandbox_blocking().expect("sandbox should be created");
    c.bench_function("sandbox_recycle", |b| {
        b.iter(|| {
            sandbox.reset_blocking().expect("sandbox should be reset");
        });
    });
    sandbox
        .destroy_blocking()
        .expect("sandbox should be cleaned up");
}

async fn python3_available() -> bool {
    tokio::process::Command::new("python3")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await
        .is_ok()
}

fn request() -> ExecuteRequest {
    ExecuteRequest {
        language: "python".to_string(),
        code: "print('hello')\n".to_string(),
        timeout_ms: 5_000,
        cpu_time_limit_ms: Some(5_000),
        memory_limit_mb: 128,
        runtime_version: None,
        tenant_id: None,
        request_id: None,
    }
}

criterion_group!(
    mvp,
    bench_pool_acquire,
    bench_sandbox_create,
    bench_execute_hello,
    bench_sandbox_recycle
);
criterion_main!(mvp);
