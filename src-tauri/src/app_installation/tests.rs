use super::*;
use reqwest::{header::HeaderMap, StatusCode, Url};

/// 测试里的每个 HTTP 客户端都从这里来。
///
/// 这些测试全部打本机 127.0.0.1 上的假服务器。reqwest 默认会读系统代理，
/// 开着代理的开发机（macOS 上 `scutil --proxy` 里 HTTPEnable=1 的那种）
/// 就会把这些请求转给代理，`resolve()` 的覆盖对走代理的请求根本不生效 ——
/// 于是 3 个测试在本地红、在 CI 绿，看着像是断点续传或镜像路由坏了，其实
/// 一个字节都没到假服务器。生产代码里直连客户端本来就是 `no_proxy()`
/// （见 `download::build_client`），这里只是让夹具也如此。
fn local_client() -> reqwest::ClientBuilder {
    reqwest::Client::builder().no_proxy()
}

fn request() -> InstallRequest {
    InstallRequest {
        request_id: "fixture-1".into(),
        tool_id: "claude_desktop".into(),
        action: "start".into(),
        job_id: String::new(),
        intent: None,
    }
}
fn intent() -> Intent {
    Intent {
        line_id: "mainland_optimized".into(),
        model_id: "model-a".into(),
        billing_group: "default".into(),
        models: vec![ModelBinding {
            model_id: "model-a".into(),
            billing_group: "default".into(),
        }],
    }
}
fn fixture() -> (
    AppInstallationState,
    Worker,
    shutdown_coordinator::ShutdownCoordinator,
) {
    let state = AppInstallationState::default();
    let coordinator = shutdown_coordinator::ShutdownCoordinator::default();
    let (sender, receiver) = watch::channel(false);
    let mut progress = InstallProjection::idle(&request());
    progress.job_id = "native-job".into();
    progress.phase = "checking";
    progress.can_cancel = true;
    *state.0.lock().unwrap() = Some(Job {
        progress,
        start_request_id: "fixture-1".into(),
        cancel: sender,
        consent: None,
        package: None,
        operating: true,
    });
    let worker = Worker {
        state: state.clone(),
        id: "native-job".into(),
        cancel: receiver,
        permit: coordinator.admit_operation().unwrap(),
    };
    (state, worker, coordinator)
}

#[test]
fn installer_catalog_is_closed_and_architecture_specific() {
    for arch in ["x64", "arm64"] {
        for tool in ["codex_desktop", "claude_desktop"] {
            let source = catalog::source(tool, "windows", arch).unwrap();
            assert_eq!(catalog::mode(tool, "windows", arch), "system_assisted");
            assert_eq!(source.architecture, arch);
            assert!(catalog::allowed_url(
                source,
                &Url::parse(source.url).unwrap()
            ));
            for bad in [
                "http://persistent.oaistatic.com/codex-app-prod/Codex.dmg",
                "https://example.com/test.msix",
                "https://persistent.oaistatic.com.evil.example/codex-app-prod/test.msix",
                "https://user:pass@downloads.claude.ai/releases/test.msix",
                "https://127.0.0.1/test.msix",
            ] {
                assert!(!catalog::allowed_url(source, &Url::parse(bad).unwrap()));
            }
        }
    }
    assert!(catalog::source("codex_desktop", "macos", "x64").is_none());
    assert_eq!(catalog::mode("pi", "macos", "x64"), "guided");
    assert!(catalog::guide("arbitrary").is_none());
    assert!(serde_json::from_value::<InstallRequest>(serde_json::json!({"requestId":"x","toolId":"pi","action":"start","jobId":"","url":"https://evil.example"})).is_err());
}

