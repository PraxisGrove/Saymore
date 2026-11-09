#![allow(clippy::panic_in_result_fn)]

use httpmock::{Method::GET, MockServer};
use sha2::{Digest, Sha256};

use super::verification::VERIFICATION_MARKER;
use super::*;

#[tokio::test]
async fn resumes_a_partial_file_and_atomically_activates_it()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start_async().await;
    let download = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/model.bin")
                .header("range", "bytes=2-");
            then.status(206).body("c");
        })
        .await;
    let directory = tempfile::tempdir()?;
    let installer = VerifiedModelInstaller::with_manifest(
        directory.path().join("models"),
        server.base_url(),
        vec![test_artifact("model.bin", b"abc")],
    )?;
    let staging = installer.staging_directory();
    fs::create_dir_all(&staging)?;
    fs::write(staging.join("model.bin.part"), b"ab")?;

    let installed = installer.install(Arc::new(|_| {})).await?;

    download.assert_async().await;
    assert_eq!(b"abc", fs::read(installed.join("model.bin"))?.as_slice());
    assert!(installed.join(VERIFICATION_MARKER).is_file());
    assert!(installer.is_installed());
    fs::write(installed.join("model.bin"), b"abcd")?;
    assert!(!installer.is_installed());
    assert!(!staging.exists());
    Ok(())
}

#[tokio::test]
async fn a_new_installer_instance_resumes_the_persisted_partial_file()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start_async().await;
    let download = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/model.bin")
                .header("range", "bytes=2-");
            then.status(206).body("c");
        })
        .await;
    let directory = tempfile::tempdir()?;
    let models_root = directory.path().join("models");
    let first_process = VerifiedModelInstaller::with_manifest(
        models_root.clone(),
        server.base_url(),
        vec![test_artifact("model.bin", b"abc")],
    )?;
    fs::create_dir_all(first_process.staging_directory())?;
    fs::write(
        first_process.staging_directory().join("model.bin.part"),
        b"ab",
    )?;
    drop(first_process);

    let restarted_process = VerifiedModelInstaller::with_manifest(
        models_root,
        server.base_url(),
        vec![test_artifact("model.bin", b"abc")],
    )?;
    let installed = restarted_process.install(Arc::new(|_| {})).await?;

    download.assert_async().await;
    assert_eq!(b"abc", fs::read(installed.join("model.bin"))?.as_slice());
    assert!(restarted_process.is_installed());
    Ok(())
}

#[tokio::test]
async fn restarts_when_the_server_ignores_the_range_header()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start_async().await;
    server
        .mock_async(|when, then| {
            when.method(GET).path("/model.bin");
            then.status(200).body("abc");
        })
        .await;
    let directory = tempfile::tempdir()?;
    let installer = VerifiedModelInstaller::with_manifest(
        directory.path().join("models"),
        server.base_url(),
        vec![test_artifact("model.bin", b"abc")],
    )?;
    let staging = installer.staging_directory();
    fs::create_dir_all(&staging)?;
    fs::write(staging.join("model.bin.part"), b"ab")?;

    let installed = installer.install(Arc::new(|_| {})).await?;

    assert_eq!(b"abc", fs::read(installed.join("model.bin"))?.as_slice());
    Ok(())
}

#[tokio::test]
async fn pause_preserves_the_partial_download_for_a_later_resume()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start_async().await;
    server
        .mock_async(|when, then| {
            when.method(GET).path("/model.bin");
            then.status(200).body("abc");
        })
        .await;
    let directory = tempfile::tempdir()?;
    let installer = VerifiedModelInstaller::with_manifest(
        directory.path().join("models"),
        server.base_url(),
        vec![test_artifact("model.bin", b"abc")],
    )?;
    let control = ModelInstallControl::default();
    let pause = control.clone();

    let result = installer
        .install_with_control(Arc::new(move |_| pause.pause()), control)
        .await;

    assert!(matches!(
        result,
        Err(ModelInstallError::Interrupted(
            ModelInstallInterruption::Paused
        ))
    ));
    assert!(
        installer
            .staging_directory()
            .join("model.bin.part")
            .is_file()
    );
    assert_eq!(
        ModelDownloadProgress {
            downloaded_bytes: 3,
            total_bytes: 3,
        },
        installer.partial_download_progress()?
    );
    assert!(!installer.model_directory().exists());

    let installed = installer.install(Arc::new(|_| {})).await?;
    assert_eq!(b"abc", fs::read(installed.join("model.bin"))?.as_slice());
    Ok(())
}

