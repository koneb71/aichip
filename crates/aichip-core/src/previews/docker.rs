//! Docker, through the `docker` CLI the user already has.
//!
//! Same relationship as every other tool aichip drives: spawn the official
//! binary on `PATH`, read its stdout. No daemon socket is opened here and none
//! is ever handed to a container — mounting `/var/run/docker.sock` into a
//! preview would give a branch under review the ability to start privileged
//! containers on the host, which is a host compromise, not a preview.

use std::path::Path;
use tokio::process::Command;

const DOCKER: &str = "docker";

/// Every container aichip starts carries this, so a sweep can find them all
/// and can never touch one the user started themselves.
pub const OWNER_LABEL: &str = "com.aichip.preview";

/// Is Docker here *and* running? `None` means the CLI is missing.
///
/// Checked by running it, for the same reason engine detection is: a socket
/// that exists proves nothing about a daemon that will answer.
pub async fn detect() -> Option<Result<String, String>> {
    let out = Command::new(DOCKER)
        .args(["version", "--format", "{{.Server.Version}}"])
        .output()
        .await
        .ok()?;
    if out.status.success() {
        Some(Ok(String::from_utf8_lossy(&out.stdout).trim().to_string()))
    } else {
        // Installed but the daemon is down — a completely different fix from
        // "not installed", so it is a different answer.
        Some(Err(tail(&String::from_utf8_lossy(&out.stderr), 2)))
    }
}

/// Build an image from a worktree. Returns the tail of the build log on
/// failure, which is the part that says what actually went wrong.
pub async fn build(context: &Path, tag: &str) -> Result<(), String> {
    let out = Command::new(DOCKER)
        .arg("build")
        // The branch is the build context and nothing else is.
        .args(["-t", tag])
        .args(["--label", &format!("{OWNER_LABEL}=1")])
        .arg(context)
        .output()
        .await
        .map_err(|e| format!("could not run docker: {e}"))?;
    if out.status.success() {
        return Ok(());
    }
    // Docker writes build progress to stderr, including the failing step.
    Err(tail(&String::from_utf8_lossy(&out.stderr), 20))
}