#[test]
fn installer_mac_identity_and_destination_conflicts_never_fake_presence() {
    let root = cache::unique_dir(&std::env::temp_dir()).unwrap();
    let source = catalog::source("codex_desktop", "macos", "arm64").unwrap();
    let chatgpt = root.join("ChatGPT.app");
    std::fs::create_dir(&chatgpt).unwrap();
    // Canonical discovery rejects ordinary ChatGPT's non-Codex bundle ID.
    // Merely having that bundle beside the destination must not stop Codex.
    assert_eq!(
        platform::mac_destination_presence(source, &root, false),
        Ok(false)
    );
    // Canonical discovery also recognizes genuine Codex renamed ChatGPT.app.
    assert_eq!(
        platform::mac_destination_presence(source, &root, true),
        Ok(true)
    );
    let destination = root.join("Codex.app");
    cache::write(&destination, b"unrelated user file").unwrap();
    assert_eq!(
        platform::mac_destination_presence(source, &root, false),
        Err("installation_location_conflict")
    );
    assert_eq!(std::fs::read(&destination).unwrap(), b"unrelated user file");
    std::fs::remove_file(&destination).unwrap();
    std::fs::create_dir(&destination).unwrap();
    assert_eq!(
        platform::mac_destination_presence(source, &root, false),
        Err("installation_location_conflict")
    );
    std::fs::remove_dir(&destination).unwrap();
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(root.join("missing"), &destination).unwrap();
        assert_eq!(
            platform::mac_destination_presence(source, &root, false),
            Err("installation_location_conflict")
        );
        assert!(std::fs::symlink_metadata(&destination)
            .unwrap()
            .file_type()
            .is_symlink());
    }
    let (state, worker, _) = fixture();
    worker.finish(Err("installation_location_conflict"));
    assert_eq!(state.read(&request()).phase, "failed");
    assert_eq!(state.read(&request()).disposition, "none");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn installer_download_validates_exact_ranges_and_strong_replay() {
    let mut h = HeaderMap::new();
    h.insert("content-type", "application/octet-stream".parse().unwrap());
    h.insert("content-length", "60".parse().unwrap());
    h.insert("content-range", "bytes 40-99/100".parse().unwrap());
    assert_eq!(
        download::response_size(&h, StatusCode::PARTIAL_CONTENT, 40, 100),
        Ok(100)
    );
    assert!(download::response_size(&h, StatusCode::PARTIAL_CONTENT, 41, 100).is_err());
    assert_eq!(download::response_size(&h, StatusCode::OK, 0, 100), Ok(60));
    h.insert("content-type", "text/html".parse().unwrap());
    assert!(download::response_size(&h, StatusCode::OK, 0, 100).is_err());
    assert!(download::strong_etag("\"original\""));
    for bad in ["", "W/\"weak\"", "\"bad\nvalue\"", "\"embedded\"quote\""] {
        assert!(!download::strong_etag(bad));
    }
}

#[test]
fn installer_consent_is_exact_one_use_and_never_existing_or_ambiguous() {
    let (state, worker, _) = fixture();
    let account = AccountV2State::default();
    let epoch = native_session_epoch(&account).unwrap();
    let choice = intent();
    assert!(choice.valid());
    worker.update(|j| {
        j.progress.phase = "installed";
        j.progress.disposition = "created";
        j.progress.installation_id = "exact-target".into();
        j.consent = Some(Consent {
            intent: choice.clone(),
            epoch,
        });
    });
    assert!(state
        .claim(
            "native-job",
            "claude_desktop",
            "wrong-target",
            &choice,
            &account
        )
        .is_err());
    assert!(state
        .claim(
            "native-job",
            "claude_desktop",
            "exact-target",
            &choice,
            &account
        )
        .is_err());
    worker.update(|j| {
        j.consent = Some(Consent {
            intent: choice.clone(),
            epoch,
        })
    });
    assert!(state
        .claim(
            "native-job",
            "claude_desktop",
            "exact-target",
            &choice,
            &account
        )
        .is_ok());
    assert!(state
        .claim(
            "native-job",
            "claude_desktop",
            "exact-target",
            &choice,
            &account
        )
        .is_err());
    for disposition in ["existing", "unconfirmed"] {
        worker.update(|j| {
            j.progress.disposition = disposition;
            j.consent = Some(Consent {
                intent: choice.clone(),
                epoch,
            });
        });
        assert!(state
            .claim(
                "native-job",
                "claude_desktop",
                "exact-target",
                &choice,
                &account
            )
            .is_err());
    }
    let mut changed = choice;
    changed.models.push(ModelBinding {
        model_id: "model-a".into(),
        billing_group: "other".into(),
    });
    assert!(!changed.valid());
}