#[tokio::test]
async fn cancelled_download_can_discard_its_partial_files() -> Result<(), Box<dyn std::error::Error>>
{
    let server = MockServer::start_async().await;
    server
        .mock_async(|when, then| {
            when.method(GET).path("/model.bin");
            then.status(200).body("abc");
        })
        .await;
    let directory = tempfile::tempdir()?;
    let installer = VerifiedModelInstaller::with_manifest(
        directory.path().join("models"),
        server.base_url(),
        vec![test_artifact("model.bin", b"abc")],
    )?;
    let control = ModelInstallControl::default();
    let cancel = control.clone();

    let result = installer
        .install_with_control(Arc::new(move |_| cancel.cancel()), control)
        .await;

    assert!(matches!(
        result,
        Err(ModelInstallError::Interrupted(
            ModelInstallInterruption::Cancelled
        ))
    ));
    installer.discard_partial_download()?;
    assert!(!installer.staging_directory().exists());
    assert!(!installer.model_directory().exists());
    Ok(())
}

#[tokio::test]
async fn maps_a_remote_subdirectory_to_a_nested_local_layout()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start_async().await;
    server
        .mock_async(|when, then| {
            when.method(GET).path("/remote/model.bin");
            then.status(200).body("abc");
        })
        .await;
    let directory = tempfile::tempdir()?;
    let mut expected = test_artifact("unused", b"abc");
    expected.remote_path = "remote/model.bin".to_owned();
    expected.local_path = "tokenizer/model.bin".to_owned();
    let installer = VerifiedModelInstaller::with_manifest(
        directory.path().join("models"),
        server.base_url(),
        vec![expected],
    )?;

    let installed = installer.install(Arc::new(|_| {})).await?;

    assert_eq!(
        b"abc",
        fs::read(installed.join("tokenizer/model.bin"))?.as_slice()
    );
    assert!(installer.is_installed());
    Ok(())
}

#[tokio::test]
async fn extracts_only_the_pinned_file_from_a_tar_bzip2_archive()
-> Result<(), Box<dyn std::error::Error>> {
    let model = b"punctuation-model";
    let archive = test_archive(&[
        ("bundle/model.int8.onnx", model.as_slice()),
        ("bundle/README.md", b"not installed".as_slice()),
    ])?;
    let server = MockServer::start_async().await;
    server
        .mock_async(|when, then| {
            when.method(GET).path("/punctuation.tar.bz2");
            then.status(200).body(archive.clone());
        })
        .await;
    let directory = tempfile::tempdir()?;
    let installer = VerifiedModelInstaller::with_archive_manifest(
        directory.path().join("models"),
        server.base_url(),
        test_artifact("punctuation.tar.bz2", &archive),
        "bundle/model.int8.onnx".to_owned(),
        test_artifact("model.int8.onnx", model),
    )?;

    let installed = installer.install(Arc::new(|_| {})).await?;

    assert_eq!(
        model,
        fs::read(installed.join("model.int8.onnx"))?.as_slice()
    );
    assert!(!installed.join("README.md").exists());
    assert!(!installed.join("punctuation.tar.bz2").exists());
    assert!(!installer.staging_directory().exists());
    assert!(installer.is_installed());
    Ok(())
}

#[tokio::test]
async fn rejects_a_wrong_hash_without_activating_the_model()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start_async().await;
    let download = server
        .mock_async(|when, then| {
            when.method(GET).path("/model.bin");
            then.status(200).body("bad");
        })
        .await;
    let directory = tempfile::tempdir()?;
    let mut expected = test_artifact("model.bin", b"bad");
    expected.sha256 = "00".repeat(32);
    let installer = VerifiedModelInstaller::with_manifest(
        directory.path().join("models"),
        server.base_url(),
        vec![expected],
    )?;

    let result = installer.install(Arc::new(|_| {})).await;

    assert!(matches!(result, Err(ModelInstallError::Integrity(_))));
    assert_eq!(4, download.calls_async().await);
    assert!(!installer.model_directory().exists());
    Ok(())
}

