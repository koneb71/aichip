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
/// `dockerfile` is `None` for a branch that has its own, and `Some` for an
/// approved recipe — which is fed on stdin rather than written into the
/// worktree, so a preview never adds a file to the diff under review.
pub async fn build(
    context: &Path,
    tag: &str,
    dockerfile: Option<&str>,
    // Everything docker prints is kept here, so a failed build can be read
    // past the last twenty lines the card has room for.
    log: &Path,
) -> Result<(), String> {
    let mut cmd = Command::new(DOCKER);
    cmd.arg("build")
        // The branch is the build context and nothing else is.
        .args(["-t", tag])
        .args(["--label", &format!("{OWNER_LABEL}=1")]);
    if dockerfile.is_some() {
        cmd.args(["-f", "-"]);
    }
    cmd.arg(context);

    let out = match dockerfile {
        None => cmd.output().await,
        Some(text) => {
            use tokio::io::AsyncWriteExt;
            cmd.stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped());
            let mut child = cmd.spawn().map_err(|e| format!("could not run docker: {e}"))?;
            // Dropped after the write so docker sees EOF and starts building.
            {
                let mut stdin = child.stdin.take().ok_or("docker took no stdin")?;
                stdin
                    .write_all(text.as_bytes())
                    .await
                    .map_err(|e| format!("could not hand docker the recipe: {e}"))?;
            }
            child.wait_with_output().await
        }
    }
    .map_err(|e| format!("could not run docker: {e}"))?;
    // Written whether it succeeded or not: a build that worked but produced
    // warnings is worth being able to read too.
    let full = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let _ = tokio::fs::write(log, &full).await;

    if out.status.success() {
        return Ok(());
    }
    // The tail is what the card shows; the file above is the whole thing.
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

/// Bring a stack up, namespaced so it cannot touch anything the user runs.
///
/// `--project-name` is the safety property. Compose prefixes networks, volumes
/// and container names with it, so a preview's `app` network and `backend-data`
/// volume are its own — it cannot join the user's real ones, and `down -v`
/// later cannot delete them.
///
/// `--project-directory` points at the branch, because the rewritten file lives
/// in a temp path and every `build.context` and bind mount in it is relative to
/// the original.
pub async fn compose_up(
    file: &Path,
    project_dir: &Path,
    project: &str,
    log: &Path,
) -> Result<(), String> {
    let out = Command::new(DOCKER)
        .args(["compose", "-f"])
        .arg(file)
        .arg("--project-directory")
        .arg(project_dir)
        .args(["-p", project])
        // The one loopback binding is already in the file — `compose up` has
        // no `--publish`, which is why the file is rewritten rather than
        // overridden. Named services are *not* passed: a stack is the point,
        // so its dependencies come up too.
        .args(["up", "--detach", "--build", "--remove-orphans", "--wait"])
        .output()
        .await
        .map_err(|e| format!("could not run docker compose: {e}"))?;
    let full = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let _ = tokio::fs::write(log, &full).await;
    if out.status.success() {
        return Ok(());
    }
    Err(tail(&String::from_utf8_lossy(&out.stderr), 20))
}

/// Every service's output in a stack, prefixed with which service said it.
pub async fn compose_logs(project: &str, lines: u32) -> String {
    Command::new(DOCKER)
        .args(["compose", "-p", project, "logs", "--tail", &lines.to_string()])
        .output()
        .await
        .map(|o| {
            let mut all = String::from_utf8_lossy(&o.stdout).into_owned();
            all.push_str(&String::from_utf8_lossy(&o.stderr));
            all.trim().to_string()
        })
        .unwrap_or_default()
}

/// Take a stack down knowing only its project name.
///
/// Compose tracks a running project by name, so the file it came up with is not
/// needed — which matters for a stack nothing in the database claims any more,
/// where the rewritten file may already be gone.
pub async fn compose_down_project(project: &str) {
    let _ = Command::new(DOCKER)
        .args(["compose", "-p", project, "down", "--volumes", "--remove-orphans"])
        .output()
        .await;
}

/// Is this stack still up?
///
/// Asked of compose rather than of a container name: compose names containers
/// `<project>-<service>-1`, so looking for `aichip-preview-<id>` finds nothing
/// and would declare a perfectly healthy stack dead. That is not hypothetical —
/// it is what the first version of this did.
pub async fn compose_running(project: &str) -> bool {
    Command::new(DOCKER)
        .args(["compose", "-p", project, "ps", "--status", "running", "-q"])
        .output()
        .await
        .map(|o| o.status.success() && !String::from_utf8_lossy(&o.stdout).trim().is_empty())
        .unwrap_or(false)
}

/// Take a stack down, and its networks and volumes with it.
///
/// `--volumes` is safe *because* of the project namespace: it removes only
/// volumes compose created under this preview's name, never the user's.
pub async fn compose_down(file: &Path, project_dir: &Path, project: &str) {
    let _ = Command::new(DOCKER)
        .args(["compose", "-f"])
        .arg(file)
        .arg("--project-directory")
        .arg(project_dir)
        .args(["-p", project])
        .args(["down", "--volumes", "--remove-orphans"])
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
/// Every image aichip built for one preview, by its own tag prefix.
///
/// A stack builds one image per service, so a preview's images are
/// `aichip-preview-<short>-web`, `-api`, and so on. Found by prefix rather than
/// by label because the prefix is the thing that *guarantees* they are ours —
/// see `compose::namespace_built_images`. The label is on them too, but a label
/// is a claim about an image and the name is the image.
pub async fn images_for(prefix: &str) -> Vec<String> {
    Command::new(DOCKER)
        .args(["images", "--format", "{{.Repository}}:{{.Tag}}"])
        .output()
        .await
        .ok()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .map(str::trim)
                .filter(|l| l.starts_with(prefix))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

pub async fn remove_image(tag: &str) {
    let _ = Command::new(DOCKER).args(["rmi", "--force", tag]).output().await;
}

/// Every preview *stack* compose currently knows about, by project name.
///
/// Compose containers carry no label of ours, so the container sweep cannot see
/// them at all. Found by project-name prefix instead — which is safe for the
/// same reason the label is: nothing the user starts is called this.
pub async fn list_owned_stacks() -> Vec<String> {
    Command::new(DOCKER)
        .args(["compose", "ls", "--all", "--format", "json"])
        .output()
        .await
        .ok()
        .and_then(|o| serde_json::from_slice::<serde_json::Value>(&o.stdout).ok())
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default()
        .iter()
        .filter_map(|e| e.get("Name")?.as_str().map(str::to_string))
        .filter(|n| n.starts_with("aichip-preview-"))
        .collect()
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
