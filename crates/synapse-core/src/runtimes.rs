use std::{
    ffi::OsStr,
    fs,
    fs::File,
    io::{Read, Write},
    path::{Component, Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use utoipa::ToSchema;

use crate::{find_command, Providers, SynapseError, SystemProviders};

const RUNTIME_STORE_ENV: &str = "SYNAPSE_RUNTIME_STORE_DIR";
const RUNTIME_BUNDLE_DIR_ENV: &str = "SYNAPSE_RUNTIME_BUNDLE_DIR";
const DEFAULT_RUNTIME_STORE_DIR: &str = "synapse-runtime-store";
const SUPPORTED_PYTHON_LANGUAGE: &str = "python";
const DEFAULT_PYTHON_COMMAND: &str = "python3";
const DEFAULT_BUNDLE_VERSION: &str = "bundle";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeInstallSource {
    Manual,
    Bundle,
    HostImport,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
pub struct RuntimeInfo {
    pub language: String,
    pub requested_version: Option<String>,
    pub resolved_version: String,
    pub command: String,
}

#[derive(Debug, Clone)]
pub struct ResolvedRuntime {
    artifact: RuntimeArtifact,
    info: RuntimeInfo,
}

#[derive(Debug, Clone)]
pub struct RuntimeArtifact {
    binary: PathBuf,
    workspace_lowerdir: PathBuf,
}

impl RuntimeArtifact {
    pub fn binary(&self) -> &Path {
        &self.binary
    }

    pub fn workspace_lowerdir(&self) -> &Path {
        &self.workspace_lowerdir
    }
}

impl ResolvedRuntime {
    pub fn artifact(&self) -> &RuntimeArtifact {
        &self.artifact
    }

    pub fn info(&self) -> &RuntimeInfo {
        &self.info
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeManifest {
    pub language: String,
    pub version: String,
    pub command: String,
    pub binary_path: String,
    pub sha256: String,
    #[serde(default = "default_runtime_install_source")]
    pub install_source: RuntimeInstallSource,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub installed_from: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InstalledRuntime {
    pub language: String,
    pub version: String,
    pub command: String,
    pub active: bool,
    pub healthy: bool,
    pub binary: PathBuf,
    pub sha256: String,
    pub install_source: RuntimeInstallSource,
    pub installed_from: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootstrapRuntimeResult {
    pub runtime: InstalledRuntime,
    pub source: RuntimeInstallSource,
}

#[derive(Debug, Clone)]
pub struct RuntimeRegistry {
    root: PathBuf,
}

impl Default for RuntimeRegistry {
    fn default() -> Self {
        Self::from_providers(&SystemProviders)
    }
}

impl RuntimeRegistry {
    pub fn from_providers(providers: &dyn Providers) -> Self {
        Self {
            root: runtime_store_root(providers),
        }
    }

    pub fn from_root(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn resolve(
        &self,
        language: &str,
        requested_version: Option<&str>,
    ) -> Result<ResolvedRuntime, SynapseError> {
        let installed = self.verify(language, requested_version)?;
        let manifest = self.load_manifest(&installed.language, &installed.version)?;
        let runtime_dir = self.runtime_dir(&installed.language, &installed.version);
        let binary = manifest_binary_path(&runtime_dir, &manifest)?;

        Ok(ResolvedRuntime {
            artifact: RuntimeArtifact {
                binary,
                workspace_lowerdir: runtime_dir,
            },
            info: RuntimeInfo {
                language: manifest.language,
                requested_version: requested_version
                    .map(str::trim)
                    .filter(|version| !version.is_empty())
                    .map(str::to_string),
                resolved_version: manifest.version,
                command: manifest.command,
            },
        })
    }

    pub fn list(&self) -> Vec<InstalledRuntime> {
        let mut runtimes = Vec::new();
        let runtimes_root = self.runtimes_root();
        let Ok(language_dirs) = fs::read_dir(&runtimes_root) else {
            return runtimes;
        };

        for language_dir in language_dirs.flatten() {
            let language = language_dir.file_name().to_string_lossy().into_owned();
            let active_version = self.active_version(&language).ok().flatten();
            let Ok(version_dirs) = fs::read_dir(language_dir.path()) else {
                continue;
            };
            for version_dir in version_dirs.flatten() {
                let version = version_dir.file_name().to_string_lossy().into_owned();
                let Ok(manifest) = self.load_manifest(&language, &version) else {
                    continue;
                };
                let Ok(binary) =
                    manifest_binary_path(&self.runtime_dir(&language, &version), &manifest)
                else {
                    continue;
                };
                let healthy = validate_manifest_binary(&manifest, &binary).is_ok();
                runtimes.push(InstalledRuntime {
                    language: manifest.language,
                    version: manifest.version,
                    command: manifest.command,
                    active: active_version.as_deref() == Some(version.as_str()),
                    healthy,
                    binary,
                    sha256: manifest.sha256,
                    install_source: manifest.install_source,
                    installed_from: manifest.installed_from,
                });
            }
        }

        runtimes.sort_by(|left, right| {
            left.language
                .cmp(&right.language)
                .then_with(|| right.active.cmp(&left.active))
                .then_with(|| left.version.cmp(&right.version))
        });
        runtimes
    }

    pub fn verify(
        &self,
        language: &str,
        requested_version: Option<&str>,
    ) -> Result<InstalledRuntime, SynapseError> {
        let normalized = normalize_language(language)?;
        let requested_version = requested_version.map(str::trim);
        let active_version = self.active_version(&normalized)?;
        let version = match requested_version {
            Some(version) if !version.is_empty() => normalized_version(version)?,
            _ => active_version.clone().ok_or_else(|| {
                SynapseError::RuntimeUnavailable(format!(
                    "no active runtime configured for {normalized}"
                ))
            })?,
        };

        let manifest = self.load_manifest(&normalized, &version)?;
        let runtime_dir = self.runtime_dir(&normalized, &version);
        let binary = manifest_binary_path(&runtime_dir, &manifest)?;
        validate_manifest_binary(&manifest, &binary)?;

        Ok(InstalledRuntime {
            language: manifest.language,
            version: manifest.version,
            command: manifest.command,
            active: active_version.as_deref() == Some(version.as_str()),
            healthy: true,
            binary,
            sha256: manifest.sha256,
            install_source: manifest.install_source,
            installed_from: manifest.installed_from,
        })
    }

    pub fn install(
        &self,
        language: &str,
        version: &str,
        source_path: &Path,
    ) -> Result<InstalledRuntime, SynapseError> {
        self.install_with_source(language, version, source_path, RuntimeInstallSource::Manual)
    }

    fn install_with_source(
        &self,
        language: &str,
        version: &str,
        source_path: &Path,
        install_source: RuntimeInstallSource,
    ) -> Result<InstalledRuntime, SynapseError> {
        let normalized = normalize_language(language)?;
        let version = normalized_version(version)?;
        if !source_path.is_file() {
            return Err(SynapseError::RuntimeUnavailable(format!(
                "runtime source {} does not exist",
                source_path.display()
            )));
        }

        let command = command_name_for_binary(source_path)?;
        let source_record = canonicalize_path(source_path)?;
        let runtime_dir = self.runtime_dir(&normalized, &version);
        fs::create_dir_all(&runtime_dir)?;
        let stored_binary_name = command.clone();
        let stored_binary = runtime_dir.join(&stored_binary_name);
        copy_runtime_binary(source_path, &stored_binary)?;

        let manifest = RuntimeManifest {
            language: normalized.clone(),
            version: version.clone(),
            command: command.clone(),
            binary_path: stored_binary_name,
            sha256: sha256_file(&stored_binary)?,
            install_source,
            installed_from: Some(source_record.display().to_string()),
        };
        self.write_manifest(&manifest)?;

        let active = self.active_version(&normalized)?.as_deref() == Some(version.as_str());
        Ok(InstalledRuntime {
            language: manifest.language,
            version: manifest.version,
            command: manifest.command,
            active,
            healthy: true,
            binary: stored_binary,
            sha256: manifest.sha256,
            install_source: manifest.install_source,
            installed_from: manifest.installed_from,
        })
    }

    pub fn install_bundle(&self, bundle_dir: &Path) -> Result<InstalledRuntime, SynapseError> {
        if !bundle_dir.is_dir() {
            return Err(SynapseError::RuntimeUnavailable(format!(
                "runtime bundle {} does not exist",
                bundle_dir.display()
            )));
        }

        let bundle_manifest_path = bundle_dir.join("manifest.json");
        let bytes = fs::read(&bundle_manifest_path)?;
        let bundle_manifest: RuntimeManifest = serde_json::from_slice(&bytes).map_err(|error| {
            SynapseError::RuntimeUnavailable(format!(
                "runtime manifest {} is invalid: {error}",
                bundle_manifest_path.display()
            ))
        })?;

        let language = normalize_language(&bundle_manifest.language)?;
        let version = normalized_version(&bundle_manifest.version)?;
        let command = normalized_component("runtime command", &bundle_manifest.command)?;
        let bundle_binary = manifest_binary_path(bundle_dir, &bundle_manifest)?;

        validate_manifest_binary(&bundle_manifest, &bundle_binary)?;

        let source_record = canonicalize_path(bundle_dir)?;
        let runtime_dir = self.runtime_dir(&language, &version);
        fs::create_dir_all(&runtime_dir)?;
        let stored_binary = runtime_dir.join(&bundle_manifest.binary_path);
        copy_runtime_binary(&bundle_binary, &stored_binary)?;

        let stored_sha256 = sha256_file(&stored_binary)?;
        if stored_sha256 != bundle_manifest.sha256 {
            return Err(SynapseError::RuntimeUnavailable(format!(
                "runtime {}:{} failed copy integrity check",
                language, version
            )));
        }

        let manifest = RuntimeManifest {
            language: language.clone(),
            version: version.clone(),
            command,
            binary_path: bundle_manifest.binary_path,
            sha256: stored_sha256,
            install_source: RuntimeInstallSource::Bundle,
            installed_from: Some(source_record.display().to_string()),
        };
        self.write_manifest(&manifest)?;

        let active = self.active_version(&language)?.as_deref() == Some(version.as_str());
        Ok(InstalledRuntime {
            language: manifest.language,
            version: manifest.version,
            command: manifest.command,
            active,
            healthy: true,
            binary: stored_binary,
            sha256: manifest.sha256,
            install_source: manifest.install_source,
            installed_from: manifest.installed_from,
        })
    }

    pub fn activate(
        &self,
        language: &str,
        version: &str,
    ) -> Result<InstalledRuntime, SynapseError> {
        let normalized = normalize_language(language)?;
        let version = normalized_version(version)?;
        let manifest = self.load_manifest(&normalized, &version)?;
        let active_root = self.active_root();
        fs::create_dir_all(&active_root)?;
        fs::write(
            active_root.join(format!("{normalized}.txt")),
            version.as_bytes(),
        )?;

        let binary = manifest_binary_path(&self.runtime_dir(&normalized, &version), &manifest)?;
        let healthy = validate_manifest_binary(&manifest, &binary).is_ok();
        Ok(InstalledRuntime {
            language: manifest.language,
            version: manifest.version,
            command: manifest.command,
            active: true,
            healthy,
            binary,
            sha256: manifest.sha256,
            install_source: manifest.install_source,
            installed_from: manifest.installed_from,
        })
    }

    pub fn import_host_runtime(
        &self,
        providers: &dyn Providers,
        language: &str,
        version: &str,
        command: &str,
        activate: bool,
    ) -> Result<InstalledRuntime, SynapseError> {
        let Some(source_path) = find_command(providers, command) else {
            return Err(SynapseError::RuntimeUnavailable(format!(
                "{command} is not available in PATH"
            )));
        };

        let installed = self.install_with_source(
            language,
            version,
            &source_path,
            RuntimeInstallSource::HostImport,
        )?;
        if activate {
            self.activate(language, version)
        } else {
            Ok(installed)
        }
    }

    pub fn ensure_default_runtime(
        &self,
        providers: &dyn Providers,
    ) -> Result<BootstrapRuntimeResult, SynapseError> {
        if let Ok(runtime) = self.verify(SUPPORTED_PYTHON_LANGUAGE, None) {
            return Ok(BootstrapRuntimeResult {
                source: runtime.install_source.clone(),
                runtime,
            });
        }

        if let Some(bundle_dir) = default_runtime_bundle_dir(providers) {
            match self.install_bundle(&bundle_dir) {
                Ok(bundle_runtime) => {
                    let runtime =
                        self.activate(&bundle_runtime.language, &bundle_runtime.version)?;
                    return Ok(BootstrapRuntimeResult {
                        source: RuntimeInstallSource::Bundle,
                        runtime,
                    });
                }
                Err(error) if bundle_dir.join("manifest.json").is_file() => {
                    return Err(error);
                }
                Err(_) => {}
            }
        }

        let runtime = self.import_host_runtime(
            providers,
            SUPPORTED_PYTHON_LANGUAGE,
            "system",
            DEFAULT_PYTHON_COMMAND,
            true,
        )?;
        Ok(BootstrapRuntimeResult {
            source: RuntimeInstallSource::HostImport,
            runtime,
        })
    }

    pub fn bootstrap_system_defaults(&self) -> Result<(), SynapseError> {
        self.ensure_default_runtime(&SystemProviders)?;
        Ok(())
    }

    fn active_version(&self, language: &str) -> Result<Option<String>, SynapseError> {
        let marker = self.active_root().join(format!("{language}.txt"));
        match fs::read_to_string(marker) {
            Ok(version) => {
                let trimmed = version.trim();
                if trimmed.is_empty() {
                    Ok(None)
                } else {
                    Ok(Some(normalized_version(trimmed)?))
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    fn write_manifest(&self, manifest: &RuntimeManifest) -> Result<(), SynapseError> {
        let runtime_dir = self.runtime_dir(&manifest.language, &manifest.version);
        fs::create_dir_all(&runtime_dir)?;
        let bytes = serde_json::to_vec_pretty(manifest).map_err(|error| {
            SynapseError::Internal(format!("serialize runtime manifest: {error}"))
        })?;
        fs::write(runtime_dir.join("manifest.json"), bytes)?;
        Ok(())
    }

    fn load_manifest(
        &self,
        language: &str,
        version: &str,
    ) -> Result<RuntimeManifest, SynapseError> {
        let path = self.runtime_dir(language, version).join("manifest.json");
        let bytes = fs::read(&path)?;
        serde_json::from_slice(&bytes).map_err(|error| {
            SynapseError::RuntimeUnavailable(format!(
                "runtime manifest {} is invalid: {error}",
                path.display()
            ))
        })
    }

    fn runtime_dir(&self, language: &str, version: &str) -> PathBuf {
        self.runtimes_root().join(language).join(version)
    }

    fn runtimes_root(&self) -> PathBuf {
        self.root.join("runtimes")
    }

    fn active_root(&self) -> PathBuf {
        self.root.join("active")
    }
}

fn runtime_store_root(providers: &dyn Providers) -> PathBuf {
    providers
        .env_var(RUNTIME_STORE_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| providers.temp_dir().join(DEFAULT_RUNTIME_STORE_DIR))
}

fn default_runtime_bundle_dir(providers: &dyn Providers) -> Option<PathBuf> {
    if let Some(path) = providers.env_var(RUNTIME_BUNDLE_DIR_ENV) {
        return Some(PathBuf::from(path));
    }

    let cwd = std::env::current_dir().ok()?;
    let direct = cwd.join("runtime-bundles/python");
    if direct.is_dir() {
        return Some(direct);
    }

    let nested = cwd
        .join("runtime-bundles/python")
        .join(DEFAULT_BUNDLE_VERSION);
    if nested.is_dir() {
        return Some(nested);
    }

    None
}

fn default_runtime_install_source() -> RuntimeInstallSource {
    RuntimeInstallSource::Manual
}

fn normalize_language(language: &str) -> Result<String, SynapseError> {
    let normalized = normalized_component("runtime language", language)?.to_ascii_lowercase();
    match normalized.as_str() {
        "python" | "python3" => Ok(SUPPORTED_PYTHON_LANGUAGE.to_string()),
        other => Err(SynapseError::UnsupportedLanguage(other.to_string())),
    }
}

fn normalized_version(version: &str) -> Result<String, SynapseError> {
    normalized_component("runtime version", version)
}

fn normalized_component(label: &str, value: &str) -> Result<String, SynapseError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(SynapseError::InvalidInput(format!(
            "{label} cannot be empty"
        )));
    }
    if trimmed == "." || trimmed == ".." {
        return Err(SynapseError::InvalidInput(format!(
            "{label} contains an invalid path segment"
        )));
    }
    if trimmed.contains('/') || trimmed.contains('\\') {
        return Err(SynapseError::InvalidInput(format!(
            "{label} must not contain path separators"
        )));
    }
    Ok(trimmed.to_string())
}

fn command_name_for_binary(path: &Path) -> Result<String, SynapseError> {
    path.file_name()
        .and_then(OsStr::to_str)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| {
            SynapseError::InvalidInput(format!(
                "runtime source {} must end with a binary name",
                path.display()
            ))
        })
}

fn validate_manifest_binary(manifest: &RuntimeManifest, binary: &Path) -> Result<(), SynapseError> {
    if !binary.is_file() {
        return Err(SynapseError::RuntimeUnavailable(format!(
            "runtime {}:{} binary {} is missing",
            manifest.language,
            manifest.version,
            binary.display()
        )));
    }

    let actual = sha256_file(binary)?;
    if actual != manifest.sha256 {
        return Err(SynapseError::RuntimeUnavailable(format!(
            "runtime {}:{} failed integrity check",
            manifest.language, manifest.version
        )));
    }

    Ok(())
}

fn manifest_binary_path(
    runtime_dir: &Path,
    manifest: &RuntimeManifest,
) -> Result<PathBuf, SynapseError> {
    let path = PathBuf::from(&manifest.binary_path);
    if path.is_absolute() {
        return Err(SynapseError::RuntimeUnavailable(format!(
            "runtime {}:{} manifest binary_path must be relative",
            manifest.language, manifest.version
        )));
    }

    let mut components = path.components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(_)), None) => Ok(runtime_dir.join(path)),
        _ => Err(SynapseError::RuntimeUnavailable(format!(
            "runtime {}:{} manifest binary_path must be a single file name",
            manifest.language, manifest.version
        ))),
    }
}

fn canonicalize_path(path: &Path) -> Result<PathBuf, SynapseError> {
    fs::canonicalize(path).or_else(|_| Ok(path.to_path_buf()))
}

fn copy_runtime_binary(source: &Path, destination: &Path) -> Result<(), SynapseError> {
    let mut source_file = File::open(source)?;
    let metadata = source_file.metadata()?;
    if !metadata.is_file() {
        return Err(SynapseError::InvalidInput(format!(
            "runtime source {} must be a regular file",
            source.display()
        )));
    }

    let mut destination_file = File::create(destination)?;
    std::io::copy(&mut source_file, &mut destination_file)?;
    destination_file.flush()?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let mode = metadata.permissions().mode() & 0o777;
        fs::set_permissions(destination, fs::Permissions::from_mode(mode))?;
    }

    Ok(())
}

#[cfg(unix)]
fn open_file_nofollow(path: &Path) -> Result<File, SynapseError> {
    use std::os::unix::fs::OpenOptionsExt;

    fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .map_err(Into::into)
}

#[cfg(not(unix))]
fn open_file_nofollow(path: &Path) -> Result<File, SynapseError> {
    File::open(path).map_err(Into::into)
}

fn sha256_file(path: &Path) -> Result<String, SynapseError> {
    let mut file = open_file_nofollow(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 8192];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::{
        manifest_binary_path, sha256_file, RuntimeInstallSource, RuntimeManifest, RuntimeRegistry,
    };
    use crate::SynapseError;
    use std::{
        env, fs,
        path::{Path, PathBuf},
    };

    fn unique_root(prefix: &str) -> PathBuf {
        let path = env::temp_dir().join(format!(
            "{prefix}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&path);
        path
    }

    fn fake_runtime_binary(root: &PathBuf, name: &str) -> PathBuf {
        fs::create_dir_all(root).unwrap();
        let path = root.join(name);
        fs::write(&path, b"#!/bin/sh\nexit 0\n").unwrap();
        #[cfg(unix)]
        {
            let mut permissions = fs::metadata(&path).unwrap().permissions();
            use std::os::unix::fs::PermissionsExt;
            permissions.set_mode(0o755);
            fs::set_permissions(&path, permissions).unwrap();
        }
        path
    }

    fn fake_runtime_bundle(root: &Path, version: &str) -> PathBuf {
        let bundle_dir = root.join(format!("bundle-{version}"));
        fs::create_dir_all(&bundle_dir).unwrap();
        let binary = fake_runtime_binary(&bundle_dir, "python3");
        let manifest = RuntimeManifest {
            language: "python".to_string(),
            version: version.to_string(),
            command: "python3".to_string(),
            binary_path: "python3".to_string(),
            sha256: sha256_file(&binary).unwrap(),
            install_source: RuntimeInstallSource::Bundle,
            installed_from: None,
        };
        fs::write(
            bundle_dir.join("manifest.json"),
            serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();
        bundle_dir
    }

    #[test]
    fn registry_rejects_unknown_runtime() {
        let root = unique_root("synapse-runtime-registry-rejects");
        let registry = RuntimeRegistry::from_root(&root);

        let error = registry.resolve("ruby", None).unwrap_err();
        assert!(matches!(error, SynapseError::UnsupportedLanguage(language) if language == "ruby"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn registry_installs_activates_and_resolves_runtime() {
        let root = unique_root("synapse-runtime-registry-install");
        let registry = RuntimeRegistry::from_root(&root);
        let binary = fake_runtime_binary(&root.join("src"), "python3");

        let installed = registry.install("python", "3.11.9", &binary).unwrap();
        assert_eq!(installed.version, "3.11.9");
        assert!(!installed.active);

        let activated = registry.activate("python", "3.11.9").unwrap();
        assert!(activated.active);

        let resolved = registry.resolve("python", None).unwrap();
        assert_eq!(resolved.info().resolved_version, "3.11.9");
        assert!(resolved.artifact().binary().ends_with("python3"));

        let listed = registry.list();
        assert_eq!(listed.len(), 1);
        assert!(listed[0].active);
        assert!(listed[0].healthy);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn registry_requires_explicit_active_runtime_for_default_resolution() {
        let root = unique_root("synapse-runtime-registry-missing-active");
        let registry = RuntimeRegistry::from_root(&root);

        let error = registry.resolve("python", None).unwrap_err();
        assert!(
            matches!(error, SynapseError::RuntimeUnavailable(message) if message == "no active runtime configured for python")
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn registry_detects_corrupted_runtime_binary() {
        let root = unique_root("synapse-runtime-registry-corrupt");
        let registry = RuntimeRegistry::from_root(&root);
        let binary = fake_runtime_binary(&root.join("src"), "python3");

        let installed = registry.install("python", "3.12.0", &binary).unwrap();
        registry.activate("python", "3.12.0").unwrap();
        fs::write(&installed.binary, b"corrupted").unwrap();

        let error = registry.resolve("python", Some("3.12.0")).unwrap_err();
        assert!(
            matches!(error, SynapseError::RuntimeUnavailable(message) if message.contains("integrity check"))
        );

        let listed = registry.list();
        assert_eq!(listed.len(), 1);
        assert!(!listed[0].healthy);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn registry_rejects_version_with_path_separator() {
        let root = unique_root("synapse-runtime-registry-invalid-version");
        let registry = RuntimeRegistry::from_root(&root);
        let binary = fake_runtime_binary(&root.join("src"), "python3");

        let error = registry
            .install("python", "../3.11.9", &binary)
            .unwrap_err();
        assert!(
            matches!(error, SynapseError::InvalidInput(message) if message == "runtime version must not contain path separators")
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn registry_rejects_manifest_with_absolute_binary_path() {
        let root = unique_root("synapse-runtime-registry-absolute-manifest");
        let runtime_dir = root.join("runtimes/python/3.11.9");
        fs::create_dir_all(&runtime_dir).unwrap();
        let manifest = RuntimeManifest {
            language: "python".to_string(),
            version: "3.11.9".to_string(),
            command: "python3".to_string(),
            binary_path: "/usr/bin/python3".to_string(),
            sha256: "deadbeef".to_string(),
            install_source: RuntimeInstallSource::Manual,
            installed_from: None,
        };

        let error = manifest_binary_path(&runtime_dir, &manifest).unwrap_err();
        assert!(
            matches!(error, SynapseError::RuntimeUnavailable(message) if message.contains("must be relative"))
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn registry_copies_binary_into_managed_store() {
        let root = unique_root("synapse-runtime-registry-copy");
        let registry = RuntimeRegistry::from_root(&root);
        let binary = fake_runtime_binary(&root.join("src"), "python3");

        let installed = registry.install("python", "3.12.1", &binary).unwrap();
        fs::write(&binary, b"changed").unwrap();

        let resolved = registry.resolve("python", Some("3.12.1")).unwrap();
        assert_eq!(installed.binary, resolved.artifact().binary());
        assert!(installed
            .binary
            .starts_with(root.join("runtimes/python/3.12.1")));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn registry_verify_returns_active_runtime_details() {
        let root = unique_root("synapse-runtime-registry-verify");
        let registry = RuntimeRegistry::from_root(&root);
        let binary = fake_runtime_binary(&root.join("src"), "python3");

        registry.install("python", "3.12.3", &binary).unwrap();
        registry.activate("python", "3.12.3").unwrap();

        let runtime = registry.verify("python", None).unwrap();
        assert_eq!(runtime.language, "python");
        assert_eq!(runtime.version, "3.12.3");
        assert!(runtime.active);
        assert!(runtime.healthy);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn registry_installs_runtime_from_bundle() {
        let root = unique_root("synapse-runtime-registry-bundle");
        let registry = RuntimeRegistry::from_root(&root);
        let bundle = fake_runtime_bundle(&root, "3.12.4");

        let installed = registry.install_bundle(&bundle).unwrap();
        assert_eq!(installed.language, "python");
        assert_eq!(installed.version, "3.12.4");
        assert_eq!(installed.command, "python3");
        assert!(!installed.active);
        assert!(installed
            .binary
            .starts_with(root.join("runtimes/python/3.12.4")));

        registry.activate("python", "3.12.4").unwrap();
        let resolved = registry.resolve("python", None).unwrap();
        assert_eq!(resolved.info().resolved_version, "3.12.4");
        assert!(resolved.artifact().binary().ends_with("python3"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn registry_rejects_bundle_with_hash_mismatch() {
        let root = unique_root("synapse-runtime-registry-bundle-corrupt");
        let registry = RuntimeRegistry::from_root(&root);
        let bundle = fake_runtime_bundle(&root, "3.12.5");
        let manifest_path = bundle.join("manifest.json");
        let mut manifest: RuntimeManifest =
            serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
        manifest.sha256 = "deadbeef".to_string();
        fs::write(
            &manifest_path,
            serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();

        let error = registry.install_bundle(&bundle).unwrap_err();
        assert!(
            matches!(error, SynapseError::RuntimeUnavailable(message) if message.contains("integrity check"))
        );

        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn registry_installs_from_symlink_source_binary() {
        let root = unique_root("synapse-runtime-registry-symlink");
        let registry = RuntimeRegistry::from_root(&root);
        let binary = fake_runtime_binary(&root.join("src"), "python3");
        let symlink = root.join("python3-link");
        std::os::unix::fs::symlink(&binary, &symlink).unwrap();

        let installed = registry.install("python", "3.12.2", &symlink).unwrap();
        assert!(installed
            .binary
            .starts_with(root.join("runtimes/python/3.12.2")));
        assert!(registry.resolve("python", Some("3.12.2")).is_ok());

        let _ = fs::remove_dir_all(root);
    }
}