#[test]
fn installer_progress_survives_observation_without_relaunch_or_fake_connection() {
    let (state, worker, coordinator) = fixture();
    worker.phase("awaiting_system_confirmation", false);
    assert!(state.read(&request()).active());
    assert_eq!(state.read(&request()).phase, "awaiting_system_confirmation");
    assert_eq!(state.read(&request()).disposition, "none");
    worker.finish(Err("installation_not_detected"));
    assert_eq!(state.read(&request()).phase, "awaiting_system_confirmation");
    assert!(!state.0.lock().unwrap().as_ref().unwrap().operating);
    drop(worker); // Windows handoff does not retain a background admission.
    assert!(coordinator.request_shutdown());
}

#[tokio::test]
async fn installer_cancel_and_shutdown_never_drop_an_owned_mutation() {
    let (state, worker, coordinator) = fixture();
    coordinator.request_shutdown();
    assert_eq!(worker.check_cancelled(), Err("cancelled"));
    worker.phase("installing", false);
    assert_eq!(
        coordinator
            .wait_quiescent(tokio::time::Instant::now())
            .await,
        shutdown_coordinator::DrainOutcome::FinishingOperation
    );
    // Cancellation signals ownership; the permit is retained until finish.
    worker.finish(Err("installation_failed"));
    assert_eq!(state.read(&request()).phase, "failed");
    drop(worker);
    assert_eq!(
        coordinator
            .wait_quiescent(tokio::time::Instant::now())
            .await,
        shutdown_coordinator::DrainOutcome::Quiescent
    );
    let (_, before, coordinator) = fixture();
    before.update(|j| {
        j.cancel.send_replace(true);
    });
    assert_eq!(before.check_cancelled(), Err("cancelled"));
    before.finish(Err("cancelled"));
    drop(before);
    assert!(coordinator.request_shutdown());
}

