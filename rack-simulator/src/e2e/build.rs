use anyhow::{Context, Result, anyhow};
use std::{
    path::{Path, PathBuf},
    process::Stdio,
};
use tokio::{io::AsyncWriteExt as _, process::Command};

use crate::output::Output;

const ROCKY_LINUX_MIRROR: &str =
    "https://mirror.rackspace.com/rocky/10.1/BaseOS/x86_64/kickstart/images/pxeboot";

/// Ensure director VM images exist at the given paths, always running Docker build.
///
/// Docker's layer caching makes this a no-op when nothing has changed.
/// Locates the repo root (by finding the nearest `Dockerfile` walking up from cwd),
/// then runs:
///
/// ```text
/// docker build --target rack-director-e2e-export --output <dest_dir> <repo_root>
/// ```
///
/// The `rack-director-e2e-export` scratch stage exports exactly two files:
/// `vmlinuz-director` and `director-initramfs.img`.
pub async fn ensure_director_images(
    kernel: &Path,
    initramfs: &Path,
    output: &Output,
) -> Result<()> {
    run_docker_build(
        kernel,
        initramfs,
        "rack-director-e2e-export",
        "Director VM images",
        "Director kernel path has no parent directory",
        "Failed to create director image directory",
        output,
    )
    .await
}

/// Ensure agent images exist at the given paths, always running Docker build.
///
/// Docker's layer caching makes this a no-op when nothing has changed.
/// Locates the repo root (by finding the nearest `Dockerfile` walking up from cwd),
/// then runs:
///
/// ```text
/// docker build --target agent-image-export --output <dest_dir> <repo_root>
/// ```
///
/// The `agent-image-export` scratch stage exports exactly two files:
/// `vmlinuz` and `initramfs.img`.
pub async fn ensure_agent_images(kernel: &Path, initramfs: &Path, output: &Output) -> Result<()> {
    run_docker_build(
        kernel,
        initramfs,
        "agent-image-export",
        "Agent images",
        "Agent kernel path has no parent directory",
        "Failed to create agent image directory",
        output,
    )
    .await
}

/// Ensure Rocky Linux 10.1 PXE installer files are cached locally.
///
/// Downloads `vmlinuz` and `initrd.img` from the Rackspace mirror if they are
/// not already present at the given paths.  The files are large (tens of MiB)
/// so they are cached and skipped on subsequent runs.
pub async fn ensure_rocky_installer(
    kernel: &Path,
    initramfs: &Path,
    output: &Output,
) -> Result<()> {
    if kernel.exists() && initramfs.exists() {
        output.info("Rocky Linux installer already cached — skipping download");
        return Ok(());
    }

    if let Some(dir) = kernel.parent() {
        std::fs::create_dir_all(dir)
            .with_context(|| format!("Failed to create installer cache dir: {}", dir.display()))?;
    }

    output.step("Downloading Rocky Linux 10.1 PXE installer...");

    download_file(&format!("{}/vmlinuz", ROCKY_LINUX_MIRROR), kernel, output)
        .await
        .context("Failed to download Rocky Linux vmlinuz")?;

    download_file(
        &format!("{}/initrd.img", ROCKY_LINUX_MIRROR),
        initramfs,
        output,
    )
    .await
    .context("Failed to download Rocky Linux initrd.img")?;

    output.success("Rocky Linux installer downloaded");
    Ok(())
}

/// Download a URL to a local file, showing progress.
async fn download_file(url: &str, dest: &Path, output: &Output) -> Result<()> {
    output.info(&format!("Downloading {} → {}", url, dest.display()));

    let response = reqwest::get(url)
        .await
        .with_context(|| format!("HTTP request failed: {}", url))?;

    if !response.status().is_success() {
        return Err(anyhow!("HTTP {} downloading {}", response.status(), url));
    }

    let mut file = tokio::fs::File::create(dest)
        .await
        .with_context(|| format!("Failed to create file: {}", dest.display()))?;

    let bytes = response
        .bytes()
        .await
        .with_context(|| format!("Failed to read response body from {}", url))?;

    file.write_all(&bytes)
        .await
        .with_context(|| format!("Failed to write to {}", dest.display()))?;

    Ok(())
}

/// Run a `docker build` with the given target and verify output files were produced.
///
/// Always runs Docker regardless of whether files already exist — Docker's layer
/// caching ensures this is fast (a no-op) when nothing has changed.
async fn run_docker_build(
    kernel: &Path,
    initramfs: &Path,
    target: &str,
    label: &str,
    no_parent_msg: &str,
    create_dir_msg: &str,
    output: &Output,
) -> Result<()> {
    let repo_root = find_repo_root()?;
    let dest_dir = kernel
        .parent()
        .ok_or_else(|| anyhow!("{}", no_parent_msg))?;

    std::fs::create_dir_all(dest_dir)
        .with_context(|| format!("{}: {}", create_dir_msg, dest_dir.display()))?;

    output.step(&format!("{} — building via Docker...", label));
    output.info(&format!("Build context: {}", repo_root.display()));
    output.info(&format!("Output directory: {}", dest_dir.display()));

    let dockerfile = repo_root.join("docker").join("Dockerfile");
    let status = Command::new("docker")
        .args([
            "build",
            "--file",
            &dockerfile.to_string_lossy(),
            "--target",
            target,
            "--output",
            &dest_dir.to_string_lossy(),
            &repo_root.to_string_lossy(),
        ])
        .stdout(Stdio::null())
        .status()
        .await
        .context("Failed to run docker build (is Docker installed and running?)")?;

    if !status.success() {
        return Err(anyhow!(
            "docker build failed with exit code {:?}",
            status.code()
        ));
    }

    if !kernel.exists() {
        return Err(anyhow!(
            "Docker build succeeded but expected output file was not produced: {}",
            kernel.display()
        ));
    }
    if !initramfs.exists() {
        return Err(anyhow!(
            "Docker build succeeded but expected output file was not produced: {}",
            initramfs.display()
        ));
    }

    output.success(&format!("{} built successfully", label));
    Ok(())
}

/// Walk up from the current working directory to find the repo root.
///
/// The repo root is identified as the first ancestor directory that contains
/// a `docker/Dockerfile`.
fn find_repo_root() -> Result<PathBuf> {
    let mut dir = std::env::current_dir().context("Failed to get current directory")?;

    loop {
        if dir.join("docker").join("Dockerfile").exists() {
            return Ok(dir);
        }
        match dir.parent() {
            Some(parent) => dir = parent.to_path_buf(),
            None => {
                return Err(anyhow!(
                    "Could not find repo root: no docker/Dockerfile found in any parent directory"
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_repo_root_finds_dockerfile() {
        // The repo root contains a docker/Dockerfile, so running from anywhere inside
        // the repo should succeed.
        let root = find_repo_root().unwrap();
        assert!(root.join("docker").join("Dockerfile").exists());
    }
}