#[tokio::test]
async fn recovers_after_a_network_outage_without_a_false_installed_state()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start_async().await;
    let outage = server
        .mock_async(|when, then| {
            when.method(GET).path("/model.bin");
            then.status(503);
        })
        .await;
    let directory = tempfile::tempdir()?;
    let installer = VerifiedModelInstaller::with_manifest(
        directory.path().join("models"),
        server.base_url(),
        vec![test_artifact("model.bin", b"abc")],
    )?;

    let failure = installer.install(Arc::new(|_| {})).await;

    assert!(matches!(failure, Err(ModelInstallError::Download(_))));
    assert_eq!(4, outage.calls_async().await);
    assert!(!installer.is_installed());
    assert!(!installer.model_directory().exists());
    outage.delete_async().await;
    server
        .mock_async(|when, then| {
            when.method(GET).path("/model.bin");
            then.status(200).body("abc");
        })
        .await;

    let installed = installer.install(Arc::new(|_| {})).await?;

    assert_eq!(b"abc", fs::read(installed.join("model.bin"))?.as_slice());
    assert!(installer.is_installed());
    Ok(())
}

#[test]
fn rejects_an_install_that_exceeds_available_disk_space() -> Result<(), Box<dyn std::error::Error>>
{
    let directory = tempfile::tempdir()?;
    let mut artifact = test_artifact("model.bin", b"abc");
    artifact.bytes = u64::MAX;
    let installer = VerifiedModelInstaller::with_manifest(
        directory.path().join("models"),
        "https://example.invalid".to_owned(),
        vec![artifact],
    )?;

    let result = installer.ensure_install_space();

    assert!(matches!(
        result,
        Err(ModelInstallError::InsufficientSpace {
            required_bytes: u64::MAX,
            ..
        })
    ));
    assert!(!installer.model_directory().exists());
    Ok(())
}

#[tokio::test]
async fn installs_atomically_under_a_long_unicode_models_path()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start_async().await;
    server
        .mock_async(|when, then| {
            when.method(GET).path("/model.bin");
            then.status(200).body("abc");
        })
        .await;
    let directory = tempfile::tempdir()?;
    let mut models_root = directory.path().to_path_buf();
    for index in 0..8 {
        models_root.push(format!("中文模型目录-{index}-{}", "a".repeat(24)));
    }
    assert!(models_root.as_os_str().len() > 260);
    let installer = VerifiedModelInstaller::with_manifest(
        models_root,
        server.base_url(),
        vec![test_artifact("model.bin", b"abc")],
    )?;

    let installed = installer.install(Arc::new(|_| {})).await?;

    assert_eq!(b"abc", fs::read(installed.join("model.bin"))?.as_slice());
    assert!(installer.is_installed());
    Ok(())
}

#[test]
fn same_size_corruption_is_not_treated_as_installed() -> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let installer = VerifiedModelInstaller::with_manifest(
        directory.path().join("models"),
        "https://example.invalid".to_owned(),
        vec![test_artifact("model.bin", b"abc")],
    )?;
    let model_directory = installer.model_directory();
    fs::create_dir_all(&model_directory)?;
    fs::write(model_directory.join("model.bin"), b"bad")?;

    assert!(!installer.is_installed());
    Ok(())
}

#[test]
fn whisper_download_request_pins_the_ggml_artifact_and_resume_offset()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let installer = VerifiedModelInstaller::whisper_large_v3_turbo(directory.path().to_path_buf())?;
    let [artifact] = installer.manifest.downloads.as_slice() else {
        return Err("Whisper must download exactly one GGML file".into());
    };
    let request = installer.artifact_request(artifact, 123).build()?;

    assert_eq!(
        "https://huggingface.co/ggerganov/whisper.cpp/resolve/5359861c739e955e79d9a303bcbc70fb988958b1/ggml-large-v3-turbo-q8_0.bin",
        request.url().as_str()
    );
    assert_eq!("bytes=123-", request.headers()[RANGE]);
    assert_eq!(874_188_075, installer.download_size_bytes());
    assert_eq!(
        "317eb69c11673c9de1e1f0d459b253999804ec71ac4c23c17ecf5fbe24e259a1",
        artifact.sha256
    );
    Ok(())
}