#[cfg(target_os = "macos")]
#[test]
fn installer_exclusive_destination_and_cache_links_preserve_existing_bytes() {
    let parent = std::env::temp_dir();
    let root = cache::unique_dir(&parent).unwrap();
    let original = root.join("original");
    let candidate = root.join("candidate");
    cache::write(&original, b"keep me").unwrap();
    cache::write(&candidate, b"new app").unwrap();
    assert_eq!(
        platform::publish_exclusive(&candidate, &original),
        Err("installation_location_conflict")
    );
    assert_eq!(std::fs::read(&original).unwrap(), b"keep me");
    let link = root.join("link");
    std::os::unix::fs::symlink(&original, &link).unwrap();
    assert!(cache::open(&link, false).is_err());
    assert_eq!(std::fs::read(&original).unwrap(), b"keep me");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn installer_windows_handoff_does_not_silently_deploy_or_change_trust() {
    let source = include_str!("windows.ps1");
    for forbidden in [
        "Add-AppxPackage",
        "AllowUnsigned",
        "Set-ExecutionPolicy",
        "Import-Certificate",
        "ForceApplicationShutdown",
    ] {
        assert!(!source.contains(forbidden));
    }
    let observation: platform::WindowsObservation =
        serde_json::from_str(r#"{"present":false,"path":""}"#).unwrap();
    assert!(!observation.present);
    assert!(observation.path.is_empty());
    assert!(observation.reason.is_empty());
    assert!(serde_json::from_str::<platform::WindowsObservation>(
        r#"{"present":true,"path":"C:\\fake","secret":"bad"}"#
    )
    .is_err());
}

#[test]
fn installer_consent_rejects_changed_full_intent_and_expired_epoch() {
    let (state, worker, _) = fixture();
    let epoch = native_session_epoch(&AccountV2State::default()).unwrap();
    let mut variants = vec![intent(); 4];
    variants[0].line_id = "global_accelerated".into();
    variants[1].billing_group = "other".into();
    variants[2].models.push(ModelBinding {
        model_id: "model-b".into(),
        billing_group: "default".into(),
    });
    variants[3].model_id = "model-b".into();
    for choice in variants {
        worker.update(|j| {
            j.progress.phase = "installed";
            j.progress.disposition = "confirmed";
            j.progress.installation_id = "exact".into();
            j.consent = Some(Consent {
                intent: intent(),
                epoch,
            });
        });
        assert!(state
            .claim_checked(
                "native-job",
                "claude_desktop",
                "exact",
                &choice,
                |_| panic!("mismatched intent must not reach epoch validation")
            )
            .is_err());
        assert!(state.0.lock().unwrap().as_ref().unwrap().consent.is_none());
    }
    worker.update(|j| {
        j.consent = Some(Consent {
            intent: intent(),
            epoch,
        })
    });
    assert!(state
        .claim_checked("native-job", "claude_desktop", "exact", &intent(), |_| Err(
            "installation_confirmation_required"
        ))
        .is_err());
    assert!(state
        .claim_checked("native-job", "claude_desktop", "exact", &intent(), |_| Ok(
            ()
        ))
        .is_err());
}

#[tokio::test]
async fn installer_handoff_dismissal_revokes_consent_and_worker_failure_releases_admission() {
    let (state, worker, coordinator) = fixture();
    let epoch = native_session_epoch(&AccountV2State::default()).unwrap();
    worker.update(|j| {
        j.progress.phase = "awaiting_system_confirmation";
        j.progress.can_cancel = false;
        j.consent = Some(Consent {
            intent: intent(),
            epoch,
        });
        assert!(j.blocks_start("different-click"));
        j.cancel_guidance();
        assert!(j.consent.is_none());
        assert!(j.operating);
        assert!(!*j.cancel.borrow());
        j.operating = false;
        j.cancel_guidance();
        assert_eq!(j.progress.phase, "cancelled");
        assert!(!j.blocks_start("new-click"));
        assert!(j.blocks_start("fixture-1"));
    });
    drop(worker);
    coordinator.request_shutdown();
    assert_eq!(
        coordinator
            .wait_quiescent(tokio::time::Instant::now())
            .await,
        shutdown_coordinator::DrainOutcome::Quiescent
    );
    assert_eq!(
        state.read(&request()).reason_code,
        "system_handoff_dismissed"
    );
    let (state, worker, coordinator) = fixture();
    worker.launch(
        catalog::source("codex_desktop", "windows", "x64").unwrap(),
        Work::FailedWorker,
    );
    tokio::time::timeout(Duration::from_secs(1), async {
        while state.read(&request()).phase != "failed" {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(!state.0.lock().unwrap().as_ref().unwrap().operating);
    coordinator.request_shutdown();
    assert_eq!(
        coordinator
            .wait_quiescent(tokio::time::Instant::now())
            .await,
        shutdown_coordinator::DrainOutcome::Quiescent
    );
}

#[cfg(target_os = "macos")]
#[test]
fn installer_windows_exact_manifest_target_and_immutable_handoff() {
    use crate::desktop_app_discovery_core::{DesktopLaunchTarget, WindowsPackageApplication};
    let root = cache::unique_dir(&std::env::temp_dir()).unwrap();
    let package = root.join("package");
    cache::directory(&package).unwrap();
    let exe = package.join("Codex.exe");
    cache::write(&exe, b"test executable").unwrap();
    cache::write(&package.join("AppxManifest.xml"), br#"<Package><Applications><Application Id="App" Executable="Codex.exe"/></Applications></Package>"#).unwrap();
    let identity = WindowsPackageApplication::from_installed_package(
        "codex_desktop",
        "OpenAI.Codex_1.2.3.4_x64__abcdefghijklm",
        "OpenAI.Codex_abcdefghijklm",
        "OpenAI.Codex_abcdefghijklm!App",
    )
    .unwrap();
    let good = vec![(
        exe.clone(),
        DesktopLaunchTarget::WindowsPackage(identity.clone()),
    )];
    assert_eq!(
        platform::confirmed_executable("codex_desktop", &package, &good).unwrap(),
        std::fs::canonicalize(&exe).unwrap()
    );
    assert!(platform::confirmed_executable(
        "codex_desktop",
        &package,
        &[(
            exe.clone(),
            DesktopLaunchTarget::WindowsExecutable(exe.clone())
        )]
    )
    .is_err());
    let mut duplicate = good.clone();
    duplicate.extend(good);
    assert!(platform::confirmed_executable("codex_desktop", &package, &duplicate).is_err());
    let download = root.join("installer.msix");
    cache::write(&download, b"version one").unwrap();
    let first_hash = cache::digest(&download).unwrap();
    let opened = cache::handoff_package(&root, &download, &first_hash).unwrap();
    cache::write(&download, b"version two").unwrap();
    // A crashed pending copy cannot occupy the immutable final hash key.
    cache::write(&root.join("handoff.pending"), b"partial").unwrap();
    let next =
        cache::handoff_package(&root, &download, &cache::digest(&download).unwrap()).unwrap();
    assert_ne!(opened, next);
    assert_eq!(std::fs::read(opened).unwrap(), b"version one");
    assert_eq!(std::fs::read(next).unwrap(), b"version two");
    let link = root.join("hardlink");
    std::fs::hard_link(&download, &link).unwrap();
    assert!(cache::open(&link, false).is_err());
    assert_eq!(std::fs::read(download).unwrap(), b"version two");
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn installer_real_http_interruption_resumes_only_matching_bytes() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let mut requests = Vec::new();
        for index in 0..2 {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                let mut byte = [0u8; 1];
                socket.read_exact(&mut byte).await.unwrap();
                request.push(byte[0]);
                assert!(request.len() < 8192);
            }
            requests.push(String::from_utf8(request).unwrap());
            if index == 0 {
                socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Length: 100\r\nETag: \"v1\"\r\nConnection: close\r\n\r\n").await.unwrap();
                socket.write_all(&[b'a'; 40]).await.unwrap();
                socket.flush().await.unwrap();
                tokio::time::sleep(Duration::from_millis(40)).await;
            } else {
                socket.write_all(b"HTTP/1.1 206 Partial Content\r\nContent-Type: application/octet-stream\r\nContent-Length: 60\r\nContent-Range: bytes 40-99/100\r\nETag: \"v1\"\r\nConnection: close\r\n\r\n").await.unwrap();
                socket.write_all(&[b'b'; 60]).await.unwrap();
            }
        }
        requests
    });
    let source = catalog::Source {
        url: Box::leak(format!("http://{address}/package.msix").into_boxed_str()),
        ..catalog::source("codex_desktop", "windows", "x64").unwrap()
    };
    let root = cache::unique_dir(&std::env::temp_dir()).unwrap();
    let (_, worker, _) = fixture();
    let client = local_client()
        .read_timeout(Duration::from_secs(2))
        .build()
        .unwrap();
    let result = download::fetch_with_client(&worker, source, &root, &client, |_, u| {
        u.host_str() == Some("127.0.0.1")
    })
    .await
    .unwrap();
    let requests = server.await.unwrap();
    assert!(requests[1].contains("range: bytes=40-"));
    assert!(requests[1].contains("if-range: \"v1\""));
    assert_eq!(
        std::fs::read(&result.0).unwrap(),
        [vec![b'a'; 40], vec![b'b'; 60]].concat()
    );
    assert_eq!(result.1, cache::digest(&result.0).unwrap());
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn installer_stalled_download_cancels_without_waiting_for_server() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (ready, started) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut buffer = [0; 4096];
        let received = socket.read(&mut buffer).await.unwrap();
        assert!(received > 0);
        socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Length: 100\r\nETag: \"v1\"\r\n\r\n").await.unwrap();
        let _ = ready.send(());
        std::future::pending::<()>().await;
    });
    let root = cache::unique_dir(&std::env::temp_dir()).unwrap();
    let (state, worker, coordinator) = fixture();
    let source = catalog::Source {
        url: Box::leak(format!("http://{address}/package.msix").into_boxed_str()),
        ..catalog::source("codex_desktop", "windows", "x64").unwrap()
    };
    let copy = root.clone();
    let task = tokio::spawn(async move {
        let client = local_client().build().unwrap();
        let result = worker
            .cancellable_download(download::fetch_with_client(
                &worker,
                source,
                &copy,
                &client,
                |_, u| u.host_str() == Some("127.0.0.1"),
            ))
            .await;
        worker.finish(result.map(|_| ()));
    });
    started.await.unwrap();
    coordinator.request_shutdown();
    tokio::time::timeout(Duration::from_secs(1), task)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(state.read(&request()).phase, "cancelled");
    assert_eq!(
        coordinator
            .wait_quiescent(tokio::time::Instant::now())
            .await,
        shutdown_coordinator::DrainOutcome::Quiescent
    );
    server.abort();
    let _ = server.await;
    std::fs::remove_dir_all(root).unwrap();
}

