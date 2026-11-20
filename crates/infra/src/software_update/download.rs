use futures_util::future::try_join_all;
use reqwest::{
    Client, Response, StatusCode,
    header::{CONTENT_RANGE, RANGE},
};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use template_app::{UpdateCancellation, UpdateError, UpdatePackage, UpdateProgress};

pub(super) async fn bounded_body(
    mut response: Response,
    limit: usize,
) -> Result<Vec<u8>, UpdateError> {
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| UpdateError::Network)? {
        if bytes.len().saturating_add(chunk.len()) > limit {
            return Err(UpdateError::Metadata);
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

pub(super) async fn save(
    client: &Client,
    directory: &Path,
    package: &UpdatePackage,
    cancellation: UpdateCancellation,
    progress: Arc<dyn Fn(UpdateProgress) + Send + Sync>,
) -> Result<PathBuf, UpdateError> {
    if cancellation.is_cancelled() {
        return Err(UpdateError::Cancelled);
    }
    let staging = directory
        .join(&package.version)
        .join(uuid::Uuid::new_v4().to_string());
    fs::create_dir_all(&staging).map_err(|_| UpdateError::Storage)?;
    let result = tokio::select! {
        result = transfer(client, &staging, package, &cancellation, progress) => result,
        () = cancelled(&cancellation) => Err(UpdateError::Cancelled),
    };
    if result.is_err() {
        let _ = fs::remove_dir_all(&staging);
    }
    result
}

async fn cancelled(control: &UpdateCancellation) {
    while !control.is_cancelled() {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn transfer(
    client: &Client,
    staging: &Path,
    package: &UpdatePackage,
    cancellation: &UpdateCancellation,
    progress: Arc<dyn Fn(UpdateProgress) + Send + Sync>,
) -> Result<PathBuf, UpdateError> {
    let checksums = client
        .get(&package.checksums_url)
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .map_err(|_| UpdateError::Network)?
        .error_for_status()
        .map_err(|_| UpdateError::Network)?;
    let expected = checksum(
        &bounded_body(checksums, 64 * 1024).await?,
        &package.file_name,
    )?;
    progress(UpdateProgress::Downloading {
        received: 0,
        total: package.size,
    });
    let probe = client
        .get(&package.download_url)
        .header(RANGE, "bytes=0-0")
        .send()
        .await
        .map_err(|_| UpdateError::Network)?;
    let parallel = match probe.status() {
        StatusCode::PARTIAL_CONTENT => {
            validate_range(&probe, 0, 0, package.size)?;
            if bounded_body(probe, 1).await?.len() != 1 {
                return Err(UpdateError::Integrity);
            }
            true
        }
        StatusCode::OK => false,
        _ => return Err(UpdateError::Network),
    };
    let count = if parallel { package.size.min(4) } else { 1 };
    let received = AtomicU64::new(0);
    let mut tasks = Vec::new();
    for index in 0..count {
        let start = package.size / count * index;
        let end = if index + 1 == count {
            package.size - 1
        } else {
            package.size / count * (index + 1) - 1
        };
        tasks.push(download_part(
            client,
            package,
            staging.join(format!("{index}.part")),
            parallel.then_some((start, end)),
            &received,
            &progress,
        ));
    }
    try_join_all(tasks).await?;
    progress(UpdateProgress::Verifying);
    let destination = staging.join(&package.file_name);
    assemble(staging, &destination, count, &expected, cancellation)?;
    Ok(destination)
}

async fn download_part(
    client: &Client,
    package: &UpdatePackage,
    path: PathBuf,
    range: Option<(u64, u64)>,
    received: &AtomicU64,
    progress: &Arc<dyn Fn(UpdateProgress) + Send + Sync>,
) -> Result<(), UpdateError> {
    let mut request = client.get(&package.download_url);
    if let Some((start, end)) = range {
        request = request.header(RANGE, format!("bytes={start}-{end}"));
    }
    let mut response = request.send().await.map_err(|_| UpdateError::Network)?;
    let expected = match range {
        Some((start, end)) => {
            validate_range(&response, start, end, package.size)?;
            end - start + 1
        }
        None if response.status() == StatusCode::OK => package.size,
        None => return Err(UpdateError::Network),
    };
    let mut file = File::create(path).map_err(|_| UpdateError::Storage)?;
    let mut written = 0;
    while let Some(bytes) = response.chunk().await.map_err(|_| UpdateError::Network)? {
        written += bytes.len() as u64;
        if written > expected {
            return Err(UpdateError::Integrity);
        }
        file.write_all(&bytes).map_err(|_| UpdateError::Storage)?;
        let total_received =
            received.fetch_add(bytes.len() as u64, Ordering::AcqRel) + bytes.len() as u64;
        progress(UpdateProgress::Downloading {
            received: total_received,
            total: package.size,
        });
    }
    if written != expected {
        return Err(UpdateError::Integrity);
    }
    Ok(())
}

fn validate_range(
    response: &Response,
    start: u64,
    end: u64,
    total: u64,
) -> Result<(), UpdateError> {
    let expected = format!("bytes {start}-{end}/{total}");
    if response.status() != StatusCode::PARTIAL_CONTENT
        || response
            .headers()
            .get(CONTENT_RANGE)
            .and_then(|value| value.to_str().ok())
            != Some(expected.as_str())
    {
        return Err(UpdateError::Integrity);
    }
    Ok(())
}

fn checksum(bytes: &[u8], name: &str) -> Result<String, UpdateError> {
    let text = std::str::from_utf8(bytes).map_err(|_| UpdateError::Metadata)?;
    let matches = text
        .lines()
        .filter_map(|line| {
            let (hash, file) = line.split_once(char::is_whitespace)?;
            (file.trim().trim_start_matches('*').trim_start_matches("./") == name).then_some(hash)
        })
        .collect::<Vec<_>>();
    let [hash] = matches.as_slice() else {
        return Err(UpdateError::Metadata);
    };
    if hash.len() != 64 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(UpdateError::Metadata);
    }
    Ok(hash.to_ascii_lowercase())
}

fn assemble(
    staging: &Path,
    destination: &Path,
    count: u64,
    expected: &str,
    cancellation: &UpdateCancellation,
) -> Result<(), UpdateError> {
    let temporary = staging.join("installer.part");
    let mut output = File::create(&temporary).map_err(|_| UpdateError::Storage)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    for index in 0..count {
        let path = staging.join(format!("{index}.part"));
        let mut input = File::open(&path).map_err(|_| UpdateError::Storage)?;
        loop {
            if cancellation.is_cancelled() {
                return Err(UpdateError::Cancelled);
            }
            let length = input.read(&mut buffer).map_err(|_| UpdateError::Storage)?;
            if length == 0 {
                break;
            }
            hasher.update(&buffer[..length]);
            output
                .write_all(&buffer[..length])
                .map_err(|_| UpdateError::Storage)?;
        }
        drop(input);
        fs::remove_file(path).map_err(|_| UpdateError::Storage)?;
    }
    if format!("{:x}", hasher.finalize()) != expected {
        return Err(UpdateError::Integrity);
    }
    output.sync_all().map_err(|_| UpdateError::Storage)?;
    drop(output);
    if cancellation.is_cancelled() {
        return Err(UpdateError::Cancelled);
    }
    fs::rename(temporary, destination).map_err(|_| UpdateError::Storage)?;
    Ok(())
}

#[cfg(test)]
mod tests;