#[tokio::test]
async fn whisper_reinstalls_ggml_without_reusing_or_removing_the_onnx_installation()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let root = directory.path().join("models");
    let legacy = ModelManifest::direct(
        "whisper-large-v3-turbo-int8",
        "2ca6ff69fc878651b770880507669577ac41c2ff",
        "https://example.invalid",
        vec![
            test_artifact("turbo-encoder.int8.onnx", b"encoder"),
            test_artifact("turbo-decoder.int8.onnx", b"decoder"),
            test_artifact("turbo-tokens.txt", b"tokens"),
        ],
    );
    let old_installer = VerifiedModelInstaller::new(root.clone(), legacy)?;
    let old_path = old_installer.model_directory();
    fs::create_dir_all(&old_path)?;
    for (artifact, contents) in old_installer.manifest.artifacts.iter().zip([
        b"encoder".as_slice(),
        b"decoder".as_slice(),
        b"tokens".as_slice(),
    ]) {
        fs::write(old_path.join(&artifact.local_path), contents)?;
    }
    assert!(old_installer.is_installed());
    // A valid old marker must not establish availability of the new artifact.
    write_verification_marker(&old_path, &old_installer.manifest)?;
    let old_marker = fs::read(old_path.join(VERIFICATION_MARKER))?;
    fs::create_dir_all(old_installer.staging_directory())?;
    fs::write(
        old_installer
            .staging_directory()
            .join("turbo-encoder.int8.onnx.part"),
        b"partial",
    )?;

    let mut installer = VerifiedModelInstaller::whisper_large_v3_turbo(root)?;
    assert_ne!(old_path, installer.model_directory());
    assert!(!installer.is_installed());
    assert_eq!(0, installer.partial_download_progress()?.downloaded_bytes);
    assert_eq!(0, installer.installed_size_bytes()?);

    let server = MockServer::start_async().await;
    let download = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/ggml-large-v3-turbo-q8_0.bin")
                .header("range", "bytes=2-");
            then.status(206).body("c");
        })
        .await;
    installer.manifest.base_url = server.base_url();
    // Substitute only model bytes to exercise the real layout without a large download.
    let artifact = test_artifact("ggml-large-v3-turbo-q8_0.bin", b"abc");
    installer.manifest.downloads = vec![artifact.clone()];
    installer.manifest.artifacts = vec![artifact];
    fs::create_dir_all(installer.model_directory())?;
    fs::write(
        installer.model_directory().join(VERIFICATION_MARKER),
        &old_marker,
    )?;
    assert!(!installer.is_installed());
    fs::create_dir_all(installer.staging_directory())?;
    fs::write(
        installer
            .staging_directory()
            .join("ggml-large-v3-turbo-q8_0.bin.part"),
        b"ab",
    )?;

    let installed = installer.install(Arc::new(|_| {})).await?;

    download.assert_async().await;
    assert_eq!(
        b"abc",
        fs::read(installed.join("ggml-large-v3-turbo-q8_0.bin"))?.as_slice()
    );
    assert!(installer.is_installed());
    assert!(!installed.join("turbo-encoder.int8.onnx").exists());
    assert_eq!(old_marker, fs::read(old_path.join(VERIFICATION_MARKER))?);
    assert!(
        old_installer
            .staging_directory()
            .join("turbo-encoder.int8.onnx.part")
            .exists()
    );
    installer.remove()?;
    assert!(!installer.is_installed());
    assert!(old_path.join("turbo-encoder.int8.onnx").exists());
    Ok(())
}

fn test_artifact(name: &str, contents: &[u8]) -> ModelArtifact {
    let mut digest = Sha256::new();
    digest.update(contents);
    ModelArtifact {
        remote_path: name.to_owned(),
        local_path: name.to_owned(),
        bytes: contents.len() as u64,
        sha256: digest
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
    }
}

fn test_archive(entries: &[(&str, &[u8])]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut encoded = Vec::new();
    {
        let encoder = bzip2::write::BzEncoder::new(&mut encoded, bzip2::Compression::best());
        let mut builder = tar::Builder::new(encoder);
        for (path, contents) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_size(contents.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder.append_data(&mut header, path, *contents)?;
        }
        let encoder = builder.into_inner()?;
        encoder.finish()?;
    }
    Ok(encoded)
}