async fn installer_http_fixture(
    bodies: Vec<Vec<u8>>,
) -> (String, tokio::task::JoinHandle<Vec<String>>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let mut requests = Vec::new();
        for body in bodies {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                let mut byte = [0];
                socket.read_exact(&mut byte).await.unwrap();
                request.push(byte[0]);
                assert!(request.len() < 8192);
            }
            requests.push(String::from_utf8(request).unwrap());
            socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\nETag: \"same\"\r\nConnection: close\r\n\r\n", body.len()).as_bytes()).await.unwrap();
            socket.write_all(&body).await.unwrap();
        }
        requests
    });
    (base, server)
}

#[tokio::test]
async fn installer_mirror_hash_failure_falls_back_to_official_without_credentials() {
    let (base, server) = installer_http_fixture(vec![vec![b'a'; 100], vec![b'b'; 100]]).await;
    let source = catalog::source("codex_desktop", "windows", "x64").unwrap();
    let root = cache::unique_dir(&std::env::temp_dir()).unwrap();
    let (state, worker, _) = fixture();
    let client = local_client()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let target = origins::Resolved {
        url: format!("{base}/mirror"),
        sha256: "0".repeat(64),
        size: 100,
        mirror: true,
    };
    let official = origins::Resolved::official(&format!("{base}/official"));
    let result = download::fetch_from_sources(
        &worker,
        source,
        &root,
        &client,
        None,
        Some(target),
        async { Ok(official) },
        |_, u| u.host_str() == Some("127.0.0.1"),
    )
    .await
    .unwrap();
    assert_eq!(std::fs::read(&result.0).unwrap(), vec![b'b'; 100]);
    assert_eq!(state.read(&request()).source, "official");
    let requests = server.await.unwrap();
    assert!(requests[0].starts_with("GET /mirror"));
    assert!(requests[1].starts_with("GET /official"));
    assert!(!requests[1].contains("range:"));
    assert!(requests
        .iter()
        .all(|r| !r.contains("authorization:") && !r.contains("cookie:")));
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn installer_good_mirror_never_contacts_blocked_official_feed() {
    let body = vec![b'z'; 100];
    let (base, server) = installer_http_fixture(vec![body.clone()]).await;
    let source = catalog::source("claude_desktop", "macos", "x64").unwrap();
    let root = cache::unique_dir(&std::env::temp_dir()).unwrap();
    cache::write(&root.join("hash-fixture"), &body).unwrap();
    let hash = cache::digest(&root.join("hash-fixture")).unwrap();
    let (state, worker, _) = fixture();
    let client = local_client()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let target = origins::Resolved {
        url: format!("{base}/mirror.zip"),
        sha256: hash.clone(),
        size: 100,
        mirror: true,
    };
    let result = download::fetch_from_sources(
        &worker,
        source,
        &root,
        &client,
        None,
        Some(target),
        async { panic!("usable mirror must not resolve the official feed") },
        |_, _| false,
    )
    .await
    .unwrap();
    assert_eq!(result.1, hash);
    assert_eq!(state.read(&request()).source, "mirror");
    assert_eq!(server.await.unwrap().len(), 1);
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn installer_public_mirror_failure_uses_direct_route_before_official() {
    let body = vec![b'd'; 100];
    let (base, server) = installer_http_fixture(vec![body.clone()]).await;
    let address: std::net::SocketAddr = base.trim_start_matches("http://").parse().unwrap();
    let source = catalog::source("codex_desktop", "windows", "x64").unwrap();
    let root = cache::unique_dir(&std::env::temp_dir()).unwrap();
    cache::write(&root.join("hash-fixture"), &body).unwrap();
    let hash = cache::digest(&root.join("hash-fixture")).unwrap();
    let (state, worker, _) = fixture();
    let public = local_client()
        .redirect(reqwest::redirect::Policy::none())
        .resolve("mirror.test", "127.0.0.1:1".parse().unwrap())
        .build()
        .unwrap();
    let direct = local_client()
        .redirect(reqwest::redirect::Policy::none())
        .resolve("mirror.test", address)
        .build()
        .unwrap();
    let target = origins::Resolved {
        url: "http://mirror.test/package.msix".into(),
        sha256: hash.clone(),
        size: 100,
        mirror: true,
    };
    let result = download::fetch_from_sources(
        &worker,
        source,
        &root,
        &public,
        Some(&direct),
        Some(target),
        async { panic!("the direct mirror succeeded; official must stay lazy") },
        |_, _| false,
    )
    .await
    .unwrap();
    assert_eq!(result.1, hash);
    assert_eq!(state.read(&request()).source, "mirror");
    let requests = server.await.unwrap();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].starts_with("GET /package.msix"));
    assert!(requests
        .iter()
        .all(|r| !r.to_ascii_lowercase().contains("authorization:")
            && !r.to_ascii_lowercase().contains("cookie:")));
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn installer_resume_is_bound_to_artifact_not_merely_etag() {
    let (base, server) = installer_http_fixture(vec![vec![b'b'; 100]]).await;
    let source = catalog::source("codex_desktop", "windows", "x64").unwrap();
    let root = cache::unique_dir(&std::env::temp_dir()).unwrap();
    cache::write(&root.join("installer.msix"), &[b'a'; 40]).unwrap();
    cache::write(&root.join("download.json"), &serde_json::to_vec(&serde_json::json!({"etag":"\"same\"","total":100,"sha256":"","artifact_url":format!("{base}/previous"),"expected_sha256":"","expected_size":0})).unwrap()).unwrap();
    let (_, worker, _) = fixture();
    let client = local_client().build().unwrap();
    let target = origins::Resolved::official(&format!("{base}/new"));
    let result = download::fetch_resolved(
        &worker,
        source,
        &root,
        &client,
        |_, u| u.host_str() == Some("127.0.0.1"),
        &target,
    )
    .await
    .unwrap();
    assert_eq!(std::fs::read(result.0).unwrap(), vec![b'b'; 100]);
    assert!(!server.await.unwrap()[0].contains("range:"));
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn installer_claude_zip_redirect_cannot_change_the_feed_release() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        while !request.ends_with(b"\r\n\r\n") {
            let mut byte = [0];
            socket.read_exact(&mut byte).await.unwrap();
            request.push(byte[0]);
        }
        socket.write_all(b"HTTP/1.1 302 Found\r\nLocation: /releases/darwin/universal/1.0/Claude-old.zip\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await.unwrap();
        // There must not be a second request to the now unselected release.
        assert!(
            tokio::time::timeout(Duration::from_millis(100), listener.accept())
                .await
                .is_err()
        );
    });
    let source = catalog::source("claude_desktop", "macos", "x64").unwrap();
    let root = cache::unique_dir(&std::env::temp_dir()).unwrap();
    let (_, worker, _) = fixture();
    let target = origins::Resolved::official(&format!(
        "{base}/releases/darwin/universal/2.0/Claude-current.zip"
    ));
    let client = local_client()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    assert_eq!(
        download::fetch_resolved(
            &worker,
            source,
            &root,
            &client,
            |_, u| u.host_str() == Some("127.0.0.1"),
            &target
        )
        .await,
        Err("invalid_source")
    );
    server.await.unwrap();
    assert!(!root.join("installer.zip").exists());
    std::fs::remove_dir_all(root).unwrap();
}

