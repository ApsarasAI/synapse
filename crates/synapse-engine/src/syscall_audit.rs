use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::{sandbox_audit_event, SandboxAuditEvent, SandboxAuditKind};

pub fn collect_trace_audit_events(
    request_id: &str,
    tenant_id: Option<&str>,
    trace_prefix: &Path,
) -> Vec<SandboxAuditEvent> {
    let Some(parent) = trace_prefix.parent() else {
        return Vec::new();
    };
    let Some(prefix_name) = trace_prefix.file_name().and_then(|name| name.to_str()) else {
        return Vec::new();
    };

    let mut paths: Vec<PathBuf> = match fs::read_dir(parent) {
        Ok(entries) => entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .map(|name| name == prefix_name || name.starts_with(&format!("{prefix_name}.")))
                    .unwrap_or(false)
            })
            .collect(),
        Err(_) => return Vec::new(),
    };
    paths.sort();

    let mut events = Vec::new();
    for path in paths {
        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };
        for line in content.lines() {
            if let Some(event) = parse_strace_line(request_id, tenant_id, line) {
                events.push(event);
            }
        }
        let _ = fs::remove_file(path);
    }
    events
}

fn parse_strace_line(
    request_id: &str,
    tenant_id: Option<&str>,
    line: &str,
) -> Option<SandboxAuditEvent> {
    let _ = (request_id, tenant_id);
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }

    if starts_with_any(trimmed, &["open(", "openat(", "access(", "stat(", "lstat("]) {
        let path = first_quoted(trimmed)?;
        if !interesting_file_path(&path) {
            return None;
        }
        let action = if trimmed.contains("O_WRONLY") || trimmed.contains("O_RDWR") {
            "write"
        } else {
            "read"
        };
        return Some(syscall_event(
            request_id,
            tenant_id,
            SandboxAuditKind::FileAccess,
            format!("sandbox file {action}"),
            &[
                ("syscall", syscall_name(trimmed)),
                ("action", action.to_string()),
                ("path", path),
            ],
        ));
    }

    if starts_with_any(trimmed, &["socket(", "connect(", "sendto("]) {
        if trimmed.starts_with("socket(")
            && !(trimmed.contains("AF_INET") || trimmed.contains("AF_INET6"))
        {
            return None;
        }
        let target = extract_network_target(trimmed);
        return Some(syscall_event(
            request_id,
            tenant_id,
            SandboxAuditKind::NetworkAttempt,
            "sandbox network attempt".to_string(),
            &[("syscall", syscall_name(trimmed)), ("target", target)],
        ));
    }

    if starts_with_any(
        trimmed,
        &["execve(", "clone(", "clone3(", "fork(", "vfork("],
    ) {
        let mut fields = vec![("syscall", syscall_name(trimmed))];
        if let Some(path) = first_quoted(trimmed) {
            if ignored_process_path(&path) {
                return None;
            }
            fields.push(("path", path));
        }
        return Some(syscall_event(
            request_id,
            tenant_id,
            SandboxAuditKind::ProcessSpawn,
            "sandbox process spawn attempt".to_string(),
            &fields,
        ));
    }

    None
}

fn syscall_event(
    request_id: &str,
    tenant_id: Option<&str>,
    kind: SandboxAuditKind,
    message: String,
    fields: &[(&str, String)],
) -> SandboxAuditEvent {
    let _ = (request_id, tenant_id);
    let mut fields_map = std::collections::BTreeMap::new();
    for (key, value) in fields {
        if !value.is_empty() {
            fields_map.insert((*key).to_string(), value.clone());
        }
    }
    sandbox_audit_event(kind, message, fields_map)
}

fn starts_with_any(value: &str, prefixes: &[&str]) -> bool {
    prefixes.iter().any(|prefix| value.starts_with(prefix))
}

fn interesting_file_path(path: &str) -> bool {
    path.starts_with("/workspace")
        || path.starts_with("/tmp")
        || matches!(path, "/etc/passwd" | "/etc/shadow")
}

fn ignored_process_path(path: &str) -> bool {
    path.contains("/bwrap") || path.contains("/strace")
}

fn syscall_name(line: &str) -> String {
    line.split('(').next().unwrap_or_default().to_string()
}

fn first_quoted(line: &str) -> Option<String> {
    let start = line.find('"')?;
    let rest = &line[start + 1..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn extract_network_target(line: &str) -> String {
    if let Some(ip_start) = line.find("inet_addr(\"") {
        let rest = &line[ip_start + "inet_addr(\"".len()..];
        if let Some(ip_end) = rest.find('"') {
            let ip = &rest[..ip_end];
            if let Some(port_start) = line.find("htons(") {
                let port_rest = &line[port_start + "htons(".len()..];
                if let Some(port_end) = port_rest.find(')') {
                    return format!("{ip}:{}", &port_rest[..port_end]);
                }
            }
            return ip.to_string();
        }
    }

    if let Some(path) = first_quoted(line) {
        return path;
    }

    line.to_string()
}

#[cfg(test)]
mod tests {
    use super::parse_strace_line;
    use crate::SandboxAuditKind;

    #[test]
    fn parses_file_access_lines() {
        let event = parse_strace_line(
            "req-1",
            Some("tenant-a"),
            "openat(AT_FDCWD, \"/workspace/main.py\", O_RDONLY|O_CLOEXEC) = 3",
        )
        .unwrap();
        assert_eq!(event.kind, SandboxAuditKind::FileAccess);
        assert_eq!(event.fields["path"], "/workspace/main.py");
        assert_eq!(event.fields["action"], "read");
    }

    #[test]
    fn parses_network_lines() {
        let event = parse_strace_line(
            "req-1",
            Some("tenant-a"),
            "connect(3, {sa_family=AF_INET, sin_port=htons(80), sin_addr=inet_addr(\"1.1.1.1\")}, 16) = -1 EPERM (Operation not permitted)",
        )
        .unwrap();
        assert_eq!(event.kind, SandboxAuditKind::NetworkAttempt);
        assert_eq!(event.fields["target"], "1.1.1.1:80");
    }

    #[test]
    fn skips_wrapper_process_exec_events() {
        let event = parse_strace_line(
            "req-1",
            Some("tenant-a"),
            "execve(\"/usr/bin/bwrap\", [\"bwrap\"], 0x0) = 0",
        );
        assert!(event.is_none());
    }

    #[test]
    fn skips_non_inet_socket_noise() {
        let event = parse_strace_line(
            "req-1",
            Some("tenant-a"),
            "socket(AF_UNIX, SOCK_STREAM|SOCK_CLOEXEC|SOCK_NONBLOCK, 0) = -1 EPERM (Operation not permitted)",
        );
        assert!(event.is_none());
    }

    #[test]
    fn skips_runtime_loader_file_access_noise() {
        let event = parse_strace_line(
            "req-1",
            Some("tenant-a"),
            "openat(AT_FDCWD, \"/etc/ld.so.cache\", O_RDONLY|O_CLOEXEC) = 3",
        );
        assert!(event.is_none());
    }

    #[test]
    fn parses_exec_lines() {
        let event = parse_strace_line(
            "req-1",
            Some("tenant-a"),
            "execve(\"/bin/sh\", [\"sh\"], 0x0) = -1 EPERM (Operation not permitted)",
        )
        .unwrap();
        assert_eq!(event.kind, SandboxAuditKind::ProcessSpawn);
        assert_eq!(event.fields["path"], "/bin/sh");
    }
}