/// Start a container from a built image, published on one loopback port.
///
/// The flags here are the slice's security surface, and each one is load
/// bearing:
///
/// * `-p 127.0.0.1:host:container` — **loopback only**. Publishing on
///   `0.0.0.0` would put an unreviewed branch on every network this machine is
///   attached to, which on a laptop means the café wifi.
/// * `--memory` / `--cpus` / `--pids-limit` — a preview that spins is a
///   preview that takes the machine down with it, and the machine is also
///   running your editor and the rest of your work.
/// * `--security-opt no-new-privileges` — a setuid binary inside the image
///   cannot escalate past the container user.
/// * no `-v`, no `--privileged`, no socket. The branch gets no path out.
///
/// What this does *not* do is cut the container off from the host's network
/// namespace, which on Docker Desktop means `host.docker.internal` resolves
/// and aichip's own API is reachable from inside. That is survivable only
/// because the dashboard already refuses any request whose Host or Origin is
/// not loopback — see `reject_non_local_callers`. Worth stating plainly here,
/// because if that check is ever loosened this becomes a way to drive aichip
/// from inside a branch under review.
pub async fn run(
    image: &str,
    name: &str,
    host_port: u16,
    container_port: u16,
) -> Result<String, String> {
    let out = Command::new(DOCKER)
        .args(["run", "--detach"])
        .args(["--name", name])
        .args(["--label", &format!("{OWNER_LABEL}=1")])
        .args(["-p", &format!("127.0.0.1:{host_port}:{container_port}")])
        .args(["--memory", "2g", "--cpus", "2", "--pids-limit", "512"])
        .args(["--security-opt", "no-new-privileges"])
        // Docker restarts containers on daemon boot otherwise; a preview
        // should not outlive the reason it was started.
        .args(["--restart", "no"])
        .arg(image)
        .output()
        .await
        .map_err(|e| format!("could not run docker: {e}"))?;
    if !out.status.success() {
        return Err(tail(&String::from_utf8_lossy(&out.stderr), 8));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Is this container still up? Anything other than a clear yes is a no —
/// a container that exited two seconds after starting is not a preview.
pub async fn is_running(container: &str) -> bool {
    Command::new(DOCKER)
        .args(["inspect", "-f", "{{.State.Running}}", container])
        .output()
        .await
        .map(|o| o.status.success() && String::from_utf8_lossy(&o.stdout).trim() == "true")
        .unwrap_or(false)
}

/// The tail of a container's own output. What you want when a preview builds
/// fine and then serves nothing.
pub async fn logs(container: &str, lines: u32) -> String {
    Command::new(DOCKER)
        .args(["logs", "--tail", &lines.to_string(), container])
        .output()
        .await
        .map(|o| {
            let mut all = String::from_utf8_lossy(&o.stdout).into_owned();
            all.push_str(&String::from_utf8_lossy(&o.stderr));
            all.trim().to_string()
        })
        .unwrap_or_default()
}

/// Stop and remove. Best effort by design: a container the user already
/// removed by hand must not leave the row stuck at "running" forever.
pub async fn remove(container: &str) {
    let _ = Command::new(DOCKER)
        .args(["rm", "--force", "--volumes", container])
        .output()
        .await;
}

/// Does this image still exist? Waking a preview depends on the answer, and
/// a `docker rmi` run by the user is not something aichip is told about.
pub async fn image_exists(tag: &str) -> bool {
    Command::new(DOCKER)
        .args(["image", "inspect", tag])
        .output()
        .await
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Total bytes of every preview image on disk.
///
/// Asked of Docker rather than tracked as we go: images are shared, layers are
/// deduplicated, and a number aichip computed itself would be confidently wrong
/// in a way the user could not check.
pub async fn image_disk_bytes() -> u64 {
    Command::new(DOCKER)
        .args([
            "images",
            "--filter",
            &format!("label={OWNER_LABEL}=1"),
            "--format",
            "{{.Size}}",
            "--no-trunc",
        ])
        .output()
        .await
        .ok()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .filter_map(|l| parse_size(l.trim()))
                .sum()
        })
        .unwrap_or(0)
}

/// Docker prints sizes as "123MB" / "1.2GB". Parsed rather than asked for in
/// bytes because `docker images` has no such format.
fn parse_size(text: &str) -> Option<u64> {
    let cut = text.find(|c: char| c.is_ascii_alphabetic())?;
    let (number, unit) = text.split_at(cut);
    let number: f64 = number.trim().parse().ok()?;
    let scale = match unit.trim().to_ascii_uppercase().as_str() {
        "B" => 1.0,
        "KB" => 1e3,
        "MB" => 1e6,
        "GB" => 1e9,
        "TB" => 1e12,
        _ => return None,
    };
    Some((number * scale) as u64)
}

/// Remove a built image once nothing points at it. Ignores "still in use".
pub async fn remove_image(tag: &str) {
    let _ = Command::new(DOCKER).args(["rmi", "--force", tag]).output().await;
}

/// Every preview container Docker currently knows about, by name.
///
/// Reads Docker rather than the database on purpose: this is the half of
/// reconciliation that answers "what is actually running", and asking the
/// database that question is how orphans survive a restart.
pub async fn list_owned() -> Vec<String> {
    Command::new(DOCKER)
        .args([
            "ps",
            "--all",
            "--filter",
            &format!("label={OWNER_LABEL}=1"),
            "--format",
            "{{.Names}}",
        ])
        .output()
        .await
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// The last `n` non-empty lines, which is the part of a build log worth
/// showing. Whole logs are megabytes and the failure is always at the end.
fn tail(text: &str, n: usize) -> String {
    let lines: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    let start = lines.len().saturating_sub(n);
    lines[start..].join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_dockers_own_size_spelling() {
        assert_eq!(parse_size("123MB"), Some(123_000_000));
        assert_eq!(parse_size("1.2GB"), Some(1_200_000_000));
        assert_eq!(parse_size("512B"), Some(512));
        // Docker sometimes pads; a number with no unit is not a size.
        assert_eq!(parse_size(" 4.5 GB "), Some(4_500_000_000));
        assert_eq!(parse_size("123"), None);
        assert_eq!(parse_size(""), None);
        assert_eq!(parse_size("N/A"), None);
    }

    #[test]
    fn tail_keeps_the_end_which_is_where_the_error_is() {
        assert_eq!(tail("a\nb\nc", 2), "b\nc");
        // Fewer lines than asked for is not an error.
        assert_eq!(tail("only", 5), "only");
        assert_eq!(tail("", 5), "");
        // Docker's progress output is full of blank lines; they carry nothing
        // and would otherwise crowd out the actual message.
        assert_eq!(tail("a\n\n\nb\n\n", 2), "a\nb");
    }
}
