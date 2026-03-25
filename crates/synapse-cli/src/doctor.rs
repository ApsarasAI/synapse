use std::{fs, io};

use synapse_core::{find_command, temp_path, Providers, SystemProviders};

#[derive(Debug)]
struct DoctorCheck {
    name: &'static str,
    ok: bool,
    detail: String,
}

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let providers = SystemProviders;
    let checks = vec![
        command_check(&providers, "python3", "required to execute Python code"),
        sandbox_tool_check(&providers),
        temp_dir_check(&providers),
    ];

    for check in &checks {
        let status = if check.ok { "ok" } else { "fail" };
        println!("[{status}] {}: {}", check.name, check.detail);
    }

    if checks.iter().all(|check| check.ok) {
        println!("Synapse doctor passed");
        Ok(())
    } else {
        Err(Box::new(io::Error::other(
            "Synapse doctor found one or more blocking issues",
        )))
    }
}

fn command_check(
    providers: &dyn Providers,
    command: &'static str,
    detail: &'static str,
) -> DoctorCheck {
    match find_command(providers, command) {
        Some(path) => DoctorCheck {
            name: command,
            ok: true,
            detail: format!("{detail} ({})", path.display()),
        },
        None => DoctorCheck {
            name: command,
            ok: false,
            detail: format!("{detail}; command not found in PATH"),
        },
    }
}

fn sandbox_tool_check(providers: &dyn Providers) -> DoctorCheck {
    if let Some(path) = find_command(providers, "bwrap") {
        return DoctorCheck {
            name: "sandbox",
            ok: true,
            detail: format!("bubblewrap available ({})", path.display()),
        };
    }

    DoctorCheck {
        name: "sandbox",
        ok: false,
        detail: "bubblewrap is required for secure Linux execution; command not found in PATH"
            .to_string(),
    }
}

fn temp_dir_check(providers: &dyn Providers) -> DoctorCheck {
    let temp_dir = temp_path(providers, "synapse-doctor");

    match fs::write(&temp_dir, b"ok") {
        Ok(()) => {
            let _ = fs::remove_file(&temp_dir);
            DoctorCheck {
                name: "tempdir",
                ok: true,
                detail: format!("temporary workspace writable ({})", temp_dir.display()),
            }
        }
        Err(error) => DoctorCheck {
            name: "tempdir",
            ok: false,
            detail: format!(
                "cannot write sandbox workspace in {}: {error}",
                temp_dir.display()
            ),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::{command_check, temp_dir_check};
    use synapse_core::SystemProviders;

    #[test]
    fn temp_dir_check_passes_in_normal_env() {
        let check = temp_dir_check(&SystemProviders);
        assert!(check.ok, "{}", check.detail);
    }

    #[test]
    fn command_check_reports_missing_binary() {
        let check = command_check(&SystemProviders, "synapse-does-not-exist", "test binary");
        assert!(!check.ok);
        assert!(check.detail.contains("not found"));
    }

    #[test]
    fn find_command_locates_python_when_available() {
        if let Some(path) = synapse_core::find_command(&SystemProviders, "python3") {
            assert!(path.ends_with("python3") || path.to_string_lossy().contains("python3"));
        }
    }
}
