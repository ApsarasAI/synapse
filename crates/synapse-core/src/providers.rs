use std::{
    env,
    ffi::OsString,
    path::PathBuf,
    process,
    time::{SystemTime, UNIX_EPOCH},
};

/// Single explicit entry for cross-cutting concerns (phase 1).
///
/// This is intentionally small right now: enough to centralize env/config reads,
/// PATH-based command lookup, and temp workspace path generation.
pub trait Providers: Send + Sync {
    fn env_var(&self, key: &str) -> Option<String>;
    fn env_var_os(&self, key: &str) -> Option<OsString>;
    fn temp_dir(&self) -> PathBuf;
    fn process_id(&self) -> u32;
    fn now_unix_nanos(&self) -> u128;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SystemProviders;

impl Providers for SystemProviders {
    fn env_var(&self, key: &str) -> Option<String> {
        env::var(key).ok()
    }

    fn env_var_os(&self, key: &str) -> Option<OsString> {
        env::var_os(key)
    }

    fn temp_dir(&self) -> PathBuf {
        env::temp_dir()
    }

    fn process_id(&self) -> u32 {
        process::id()
    }

    fn now_unix_nanos(&self) -> u128 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    }
}

pub fn find_command(providers: &dyn Providers, command: &str) -> Option<PathBuf> {
    let path = providers.env_var_os("PATH")?;
    env::split_paths(&path)
        .map(|dir| dir.join(command))
        .find(|candidate| candidate.is_file())
}

pub fn temp_path(providers: &dyn Providers, prefix: &str) -> PathBuf {
    let pid = providers.process_id();
    let nanos = providers.now_unix_nanos();
    providers.temp_dir().join(format!("{prefix}-{pid}-{nanos}"))
}

#[cfg(test)]
mod tests {
    use super::{find_command, temp_path, Providers};
    use std::{collections::HashMap, env, ffi::OsString, fs, path::PathBuf};

    #[derive(Debug, Default)]
    struct FakeProviders {
        env: HashMap<String, OsString>,
        temp_dir: PathBuf,
        pid: u32,
        nanos: u128,
    }

    impl Providers for FakeProviders {
        fn env_var(&self, key: &str) -> Option<String> {
            self.env
                .get(key)
                .map(|value| value.to_string_lossy().into_owned())
        }

        fn env_var_os(&self, key: &str) -> Option<OsString> {
            self.env.get(key).cloned()
        }

        fn temp_dir(&self) -> PathBuf {
            self.temp_dir.clone()
        }

        fn process_id(&self) -> u32 {
            self.pid
        }

        fn now_unix_nanos(&self) -> u128 {
            self.nanos
        }
    }

    #[test]
    fn temp_path_includes_prefix_pid_and_nanos() {
        let root = env::temp_dir().join("synapse-providers-test");
        let _ = fs::create_dir_all(&root);

        let fake = FakeProviders {
            temp_dir: root.clone(),
            pid: 123,
            nanos: 456,
            ..Default::default()
        };

        let path = temp_path(&fake, "synapse");
        assert!(path.starts_with(&root));
        assert!(path.to_string_lossy().contains("synapse-123-456"));
    }

    #[test]
    fn find_command_searches_in_path() {
        let root = env::temp_dir().join("synapse-find-command-test");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();

        let marker = root.join("mybin");
        fs::write(&marker, b"ok").unwrap();

        let mut fake = FakeProviders {
            temp_dir: env::temp_dir(),
            pid: 1,
            nanos: 1,
            ..Default::default()
        };
        fake.env
            .insert("PATH".to_string(), root.as_os_str().to_os_string());

        let found = find_command(&fake, "mybin").unwrap();
        assert_eq!(found, marker);

        let _ = fs::remove_dir_all(&root);
    }
}