async fn installer_redirect_generation_case(partial: bool) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/resolver", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let mut requests = Vec::new();
        for index in 0..4 {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                let mut byte = [0];
                socket.read_exact(&mut byte).await.unwrap();
                request.push(byte[0]);
            }
            requests.push(String::from_utf8(request).unwrap());
            if index % 2 == 0 {
                socket.write_all(format!("HTTP/1.1 302 Found\r\nLocation: /v{}.msix\r\nContent-Length: 0\r\nConnection: close\r\n\r\n", index / 2 + 1).as_bytes()).await.unwrap();
            } else {
                socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Length: 100\r\nETag: \"same\"\r\nConnection: close\r\n\r\n").await.unwrap();
                let bytes = vec![
                    if index == 1 { b'a' } else { b'b' };
                    if partial && index == 1 { 40 } else { 100 }
                ];
                socket.write_all(&bytes).await.unwrap();
                socket.flush().await.unwrap();
                tokio::time::sleep(Duration::from_millis(40)).await;
            }
        }
        requests
    });
    let source = catalog::source("claude_desktop", "windows", "x64").unwrap();
    let root = cache::unique_dir(&std::env::temp_dir()).unwrap();
    let (_, worker, _) = fixture();
    let client = local_client()
        .redirect(reqwest::redirect::Policy::none())
        .read_timeout(Duration::from_secs(2))
        .build()
        .unwrap();
    let target = origins::Resolved::official(&url);
    let first = download::fetch_resolved(
        &worker,
        source,
        &root,
        &client,
        |_, u| u.host_str() == Some("127.0.0.1"),
        &target,
    )
    .await
    .unwrap();
    if partial {
        assert_eq!(std::fs::read(first.0).unwrap(), vec![b'b'; 100]);
    } else {
        assert_eq!(std::fs::read(first.0).unwrap(), vec![b'a'; 100]);
        let next = download::fetch_resolved(
            &worker,
            source,
            &root,
            &client,
            |_, u| u.host_str() == Some("127.0.0.1"),
            &target,
        )
        .await
        .unwrap();
        assert_eq!(std::fs::read(next.0).unwrap(), vec![b'b'; 100]);
    }
    let requests = server.await.unwrap();
    assert!(requests[3].starts_with("GET /v2.msix"));
    assert!(requests.iter().all(|r| !r.contains("range:")));
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn installer_redirect_cache_never_reuses_another_resource_etag() {
    installer_redirect_generation_case(false).await;
}

#[tokio::test]
async fn installer_redirect_resume_never_appends_another_resource_etag() {
    installer_redirect_generation_case(true).await;
}
