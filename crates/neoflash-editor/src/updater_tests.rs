use crate::{
    app::Editor,
    build_info::BuildIdentity,
    tests::primary_button,
    updater::{
        GitHubAsset, GitHubRelease, PlatformTarget, ReleaseVersion, UpdateState, UpdaterConfig,
        UpdaterController, is_safe_asset_filename, lookup_sha256_for_asset, select_checksum_asset,
        select_platform_asset, should_run_daily_check,
    },
    workspace::{UpdateChannel, WorkspacePrefs},
};
use eframe::egui::{self, Event, Pos2, Rect, Vec2};
use neoflash_core::sha256::Sha256;
use neoflash_core::{
    AuthoringCommandStep, AuthoringExtensionManifest, ExtensionPermission, Project, SavedCommand,
};
use neoflash_export::release_http::validate_release_url;
use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Write},
    net::{TcpListener, TcpStream},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

struct TempDirGuard(PathBuf);

impl TempDirGuard {
    fn new(prefix: &str) -> Self {
        static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let dir = std::env::temp_dir().join(format!(
            "{prefix}-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).expect("create temp test dir");
        Self(dir)
    }
}

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[derive(Clone)]
enum FixtureResponse {
    Static {
        status: u16,
        headers: Vec<(String, String)>,
        body: Vec<u8>,
    },
    Streamed {
        total_len: usize,
        chunk: Vec<u8>,
        chunks: usize,
        delay_per_chunk: Duration,
        first_chunk_sent: Arc<AtomicBool>,
    },
    RawBodyNoContentLength {
        body: Vec<u8>,
    },
}

struct FixtureServer {
    base_url: String,
    addr: std::net::SocketAddr,
    stop: Arc<AtomicBool>,
    routes: Arc<Mutex<HashMap<String, FixtureResponse>>>,
    handle: Option<thread::JoinHandle<()>>,
}

impl FixtureServer {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback fixture");
        listener
            .set_nonblocking(true)
            .expect("nonblocking fixture listener");
        let addr = listener.local_addr().expect("fixture local addr");
        let stop = Arc::new(AtomicBool::new(false));
        let stop_srv = Arc::clone(&stop);
        let routes: Arc<Mutex<HashMap<String, FixtureResponse>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let routes_srv = Arc::clone(&routes);

        let handle = thread::spawn(move || {
            while !stop_srv.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let routes_req = Arc::clone(&routes_srv);
                        thread::spawn(move || handle_fixture_client(stream, &routes_req));
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(_) => break,
                }
            }
        });

        Self {
            base_url: format!("http://{addr}"),
            addr,
            stop,
            routes,
            handle: Some(handle),
        }
    }

    fn set_route(&self, path: &str, response: FixtureResponse) {
        self.routes
            .lock()
            .expect("lock fixture routes")
            .insert(path.to_owned(), response);
    }
}

impl Drop for FixtureServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        let _ = TcpStream::connect(self.addr);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

fn handle_fixture_client(stream: TcpStream, routes: &Mutex<HashMap<String, FixtureResponse>>) {
    // Accepted sockets from a non-blocking listener inherit O_NONBLOCK on macOS/BSD.
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
    let mut reader = BufReader::new(&stream);
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).is_err() || request_line.is_empty() {
        return;
    }
    let path = request_line
        .split_whitespace()
        .nth(1)
        .unwrap_or("/")
        .to_owned();
    // Drain request headers.
    loop {
        let mut header_line = String::new();
        if reader.read_line(&mut header_line).is_err()
            || header_line == "\r\n"
            || header_line == "\n"
            || header_line.is_empty()
        {
            break;
        }
    }

    let reply = routes.lock().expect("lock routes").get(&path).cloned();
    let mut writer = &stream;
    match reply {
        Some(FixtureResponse::Static {
            status,
            headers,
            body,
        }) => {
            let reason = match status {
                200 => "OK",
                302 => "Found",
                403 => "Forbidden",
                404 => "Not Found",
                429 => "Too Many Requests",
                _ => "Status",
            };
            let _ = write!(
                writer,
                "HTTP/1.1 {status} {reason}\r\nContent-Length: {}\r\nConnection: close\r\n",
                body.len()
            );
            for (k, v) in headers {
                let _ = write!(writer, "{k}: {v}\r\n");
            }
            let _ = write!(writer, "\r\n");
            let _ = writer.write_all(&body);
            let _ = writer.flush();
        }
        Some(FixtureResponse::Streamed {
            total_len,
            chunk,
            chunks,
            delay_per_chunk,
            first_chunk_sent,
        }) => {
            let _ = write!(
                writer,
                "HTTP/1.1 200 OK\r\nContent-Length: {total_len}\r\nConnection: close\r\n\r\n"
            );
            for idx in 0..chunks {
                if writer.write_all(&chunk).is_err() || writer.flush().is_err() {
                    break;
                }
                if idx == 0 {
                    first_chunk_sent.store(true, Ordering::Release);
                }
                thread::sleep(delay_per_chunk);
            }
        }
        Some(FixtureResponse::RawBodyNoContentLength { body }) => {
            let _ = write!(writer, "HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n");
            let _ = writer.write_all(&body);
            let _ = writer.flush();
        }
        None => {
            let body = b"not found";
            let _ = write!(
                writer,
                "HTTP/1.1 404 Not Found\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = writer.write_all(body);
            let _ = writer.flush();
        }
    }
}

fn test_platform_target() -> PlatformTarget {
    match PlatformTarget::current() {
        PlatformTarget::Unsupported => PlatformTarget::LinuxX64,
        supported => supported,
    }
}

fn current_platform_asset_name(version: &str) -> String {
    match test_platform_target() {
        PlatformTarget::MacosArm64 => format!("NeoFlash-{version}-macos-arm64.dmg"),
        PlatformTarget::MacosX64 => format!("NeoFlash-{version}-macos-x64.dmg"),
        PlatformTarget::WindowsX64 => format!("NeoFlash-{version}-windows-x64-preview.zip"),
        PlatformTarget::LinuxX64 => format!("NeoFlash-{version}-linux-x64-preview.tar.gz"),
        PlatformTarget::Unsupported => unreachable!(),
    }
}

fn wait_until_idle(ctrl: &mut UpdaterController, ctx: &egui::Context, timeout: Duration) {
    let start = Instant::now();
    while ctrl.is_checking() || ctrl.is_downloading() {
        let _ = ctrl.poll(ctx, false);
        if !ctrl.is_checking() && !ctrl.is_downloading() {
            break;
        }
        assert!(
            start.elapsed() < timeout,
            "timed out waiting for background updater worker; state={:?}",
            ctrl.state
        );
        thread::sleep(Duration::from_millis(10));
    }
}

struct UpdaterUiDriver {
    ctx: egui::Context,
    editor: Editor,
    time: f64,
    out: egui::FullOutput,
}

impl UpdaterUiDriver {
    fn new() -> Self {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let mut editor = Editor::from_project(&ctx, Project::blank());
        let out = Self::run_frame(&mut editor, &ctx, 0.1, vec![]);
        Self {
            ctx,
            editor,
            time: 0.1,
            out,
        }
    }

    fn run_frame(
        editor: &mut Editor,
        ctx: &egui::Context,
        time: f64,
        events: Vec<Event>,
    ) -> egui::FullOutput {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1440.0, 900.0))),
                time: Some(time),
                events,
                ..Default::default()
            },
            |ctx| {
                editor.poll_updater(ctx, false);
                editor.shortcuts(ctx);
                editor.menu(ctx);
                editor.draw_dialogs(ctx);
            },
        )
    }

    fn step(&mut self, events: Vec<Event>) {
        self.time += 0.1;
        self.out = Self::run_frame(&mut self.editor, &self.ctx, self.time, events);
    }

    fn settle(&mut self) {
        self.step(vec![]);
        self.step(vec![]);
    }

    fn find(&self, label: &str) -> Option<Rect> {
        let nodes = &self.out.platform_output.accesskit_update.as_ref()?.nodes;
        nodes.iter().find_map(|(_, node)| {
            (node.label() == Some(label) && node.role() != egui::accesskit::Role::Tab)
                .then(|| node.bounds())
                .flatten()
                .map(|r| {
                    Rect::from_min_max(
                        Pos2::new(r.x0 as f32, r.y0 as f32),
                        Pos2::new(r.x1 as f32, r.y1 as f32),
                    )
                })
        })
    }

    fn click(&mut self, label: &str) {
        let rect = self.find(label).unwrap_or_else(|| {
            panic!(
                "control '{label}' not found in UI; texts={:#?}",
                self.texts()
            )
        });
        let p = rect.center();
        self.step(vec![Event::PointerMoved(p)]);
        self.step(vec![primary_button(p, true), primary_button(p, false)]);
        self.settle();
    }

    fn texts(&self) -> Vec<String> {
        let mut out: Vec<String> = self
            .out
            .platform_output
            .accesskit_update
            .as_ref()
            .map(|u| {
                u.nodes
                    .iter()
                    .filter_map(|(_, n)| n.label().or(n.value()).map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        out.sort();
        out.dedup();
        out
    }

    fn find_node(&self, label: &str) -> Option<&egui::accesskit::Node> {
        self.out
            .platform_output
            .accesskit_update
            .as_ref()?
            .nodes
            .iter()
            .find_map(|(_, node)| (node.label() == Some(label)).then_some(node))
    }
}

#[test]
fn t298_semver_ordering_handles_prereleases_and_numeric_segments() {
    let ordered = [
        "0.1.0-alpha.1",
        "0.1.0-alpha.beta",
        "0.1.0-beta.1",
        "v0.1.0-beta.2",
        "0.1.0-beta.10",
        "0.1.0-rc.1",
        "0.1.0",
        "0.1.1-beta.1",
        "0.2.0",
        "2.0.0",
    ];
    let parsed: Vec<ReleaseVersion> = ordered
        .iter()
        .map(|s| ReleaseVersion::parse(s).unwrap_or_else(|| panic!("parse {s}")))
        .collect();
    for pair in parsed.windows(2) {
        assert!(pair[0] < pair[1], "expected {} < {}", pair[0], pair[1]);
    }
    assert_eq!(
        ReleaseVersion::parse("v0.1.0-beta.2+20261114"),
        ReleaseVersion::parse("0.1.0-beta.2")
    );
    assert!(ReleaseVersion::parse("0.1.0").unwrap().is_stable());
    assert!(!ReleaseVersion::parse("0.1.0-beta.1").unwrap().is_stable());
    for bad in ["", "0.1", "01.0.0", "0.1.0-", "0.1.0+"] {
        assert!(ReleaseVersion::parse(bad).is_none(), "should reject {bad}");
    }
}

#[test]
fn t298_platform_asset_checksum_selection_and_manifest_parsing() {
    let assets = vec![
        GitHubAsset {
            name: "NeoFlash-0.2.0-macos-arm64.dmg".into(),
            browser_download_url: "https://github.com/jazzymiles/NeoFlash/releases/download/v0.2.0/NeoFlash-0.2.0-macos-arm64.dmg".into(),
            size: 1000,
        },
        GitHubAsset {
            name: "NeoFlash-0.2.0-macos-arm64.zip".into(),
            browser_download_url: "https://github.com/jazzymiles/NeoFlash/releases/download/v0.2.0/NeoFlash-0.2.0-macos-arm64.zip".into(),
            size: 900,
        },
        GitHubAsset {
            name: "NeoFlash-0.2.0-windows-x64-preview.zip".into(),
            browser_download_url: "https://github.com/jazzymiles/NeoFlash/releases/download/v0.2.0/NeoFlash-0.2.0-windows-x64-preview.zip".into(),
            size: 1100,
        },
        GitHubAsset {
            name: "NeoFlash-0.2.0-linux-x64-preview.tar.gz".into(),
            browser_download_url: "https://github.com/jazzymiles/NeoFlash/releases/download/v0.2.0/NeoFlash-0.2.0-linux-x64-preview.tar.gz".into(),
            size: 1050,
        },
        GitHubAsset {
            name: "NeoFlash-0.2.0-media-corresponding-source.tar.xz".into(),
            browser_download_url: "https://github.com/jazzymiles/NeoFlash/releases/download/v0.2.0/NeoFlash-0.2.0-media-corresponding-source.tar.xz".into(),
            size: 2000,
        },
        GitHubAsset {
            name: "NeoFlash-0.2.0-SHA256SUMS.txt".into(),
            browser_download_url: "https://github.com/jazzymiles/NeoFlash/releases/download/v0.2.0/NeoFlash-0.2.0-SHA256SUMS.txt".into(),
            size: 300,
        },
        GitHubAsset {
            name: "NeoFlash-0.2.0-windows-x64-SHA256SUMS.txt".into(),
            browser_download_url: "https://github.com/jazzymiles/NeoFlash/releases/download/v0.2.0/NeoFlash-0.2.0-windows-x64-SHA256SUMS.txt".into(),
            size: 300,
        },
        GitHubAsset {
            name: "NeoFlash-0.2.0-linux-x64-SHA256SUMS.txt".into(),
            browser_download_url: "https://github.com/jazzymiles/NeoFlash/releases/download/v0.2.0/NeoFlash-0.2.0-linux-x64-SHA256SUMS.txt".into(),
            size: 300,
        },
    ];

    assert_eq!(
        select_platform_asset(&assets, PlatformTarget::MacosArm64).map(|a| a.name.as_str()),
        Some("NeoFlash-0.2.0-macos-arm64.dmg")
    );
    assert_eq!(
        select_platform_asset(&assets, PlatformTarget::MacosX64).map(|a| a.name.as_str()),
        None
    );
    assert_eq!(
        select_platform_asset(&assets, PlatformTarget::WindowsX64).map(|a| a.name.as_str()),
        Some("NeoFlash-0.2.0-windows-x64-preview.zip")
    );
    assert_eq!(
        select_platform_asset(&assets, PlatformTarget::LinuxX64).map(|a| a.name.as_str()),
        Some("NeoFlash-0.2.0-linux-x64-preview.tar.gz")
    );
    assert_eq!(
        select_platform_asset(&assets, PlatformTarget::Unsupported).map(|a| a.name.as_str()),
        None
    );

    assert_eq!(
        select_checksum_asset(&assets, PlatformTarget::MacosArm64).map(|a| a.name.as_str()),
        Some("NeoFlash-0.2.0-SHA256SUMS.txt")
    );
    assert_eq!(
        select_checksum_asset(&assets, PlatformTarget::WindowsX64).map(|a| a.name.as_str()),
        Some("NeoFlash-0.2.0-windows-x64-SHA256SUMS.txt")
    );
    assert_eq!(
        select_checksum_asset(&assets, PlatformTarget::LinuxX64).map(|a| a.name.as_str()),
        Some("NeoFlash-0.2.0-linux-x64-SHA256SUMS.txt")
    );

    let h1 = "a".repeat(64);
    let h2 = "b".repeat(64);
    let manifest = format!(
        "# release sums\n{h1}  NeoFlash-0.2.0-macos-arm64.dmg\n{h2} *./dist/NeoFlash-0.2.0-windows-x64-preview.zip\n"
    );
    assert_eq!(
        lookup_sha256_for_asset(&manifest, "NeoFlash-0.2.0-macos-arm64.dmg").unwrap(),
        h1
    );
    assert_eq!(
        lookup_sha256_for_asset(&manifest, "NeoFlash-0.2.0-windows-x64-preview.zip").unwrap(),
        h2
    );
    assert!(lookup_sha256_for_asset(&manifest, "NeoFlash-0.2.0-linux-x64-preview.tar.gz").is_err());
    let conflict = format!("{h1}  a.dmg\n{h2}  a.dmg\n");
    assert!(lookup_sha256_for_asset(&conflict, "a.dmg").is_err());
    assert!(lookup_sha256_for_asset("not-a-hash  a.dmg\n", "a.dmg").is_err());
}

#[test]
fn t298_release_url_security_blocks_untrusted_schemes_and_hosts() {
    assert!(
        validate_release_url(
            "https://github.com/jazzymiles/NeoFlash/releases/tag/v0.1.0",
            None
        )
        .is_ok()
    );
    assert!(
        validate_release_url("https://objects.githubusercontent.com/assets/foo.dmg", None).is_ok()
    );
    assert!(validate_release_url("http://github.com/jazzymiles/NeoFlash", None).is_err());
    assert!(validate_release_url("https://evil.example.com/payload.dmg", None).is_err());
    assert!(validate_release_url("https://user:pass@github.com/x", None).is_err());

    let fixture = Some("http://127.0.0.1:41234");
    assert!(validate_release_url("http://127.0.0.1:41234/dl/a.dmg", fixture).is_ok());
    assert!(validate_release_url("http://127.0.0.1:9999/dl/a.dmg", fixture).is_err());
    assert!(validate_release_url("https://github.com/jazzymiles/NeoFlash", fixture).is_err());
}

#[test]
fn t298_channel_filtering_ignores_drafts_and_prereleases_on_stable() {
    let server = FixtureServer::start();
    let base = server.base_url.clone();
    let asset_beta2 = current_platform_asset_name("0.1.0-beta.2");
    let asset_old = current_platform_asset_name("0.0.9");
    let json = serde_json::json!([
        {
            "tag_name": "v9.9.9",
            "html_url": format!("{base}/jazzymiles/NeoFlash/releases/tag/v9.9.9"),
            "draft": true,
            "prerelease": false,
            "assets": []
        },
        {
            "tag_name": "v0.1.0-beta.2",
            "html_url": format!("{base}/jazzymiles/NeoFlash/releases/tag/v0.1.0-beta.2"),
            "draft": false,
            "prerelease": true,
            "assets": [
                {
                    "name": asset_beta2,
                    "browser_download_url": format!("{base}/dl/{asset_beta2}"),
                    "size": 1234
                },
                {
                    "name": "NeoFlash-0.1.0-beta.2-SHA256SUMS.txt",
                    "browser_download_url": format!("{base}/dl/SHA256SUMS.txt"),
                    "size": 120
                }
            ]
        },
        {
            "tag_name": "v0.0.9",
            "html_url": format!("{base}/jazzymiles/NeoFlash/releases/tag/v0.0.9"),
            "draft": false,
            "prerelease": false,
            "assets": [
                {
                    "name": asset_old,
                    "browser_download_url": format!("{base}/dl/{asset_old}"),
                    "size": 999
                }
            ]
        }
    ]);
    server.set_route(
        "/repos/jazzymiles/NeoFlash/releases?per_page=20",
        FixtureResponse::Static {
            status: 200,
            headers: vec![("Content-Type".into(), "application/json".into())],
            body: json.to_string().into_bytes(),
        },
    );

    let ctx = egui::Context::default();
    let mut ctrl = UpdaterController::new(UpdaterConfig {
        api_base_override: Some(base.clone()),
        current_version: "0.1.0-beta.1".into(),
        platform: test_platform_target(),
        ..Default::default()
    });

    ctrl.start_check(UpdateChannel::Stable, &ctx);
    wait_until_idle(&mut ctrl, &ctx, Duration::from_secs(5));
    assert!(
        matches!(
            &ctrl.state,
            UpdateState::UpToDate {
                channel: UpdateChannel::Stable,
                latest_version: Some(v),
                ..
            } if v == "0.0.9"
        ),
        "expected Stable to see v0.0.9 <= current, got {:?}",
        ctrl.state
    );

    ctrl.sync_selected_channel(UpdateChannel::Beta);
    ctrl.start_check(UpdateChannel::Beta, &ctx);
    wait_until_idle(&mut ctrl, &ctx, Duration::from_secs(5));
    assert!(
        matches!(
            &ctrl.state,
            UpdateState::UpdateAvailable {
                channel: UpdateChannel::Beta,
                update
            } if update.version == "0.1.0-beta.2" && update.download.is_ok()
        ),
        "expected Beta to select v0.1.0-beta.2, got {:?}",
        ctrl.state
    );

    // Even if tagged `-beta.2` has prerelease=false, Stable ignores prerelease semvers too.
    let spoofed: Vec<GitHubRelease> = serde_json::from_value(serde_json::json!([
        {
            "tag_name": "v0.1.0-beta.3",
            "html_url": format!("{base}/jazzymiles/NeoFlash/releases/tag/v0.1.0-beta.3"),
            "draft": false,
            "prerelease": false,
            "assets": []
        },
        {
            "tag_name": "v0.1.0",
            "html_url": format!("{base}/jazzymiles/NeoFlash/releases/tag/v0.1.0"),
            "draft": false,
            "prerelease": false,
            "assets": []
        }
    ]))
    .unwrap();
    assert_eq!(spoofed.len(), 2);
}

#[test]
fn t298_rate_limit_and_offline_errors_are_bounded_and_actionable() {
    let server = FixtureServer::start();
    let ctx = egui::Context::default();
    let mut ctrl = UpdaterController::new(UpdaterConfig {
        api_base_override: Some(server.base_url.clone()),
        current_version: "0.1.0-beta.1".into(),
        ..Default::default()
    });

    server.set_route(
        "/repos/jazzymiles/NeoFlash/releases?per_page=20",
        FixtureResponse::Static {
            status: 429,
            headers: vec![("Retry-After".into(), "75".into())],
            body: b"API rate limit exceeded".to_vec(),
        },
    );
    ctrl.start_check(UpdateChannel::Stable, &ctx);
    wait_until_idle(&mut ctrl, &ctx, Duration::from_secs(5));
    assert!(matches!(
        &ctrl.state,
        UpdateState::RateLimited {
            retry_after_secs: Some(75),
            message,
            ..
        } if message.contains("75")
    ));

    server.set_route(
        "/repos/jazzymiles/NeoFlash/releases?per_page=20",
        FixtureResponse::Static {
            status: 403,
            headers: vec![("X-RateLimit-Remaining".into(), "0".into())],
            body: b"rate limit".to_vec(),
        },
    );
    ctrl.start_check(UpdateChannel::Beta, &ctx);
    wait_until_idle(&mut ctrl, &ctx, Duration::from_secs(5));
    assert!(matches!(&ctrl.state, UpdateState::RateLimited { .. }));

    // Closed local port -> offline/network error (never hits real internet).
    let closed_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let closed_port = closed_listener.local_addr().unwrap().port();
    drop(closed_listener);
    let mut offline = UpdaterController::new(UpdaterConfig {
        api_base_override: Some(format!("http://127.0.0.1:{closed_port}")),
        current_version: "0.1.0-beta.1".into(),
        check_timeout: Duration::from_secs(2),
        ..Default::default()
    });
    offline.start_check(UpdateChannel::Stable, &ctx);
    wait_until_idle(&mut offline, &ctx, Duration::from_secs(5));
    assert!(matches!(&offline.state, UpdateState::Error { .. }));
}

#[test]
fn t298_download_verifies_sha256_fails_closed_on_mismatch_and_supports_cancel() {
    let dir = TempDirGuard::new("neoflash-updater-dl");
    let server = FixtureServer::start();
    let base = server.base_url.clone();
    let asset_name = current_platform_asset_name("0.2.0");
    let payload: Vec<u8> = (0..180_000).map(|i| ((i * 17 + 5) % 251) as u8).collect();
    let payload_sha = Sha256::digest_hex(&payload);

    let release_json = serde_json::json!([
        {
            "tag_name": "v0.2.0",
            "html_url": format!("{base}/jazzymiles/NeoFlash/releases/tag/v0.2.0"),
            "draft": false,
            "prerelease": false,
            "assets": [
                {
                    "name": asset_name,
                    "browser_download_url": format!("{base}/redirect/{asset_name}"),
                    "size": payload.len() as u64
                },
                {
                    "name": "NeoFlash-0.2.0-SHA256SUMS.txt",
                    "browser_download_url": format!("{base}/sums/SHA256SUMS.txt"),
                    "size": 256
                }
            ]
        }
    ]);
    server.set_route(
        "/repos/jazzymiles/NeoFlash/releases?per_page=20",
        FixtureResponse::Static {
            status: 200,
            headers: vec![("Content-Type".into(), "application/json".into())],
            body: release_json.to_string().into_bytes(),
        },
    );
    server.set_route(
        &format!("/redirect/{asset_name}"),
        FixtureResponse::Static {
            status: 302,
            headers: vec![("Location".into(), format!("{base}/cdn/{asset_name}"))],
            body: Vec::new(),
        },
    );
    server.set_route(
        &format!("/cdn/{asset_name}"),
        FixtureResponse::Static {
            status: 200,
            headers: vec![],
            body: payload.clone(),
        },
    );
    server.set_route(
        "/sums/SHA256SUMS.txt",
        FixtureResponse::Static {
            status: 200,
            headers: vec![],
            body: format!("{payload_sha}  {asset_name}\n").into_bytes(),
        },
    );

    let ctx = egui::Context::default();
    let mut ctrl = UpdaterController::new(UpdaterConfig {
        api_base_override: Some(base.clone()),
        current_version: "0.1.0-beta.1".into(),
        platform: test_platform_target(),
        ..Default::default()
    });
    ctrl.start_check(UpdateChannel::Stable, &ctx);
    wait_until_idle(&mut ctrl, &ctx, Duration::from_secs(5));
    let available = match &ctrl.state {
        UpdateState::UpdateAvailable { update, .. } => update.clone(),
        other => panic!("expected UpdateAvailable, got {other:?}"),
    };

    // 1. Verified download succeeds and cleans up temp files.
    let good_dest = dir.0.join(&asset_name);
    ctrl.start_download_to_path(
        UpdateChannel::Stable,
        available.clone(),
        good_dest.clone(),
        &ctx,
    );
    wait_until_idle(&mut ctrl, &ctx, Duration::from_secs(5));
    assert!(matches!(
        &ctrl.state,
        UpdateState::VerifiedDownloaded { path, .. } if path == &good_dest
    ));
    assert_eq!(std::fs::read(&good_dest).unwrap(), payload);
    assert_eq!(std::fs::read_dir(&dir.0).unwrap().count(), 1);

    // 2. SHA-256 mismatch fails closed and deletes partial/temp files.
    let bad_sha = "f".repeat(64);
    server.set_route(
        "/sums/SHA256SUMS.txt",
        FixtureResponse::Static {
            status: 200,
            headers: vec![],
            body: format!("{bad_sha}  {asset_name}\n").into_bytes(),
        },
    );
    let bad_dest = dir.0.join("bad-download.bin");
    ctrl.start_download_to_path(
        UpdateChannel::Stable,
        available.clone(),
        bad_dest.clone(),
        &ctx,
    );
    wait_until_idle(&mut ctrl, &ctx, Duration::from_secs(5));
    assert!(
        matches!(&ctrl.state, UpdateState::Error { message, .. } if message.contains("SHA-256")),
        "expected SHA-256 mismatch error, got {:?}",
        ctrl.state
    );
    assert!(!bad_dest.exists(), "bad download must never be saved");
    assert_eq!(
        std::fs::read_dir(&dir.0).unwrap().count(),
        1,
        "temp .part file must be cleaned up"
    );

    // 3. Missing manifest entry fails closed before writing dest.
    server.set_route(
        "/sums/SHA256SUMS.txt",
        FixtureResponse::Static {
            status: 200,
            headers: vec![],
            body: format!("{payload_sha}  other-file.dmg\n").into_bytes(),
        },
    );
    let missing_dest = dir.0.join("missing-sum.bin");
    ctrl.start_download_to_path(
        UpdateChannel::Stable,
        available.clone(),
        missing_dest.clone(),
        &ctx,
    );
    wait_until_idle(&mut ctrl, &ctx, Duration::from_secs(5));
    assert!(matches!(&ctrl.state, UpdateState::Error { .. }));
    assert!(!missing_dest.exists());

    // 4. Streaming progress + clean cancellation removes partial file.
    server.set_route(
        "/sums/SHA256SUMS.txt",
        FixtureResponse::Static {
            status: 200,
            headers: vec![],
            body: format!("{payload_sha}  {asset_name}\n").into_bytes(),
        },
    );
    let first_chunk_sent = Arc::new(AtomicBool::new(false));
    let stream_chunk = vec![0x42u8; 8 * 1024];
    server.set_route(
        &format!("/cdn/{asset_name}"),
        FixtureResponse::Streamed {
            total_len: stream_chunk.len() * 40,
            chunk: stream_chunk,
            chunks: 40,
            delay_per_chunk: Duration::from_millis(30),
            first_chunk_sent: Arc::clone(&first_chunk_sent),
        },
    );
    let cancel_dest = dir.0.join("cancelled.bin");
    ctrl.start_download_to_path(UpdateChannel::Stable, available, cancel_dest.clone(), &ctx);

    let start = Instant::now();
    let mut saw_progress = false;
    while !first_chunk_sent.load(Ordering::Acquire) {
        let _ = ctrl.poll(&ctx, false);
        assert!(start.elapsed() < Duration::from_secs(5));
        thread::sleep(Duration::from_millis(10));
    }
    for _ in 0..20 {
        let _ = ctrl.poll(&ctx, false);
        if let UpdateState::Downloading { received_bytes, .. } = &ctrl.state
            && *received_bytes > 0
        {
            saw_progress = true;
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    assert!(saw_progress, "expected incremental byte progress");
    ctrl.cancel_download();
    assert!(matches!(&ctrl.state, UpdateState::UpdateAvailable { .. }));
    // Poll until the worker observes cancellation and drops its `.part` file guard so slow CI
    // runner scheduling never races a fixed sleep window.
    let cleanup_deadline = Instant::now() + Duration::from_secs(5);
    let leftovers = loop {
        let found: Vec<_> = std::fs::read_dir(&dir.0)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains(".part"))
            .collect();
        if found.is_empty() || Instant::now() >= cleanup_deadline {
            break found;
        }
        thread::sleep(Duration::from_millis(10));
    };
    assert!(!cancel_dest.exists());
    assert!(leftovers.is_empty(), "temp files leaked: {leftovers:?}");
}

#[test]
fn t298_daily_auto_check_persistence_and_throttle() {
    // Old JSON without `updates` preserves backward compatibility with safe defaults.
    let loaded = WorkspacePrefs::from_json("{}");
    assert_eq!(loaded.updates.channel, UpdateChannel::Stable);
    assert!(!loaded.updates.check_automatically);
    assert_eq!(loaded.updates.last_check_unix, None);

    // Roundtrip persists Beta + check_automatically + last_check_unix.
    let mut prefs = WorkspacePrefs::default();
    prefs.updates.channel = UpdateChannel::Beta;
    prefs.updates.check_automatically = true;
    prefs.updates.last_check_unix = Some(1_700_000_000);
    let serialized = serde_json::to_string(&prefs).unwrap();
    let roundtrip = WorkspacePrefs::from_json(&serialized);
    assert_eq!(roundtrip.updates, prefs.updates);

    assert!(should_run_daily_check(None, 1_700_000_000));
    assert!(!should_run_daily_check(Some(1_700_000_000), 1_700_086_399));
    assert!(should_run_daily_check(Some(1_700_000_000), 1_700_086_400));

    let server = FixtureServer::start();
    server.set_route(
        "/repos/jazzymiles/NeoFlash/releases?per_page=20",
        FixtureResponse::Static {
            status: 200,
            headers: vec![("Content-Type".into(), "application/json".into())],
            body: b"[]".to_vec(),
        },
    );
    let ctx = egui::Context::default();

    // Opt-in off -> never auto-checks.
    let mut off_ctrl = UpdaterController::new(UpdaterConfig {
        api_base_override: Some(server.base_url.clone()),
        ..Default::default()
    });
    let mut off_prefs = roundtrip.updates.clone();
    off_prefs.check_automatically = false;
    off_ctrl.maybe_auto_check(&off_prefs, 1_700_200_000, &ctx);
    assert!(matches!(off_ctrl.state, UpdateState::Idle));

    // Recent check -> skipped.
    let mut recent_ctrl = UpdaterController::new(UpdaterConfig {
        api_base_override: Some(server.base_url.clone()),
        ..Default::default()
    });
    recent_ctrl.maybe_auto_check(&roundtrip.updates, 1_700_001_000, &ctx);
    assert!(matches!(recent_ctrl.state, UpdateState::Idle));

    // >= 24h elapsed with opt-in on -> checks once.
    recent_ctrl.maybe_auto_check(&roundtrip.updates, 1_700_090_000, &ctx);
    assert!(recent_ctrl.is_checking());
    wait_until_idle(&mut recent_ctrl, &ctx, Duration::from_secs(5));
    assert!(matches!(recent_ctrl.state, UpdateState::UpToDate { .. }));
}

#[test]
fn t298_preferences_updates_and_about_ui_remove_fake_data_and_verify_real_flows() {
    let dir = TempDirGuard::new("neoflash-updater-ui");
    let server = FixtureServer::start();
    let base = server.base_url.clone();
    let asset_name = current_platform_asset_name("0.1.0-beta.2");
    let payload = b"neoflash-verified-package-bytes".repeat(128);
    let payload_sha = Sha256::digest_hex(&payload);
    let dest_path = dir.0.join(&asset_name);

    let beta_json = serde_json::json!([
        {
            "tag_name": "v0.1.0-beta.2",
            "html_url": format!("{base}/jazzymiles/NeoFlash/releases/tag/v0.1.0-beta.2"),
            "draft": false,
            "prerelease": true,
            "assets": [
                {
                    "name": asset_name,
                    "browser_download_url": format!("{base}/dl/{asset_name}"),
                    "size": payload.len() as u64
                },
                {
                    "name": "NeoFlash-0.1.0-beta.2-SHA256SUMS.txt",
                    "browser_download_url": format!("{base}/dl/SHA256SUMS.txt"),
                    "size": 128
                }
            ]
        }
    ]);
    server.set_route(
        "/repos/jazzymiles/NeoFlash/releases?per_page=20",
        FixtureResponse::Static {
            status: 200,
            headers: vec![("Content-Type".into(), "application/json".into())],
            body: beta_json.to_string().into_bytes(),
        },
    );
    server.set_route(
        &format!("/dl/{asset_name}"),
        FixtureResponse::Static {
            status: 200,
            headers: vec![],
            body: payload.clone(),
        },
    );
    server.set_route(
        "/dl/SHA256SUMS.txt",
        FixtureResponse::Static {
            status: 200,
            headers: vec![],
            body: format!("{payload_sha}  {asset_name}\n").into_bytes(),
        },
    );

    let mut d = UpdaterUiDriver::new();
    d.editor.updater.config = UpdaterConfig {
        api_base_override: Some(base),
        current_version: "0.1.0-beta.1".into(),
        platform: test_platform_target(),
        download_dest_override: Some(dest_path.clone()),
        ..Default::default()
    };

    // 1. About NeoFlash exposes real version + build + commit matching Updates.
    let id = BuildIdentity::current();
    d.click("Help");
    d.click("About NeoFlash");
    let about_texts = d.texts();
    assert!(
        about_texts.iter().any(|t| t.contains(&id.version))
            && about_texts.iter().any(|t| t == &id.about_subtitle()),
        "About NeoFlash must show real version and subtitle; got {about_texts:#?}"
    );
    d.click("Close");

    // 2. Preferences ▸ Updates shows zero fake data and real installed extensions count.
    d.click("Edit");
    d.click("Preferences… (⌘U)");
    d.click("Updates");
    let texts = d.texts();
    for forbidden in [
        "NeoFlash 2.0.0",
        "build 412",
        "3 extensions",
        "1 update available",
    ] {
        assert!(
            !texts.iter().any(|t| t.contains(forbidden)),
            "fabricated string '{forbidden}' must not appear: {texts:#?}"
        );
    }
    assert!(
        texts.iter().any(|t| t.contains(&id.version_prefix())),
        "Updates must show real version prefix '{}': {texts:#?}",
        id.version_prefix()
    );
    assert!(texts.iter().any(|t| t == "0 extensions installed"));

    // Telemetry and background switches must be disabled and explicitly off.
    for label in [
        "Send anonymous usage statistics",
        "Send crash reports",
        "Download in background",
    ] {
        let node = d
            .find_node(label)
            .unwrap_or_else(|| panic!("missing switch '{label}'"));
        assert!(node.is_disabled(), "switch '{label}' must stay disabled");
        assert_ne!(
            node.toggled(),
            Some(egui::accesskit::Toggled::True),
            "switch '{label}' must never look enabled/on"
        );
    }

    // Install a real extension manifest and verify the count updates dynamically.
    d.click("Cancel");
    let ext = AuthoringExtensionManifest {
        schema_version: 1,
        id: "neoflash.test.updater".into(),
        name: "Test Pack".into(),
        version: "1.0.0".into(),
        author: "NeoFlash".into(),
        permissions: vec![ExtensionPermission::StageTransform],
        commands: vec![SavedCommand {
            name: "Noop".into(),
            steps: vec![AuthoringCommandStep::Rotate { degrees: 15.0 }],
        }],
    };
    d.editor
        .install_extension_manifest(&ext.to_json().unwrap())
        .unwrap();
    d.click("Edit");
    d.click("Preferences… (⌘U)");
    d.click("Updates");
    assert!(d.texts().iter().any(|t| t == "1 extension installed"));

    // 3. Switch to Beta channel, check for updates, download and verify SHA-256 via UI clicks.
    d.click("Beta");
    d.click("Check for updates automatically");
    d.click("Check now");
    let wait_start = Instant::now();
    while d.editor.updater.is_checking() {
        assert!(wait_start.elapsed() < Duration::from_secs(5));
        thread::sleep(Duration::from_millis(10));
        d.step(vec![]);
    }
    d.settle();
    assert!(
        d.texts()
            .iter()
            .any(|t| t.contains("Update 0.1.0-beta.2 is available")),
        "expected update banner: {:#?}",
        d.texts()
    );
    assert!(
        d.editor.workspace.updates.last_check_unix.is_some(),
        "completed check records timestamp"
    );

    d.click("Download");
    let dl_start = Instant::now();
    while d.editor.updater.is_downloading() {
        assert!(dl_start.elapsed() < Duration::from_secs(5));
        thread::sleep(Duration::from_millis(10));
        d.step(vec![]);
    }
    d.settle();
    assert_eq!(std::fs::read(&dest_path).unwrap(), payload);
    assert!(
        d.texts()
            .iter()
            .any(|t| t.contains("Verified 0.1.0-beta.2 (SHA-256)")),
        "expected verified banner: {:#?}",
        d.texts()
    );

    // Save persists Channel + Check automatically; Reset restores defaults.
    d.click("Save");
    assert_eq!(d.editor.workspace.updates.channel, UpdateChannel::Beta);
    assert!(d.editor.workspace.updates.check_automatically);

    d.click("Edit");
    d.click("Preferences… (⌘U)");
    d.click("Updates");
    d.click("Reset to defaults");
    let draft_updates = d.editor.preferences.as_ref().unwrap().updates.clone();
    assert_eq!(draft_updates.channel, UpdateChannel::Stable);
    assert!(!draft_updates.check_automatically);
    assert!(
        draft_updates.last_check_unix.is_some(),
        "Reset keeps check history timestamp"
    );
    d.click("Cancel");
    assert_eq!(
        d.editor.workspace.updates.channel,
        UpdateChannel::Beta,
        "Cancel preserves previously saved preferences"
    );
}

#[test]
fn t472_platform_asset_and_checksum_selection_covers_macos_intel_linux_deb_and_manifests() {
    let mk_asset = |name: &str, size: u64| GitHubAsset {
        name: name.to_owned(),
        browser_download_url: format!(
            "https://github.com/jazzymiles/NeoFlash/releases/download/v0.1.0/{name}"
        ),
        size,
    };

    // Full multi-platform release set with macOS arm64 + x86_64, Windows x64, Linux .deb + .tar.gz,
    // and distinct per-platform SHA256SUMS manifests alongside package-macos.sh's SHA256SUMS.txt.
    let full_assets = vec![
        mk_asset("../unsafe-traversal-macos-arm64.dmg", 100),
        mk_asset("NeoFlash-0.1.0-macos-arm64.zip", 42_000_000),
        mk_asset("NeoFlash-0.1.0-macos-arm64.dmg", 44_000_000),
        mk_asset("NeoFlash-0.1.0-macos-x86_64.zip", 45_000_000),
        mk_asset("NeoFlash-0.1.0-macos-x86_64.dmg", 47_000_000),
        mk_asset("NeoFlash-0.1.0-macos-universal.dmg", 90_000_000),
        mk_asset("NeoFlash-0.1.0-windows-x64.zip", 48_000_000),
        mk_asset("neoflash_0.1.0~beta.2+build584_arm64.deb", 39_000_000),
        mk_asset("NeoFlash-0.1.0-linux-x64-preview.tar.gz", 46_000_000),
        mk_asset("neoflash_0.1.0~beta.2+build584_amd64.deb", 41_000_000),
        mk_asset("NeoFlash-0.1.0-windows-x64-SHA256SUMS.txt", 320),
        mk_asset("NeoFlash-0.1.0-linux-x64-SHA256SUMS.txt", 320),
        mk_asset("NeoFlash-0.1.0-macos-x86_64-SHA256SUMS.txt", 320),
        mk_asset("NeoFlash-0.1.0-SHA256SUMS.txt", 320),
    ];

    assert!(!is_safe_asset_filename("../evil.dmg"));
    assert!(!is_safe_asset_filename("sub/NeoFlash.dmg"));
    assert!(!is_safe_asset_filename("sub\\NeoFlash.zip"));
    assert!(is_safe_asset_filename(
        "neoflash_0.1.0~beta.2+build584_amd64.deb"
    ));

    // Primary format preference per platform (.dmg > .zip on macOS, .deb > .tar.gz on Linux x64).
    assert_eq!(
        select_platform_asset(&full_assets, PlatformTarget::MacosArm64).map(|a| a.name.as_str()),
        Some("NeoFlash-0.1.0-macos-arm64.dmg")
    );
    assert_eq!(
        select_platform_asset(&full_assets, PlatformTarget::MacosX64).map(|a| a.name.as_str()),
        Some("NeoFlash-0.1.0-macos-x86_64.dmg")
    );
    assert_eq!(
        select_platform_asset(&full_assets, PlatformTarget::WindowsX64).map(|a| a.name.as_str()),
        Some("NeoFlash-0.1.0-windows-x64.zip")
    );
    assert_eq!(
        select_platform_asset(&full_assets, PlatformTarget::LinuxX64).map(|a| a.name.as_str()),
        Some("neoflash_0.1.0~beta.2+build584_amd64.deb")
    );

    // Fallback formats: .zip on macOS arm64 & x86_64, universal when dedicated Intel absent, .tar.gz on Linux.
    let zip_and_tar_only = vec![
        mk_asset("NeoFlash-0.1.0-macos-arm64.zip", 42_000_000),
        mk_asset("NeoFlash-0.1.0-macos-x86_64.zip", 45_000_000),
        mk_asset("NeoFlash-0.1.0-linux-x64.tar.gz", 46_000_000),
    ];
    assert_eq!(
        select_platform_asset(&zip_and_tar_only, PlatformTarget::MacosArm64)
            .map(|a| a.name.as_str()),
        Some("NeoFlash-0.1.0-macos-arm64.zip")
    );
    assert_eq!(
        select_platform_asset(&zip_and_tar_only, PlatformTarget::MacosX64).map(|a| a.name.as_str()),
        Some("NeoFlash-0.1.0-macos-x86_64.zip")
    );
    assert_eq!(
        select_platform_asset(&zip_and_tar_only, PlatformTarget::LinuxX64).map(|a| a.name.as_str()),
        Some("NeoFlash-0.1.0-linux-x64.tar.gz")
    );
    let universal_only = vec![mk_asset("NeoFlash-0.1.0-macos-universal.dmg", 90_000_000)];
    assert_eq!(
        select_platform_asset(&universal_only, PlatformTarget::MacosX64).map(|a| a.name.as_str()),
        Some("NeoFlash-0.1.0-macos-universal.dmg")
    );

    // Checksum manifest disambiguation: MacosArm64 never picks macos-x86_64-SHA256SUMS.txt or vice versa.
    assert_eq!(
        select_checksum_asset(&full_assets, PlatformTarget::MacosArm64).map(|a| a.name.as_str()),
        Some("NeoFlash-0.1.0-SHA256SUMS.txt")
    );
    assert_eq!(
        select_checksum_asset(&full_assets, PlatformTarget::MacosX64).map(|a| a.name.as_str()),
        Some("NeoFlash-0.1.0-macos-x86_64-SHA256SUMS.txt")
    );
    assert_eq!(
        select_checksum_asset(&full_assets, PlatformTarget::WindowsX64).map(|a| a.name.as_str()),
        Some("NeoFlash-0.1.0-windows-x64-SHA256SUMS.txt")
    );
    assert_eq!(
        select_checksum_asset(&full_assets, PlatformTarget::LinuxX64).map(|a| a.name.as_str()),
        Some("NeoFlash-0.1.0-linux-x64-SHA256SUMS.txt")
    );

    // Multi-format SHA256SUMS parsing: UTF-8 BOM, two-space, binary `*`, Windows `.\`, BSD `SHA256 (...) =`.
    let d1 = "1a".repeat(32);
    let d2 = "2b".repeat(32);
    let d3 = "3c".repeat(32);
    let d4 = "4d".repeat(32);
    let manifest = format!(
        "\u{feff}# multi-format sums\n\
         {d1}  NeoFlash-0.1.0-macos-arm64.dmg\n\
         {d2} *NeoFlash-0.1.0-macos-x86_64.dmg\n\
         {d3} *.\\dist\\NeoFlash-0.1.0-windows-x64.zip\n\
         SHA256 (neoflash_0.1.0~beta.2+build584_amd64.deb) = {d4}\n"
    );
    assert_eq!(
        lookup_sha256_for_asset(&manifest, "NeoFlash-0.1.0-macos-arm64.dmg").unwrap(),
        d1
    );
    assert_eq!(
        lookup_sha256_for_asset(&manifest, "NeoFlash-0.1.0-macos-x86_64.dmg").unwrap(),
        d2
    );
    assert_eq!(
        lookup_sha256_for_asset(&manifest, "NeoFlash-0.1.0-windows-x64.zip").unwrap(),
        d3
    );
    assert_eq!(
        lookup_sha256_for_asset(&manifest, "neoflash_0.1.0~beta.2+build584_amd64.deb").unwrap(),
        d4
    );
    assert!(lookup_sha256_for_asset(&manifest, "../NeoFlash-0.1.0-macos-arm64.dmg").is_err());
    assert!(lookup_sha256_for_asset(&format!("{d1}  *\n"), "a.dmg").is_err());
}

#[test]
fn t472_loopback_github_releases_multi_mib_e2e_hardening() {
    let dir = TempDirGuard::new("neoflash-t472-updater-e2e");
    let server = FixtureServer::start();
    let base = server.base_url.clone();

    // Realistic 3.2 MiB binary archive (`3_355_443` bytes) deterministic pseudo-random payload.
    let archive_len: usize = 3 * 1024 * 1024 + 209_715;
    let mut archive_bytes = vec![0u8; archive_len];
    let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
    for chunk in archive_bytes.chunks_mut(8) {
        state ^= state >> 12;
        state ^= state << 25;
        state ^= state >> 27;
        let bytes = state.wrapping_mul(0x2545_F491_4F6C_DD1D).to_le_bytes();
        for (dst, src) in chunk.iter_mut().zip(bytes.iter()) {
            *dst = *src;
        }
    }
    let archive_sha = Sha256::digest_hex(&archive_bytes);

    let intel_dmg = "NeoFlash-0.1.0-beta.2-macos-x86_64.dmg";
    let arm_dmg = "NeoFlash-0.1.0-beta.2-macos-arm64.dmg";
    let linux_deb = "neoflash_0.1.0~beta.2+build584_amd64.deb";
    let win_zip = "NeoFlash-0.1.0-beta.2-windows-x64.zip";
    let sums_name = "NeoFlash-0.1.0-beta.2-SHA256SUMS.txt";

    let sums_manifest = format!(
        "# NeoFlash 0.1.0-beta.2 SHA-256 sums\n\
         {archive_sha}  {arm_dmg}\n\
         {archive_sha} *{intel_dmg}\n\
         {archive_sha} *./dist/{win_zip}\n\
         {archive_sha}  {linux_deb}\n"
    );

    // Phase A: Draft v0.2.0 + Prerelease v0.1.0-beta.2 + Old Stable v0.0.9.
    let ok_resp = |body: Vec<u8>| FixtureResponse::Static {
        status: 200,
        headers: Vec::new(),
        body,
    };
    let asset_json = |name: &str, size: u64| {
        serde_json::json!({
            "name": name,
            "browser_download_url": format!("{base}/releases/download/v0.1.0-beta.2/{name}"),
            "size": size
        })
    };
    let phase_a_releases = serde_json::json!([
        { "tag_name": "v0.2.0", "draft": true, "prerelease": false, "assets": [] },
        {
            "tag_name": "v0.1.0-beta.2",
            "html_url": format!("{base}/jazzymiles/NeoFlash/releases/tag/v0.1.0-beta.2"),
            "draft": false,
            "prerelease": true,
            "assets": [
                asset_json(arm_dmg, archive_len as u64),
                asset_json(intel_dmg, archive_len as u64),
                asset_json(win_zip, archive_len as u64),
                asset_json(linux_deb, archive_len as u64),
                asset_json(sums_name, sums_manifest.len() as u64),
            ]
        },
        { "tag_name": "v0.0.9", "draft": false, "prerelease": false, "assets": [] }
    ]);
    server.set_route(
        "/repos/jazzymiles/NeoFlash/releases?per_page=20",
        ok_resp(phase_a_releases.to_string().into_bytes()),
    );
    for asset in [arm_dmg, intel_dmg, win_zip, linux_deb] {
        server.set_route(
            &format!("/releases/download/v0.1.0-beta.2/{asset}"),
            FixtureResponse::Static {
                status: 302,
                headers: vec![("Location".into(), format!("{base}/objects/{asset}"))],
                body: Vec::new(),
            },
        );
        server.set_route(&format!("/objects/{asset}"), ok_resp(archive_bytes.clone()));
    }
    server.set_route(
        &format!("/releases/download/v0.1.0-beta.2/{sums_name}"),
        ok_resp(sums_manifest.clone().into_bytes()),
    );

    let ctx = egui::Context::default();
    let mut ctrl = UpdaterController::new(UpdaterConfig {
        api_base_override: Some(base.clone()),
        current_version: "0.1.0-beta.1".into(),
        platform: PlatformTarget::MacosX64,
        ..Default::default()
    });

    // 1. Channel filtering: Stable skips draft v0.2.0 & prerelease v0.1.0-beta.2 -> UpToDate(0.0.9).
    ctrl.start_check(UpdateChannel::Stable, &ctx);
    wait_until_idle(&mut ctrl, &ctx, Duration::from_secs(5));
    assert!(matches!(
        &ctrl.state,
        UpdateState::UpToDate { channel: UpdateChannel::Stable, latest_version: Some(v), .. } if v == "0.0.9"
    ));

    // Beta selects 0.1.0-beta.2 (> 0.1.0-beta.1) and resolves macos-x86_64.dmg + SHA256SUMS.txt.
    ctrl.sync_selected_channel(UpdateChannel::Beta);
    ctrl.start_check(UpdateChannel::Beta, &ctx);
    wait_until_idle(&mut ctrl, &ctx, Duration::from_secs(5));
    let beta_update = match &ctrl.state {
        UpdateState::UpdateAvailable {
            channel: UpdateChannel::Beta,
            update,
        } => {
            assert_eq!(update.version, "0.1.0-beta.2");
            assert_eq!(
                update.download.as_ref().expect("x86_64 asset").asset_name,
                intel_dmg
            );
            update.clone()
        }
        other => panic!("expected Beta UpdateAvailable, got {other:?}"),
    };

    // 2. Happy-path verified 3.2 MiB download via 302 redirect + `*`-prefixed SHA256SUMS entry.
    let verified_dest = dir.0.join(intel_dmg);
    ctrl.start_download_to_path(
        UpdateChannel::Beta,
        beta_update.clone(),
        verified_dest.clone(),
        &ctx,
    );
    wait_until_idle(&mut ctrl, &ctx, Duration::from_secs(10));
    assert!(matches!(
        &ctrl.state,
        UpdateState::VerifiedDownloaded { version, path, .. }
            if version == "0.1.0-beta.2" && path == &verified_dest
    ));
    let saved_bytes = std::fs::read(&verified_dest).expect("read verified 3.2 MiB dmg");
    assert_eq!(saved_bytes.len(), archive_len);
    assert_eq!(Sha256::digest_hex(&saved_bytes), archive_sha);

    // 3. Tampered-byte fail-closed cleanup: flip 1 byte in the 3.2 MiB payload; verify neither a
    //    new destination nor an existing verified file at `verified_dest` is corrupted or leaked.
    let mut tampered_bytes = archive_bytes.clone();
    tampered_bytes[archive_len / 2] ^= 0xA5;
    server.set_route(&format!("/objects/{intel_dmg}"), ok_resp(tampered_bytes));
    let tampered_dest = dir.0.join("tampered-intel.dmg");
    ctrl.start_download_to_path(
        UpdateChannel::Beta,
        beta_update.clone(),
        tampered_dest.clone(),
        &ctx,
    );
    wait_until_idle(&mut ctrl, &ctx, Duration::from_secs(10));
    assert!(
        matches!(&ctrl.state, UpdateState::Error { message, .. } if message.contains("SHA-256"))
    );
    assert!(!tampered_dest.exists());
    ctrl.start_download_to_path(
        UpdateChannel::Beta,
        beta_update.clone(),
        verified_dest.clone(),
        &ctx,
    );
    wait_until_idle(&mut ctrl, &ctx, Duration::from_secs(10));
    assert_eq!(
        Sha256::digest_hex(&std::fs::read(&verified_dest).unwrap()),
        archive_sha
    );
    let part_leaks: Vec<_> = std::fs::read_dir(&dir.0)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().contains(".part"))
        .collect();
    assert!(part_leaks.is_empty(), "temp .part leaked: {part_leaks:?}");

    server.set_route(
        &format!("/objects/{intel_dmg}"),
        ok_resp(archive_bytes.clone()),
    );

    // 4. Missing-checksum rejection: (a) manifest missing entry, (b) release missing SHA256SUMS.txt.
    server.set_route(
        &format!("/releases/download/v0.1.0-beta.2/{sums_name}"),
        ok_resp(format!("{archive_sha}  {arm_dmg}\n").into_bytes()),
    );
    let no_entry_dest = dir.0.join("no-entry.dmg");
    ctrl.start_download_to_path(
        UpdateChannel::Beta,
        beta_update.clone(),
        no_entry_dest.clone(),
        &ctx,
    );
    wait_until_idle(&mut ctrl, &ctx, Duration::from_secs(5));
    assert!(matches!(
        &ctrl.state,
        UpdateState::Error { message, .. } if message.contains("does not contain a checksum")
    ));
    assert!(!no_entry_dest.exists());

    let no_sums_release = serde_json::json!([
        {
            "tag_name": "v0.1.0",
            "draft": false,
            "prerelease": false,
            "assets": [{
                "name": "NeoFlash-0.1.0-macos-x86_64.dmg",
                "browser_download_url": format!("{base}/objects/{intel_dmg}"),
                "size": archive_len as u64
            }]
        },
        phase_a_releases[1].clone()
    ]);
    server.set_route(
        "/repos/jazzymiles/NeoFlash/releases?per_page=20",
        ok_resp(no_sums_release.to_string().into_bytes()),
    );
    ctrl.sync_selected_channel(UpdateChannel::Stable);
    ctrl.start_check(UpdateChannel::Stable, &ctx);
    wait_until_idle(&mut ctrl, &ctx, Duration::from_secs(5));
    let no_sums_update = match &ctrl.state {
        UpdateState::UpdateAvailable {
            channel: UpdateChannel::Stable,
            update,
        } => {
            assert_eq!(update.version, "0.1.0");
            assert!(
                update
                    .download
                    .as_ref()
                    .unwrap_err()
                    .contains("missing a SHA256SUMS manifest")
            );
            update.clone()
        }
        other => panic!("expected UpdateAvailable for 0.1.0, got {other:?}"),
    };
    let blocked_dest = dir.0.join("blocked-0.1.0.dmg");
    ctrl.start_download_to_path(
        UpdateChannel::Stable,
        no_sums_update,
        blocked_dest.clone(),
        &ctx,
    );
    assert!(matches!(&ctrl.state, UpdateState::Error { .. }));
    assert!(!blocked_dest.exists());

    server.set_route(
        &format!("/releases/download/v0.1.0-beta.2/{sums_name}"),
        ok_resp(sums_manifest.into_bytes()),
    );

    // 5. Size-cap enforcement: both declared `asset.size > max_download_bytes` and streamed body
    //    without `Content-Length` exceeding `max_download_bytes` fail closed and remove `.part`.
    let mut capped_ctrl = UpdaterController::new(UpdaterConfig {
        api_base_override: Some(base.clone()),
        current_version: "0.1.0-beta.1".into(),
        platform: PlatformTarget::MacosX64,
        max_download_bytes: 1024 * 1024, // 1 MiB cap vs 3.2 MiB archive
        ..Default::default()
    });
    let over_declared_dest = dir.0.join("over-declared.dmg");
    capped_ctrl.start_download_to_path(
        UpdateChannel::Beta,
        beta_update.clone(),
        over_declared_dest.clone(),
        &ctx,
    );
    wait_until_idle(&mut capped_ctrl, &ctx, Duration::from_secs(5));
    assert!(
        matches!(&capped_ctrl.state, UpdateState::Error { message, .. } if message.contains("download limit")),
        "expected declared size-cap rejection, got {:?}",
        capped_ctrl.state
    );
    assert!(!over_declared_dest.exists());

    let mut zero_size_update = beta_update.clone();
    if let Ok(ref mut target) = zero_size_update.download {
        target.asset_size = 0; // Simulate missing size metadata + omitted HTTP Content-Length.
    }
    server.set_route(
        &format!("/objects/{intel_dmg}"),
        FixtureResponse::RawBodyNoContentLength {
            body: archive_bytes,
        },
    );
    let over_stream_dest = dir.0.join("over-stream.dmg");
    capped_ctrl.start_download_to_path(
        UpdateChannel::Beta,
        zero_size_update,
        over_stream_dest.clone(),
        &ctx,
    );
    wait_until_idle(&mut capped_ctrl, &ctx, Duration::from_secs(5));
    assert!(
        matches!(&capped_ctrl.state, UpdateState::Error { message, .. } if message.contains("safety limit")),
        "expected streaming size-cap rejection, got {:?}",
        capped_ctrl.state
    );
    assert!(!over_stream_dest.exists());

    // 6. Mid-stream cooperative cancellation on a 4 MiB chunked transfer cleans `.part` deterministically.
    let first_chunk_sent = Arc::new(AtomicBool::new(false));
    let chunk_64k = vec![0x5Au8; 64 * 1024];
    server.set_route(
        &format!("/objects/{intel_dmg}"),
        FixtureResponse::Streamed {
            total_len: chunk_64k.len() * 64, // 4 MiB
            chunk: chunk_64k,
            chunks: 64,
            delay_per_chunk: Duration::from_millis(25),
            first_chunk_sent: Arc::clone(&first_chunk_sent),
        },
    );
    let cancel_dest = dir.0.join("cancelled-4mib.dmg");
    ctrl.start_download_to_path(UpdateChannel::Beta, beta_update, cancel_dest.clone(), &ctx);

    let cancel_wait = Instant::now();
    let mut observed_mid_stream = false;
    while cancel_wait.elapsed() < Duration::from_secs(5) {
        let _ = ctrl.poll(&ctx, false);
        if first_chunk_sent.load(Ordering::Acquire)
            && let UpdateState::Downloading {
                received_bytes,
                total_bytes: Some(total),
                ..
            } = &ctrl.state
            && *received_bytes > 0
            && *received_bytes < *total
        {
            observed_mid_stream = true;
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    assert!(observed_mid_stream, "expected mid-stream byte progress");
    ctrl.cancel_download();
    assert!(matches!(
        &ctrl.state,
        UpdateState::UpdateAvailable {
            channel: UpdateChannel::Beta,
            ..
        }
    ));
    let drain_start = Instant::now();
    while ctrl.has_active_worker() {
        let _ = ctrl.poll(&ctx, false);
        assert!(drain_start.elapsed() < Duration::from_secs(5));
        thread::sleep(Duration::from_millis(10));
    }
    assert!(
        matches!(
            &ctrl.state,
            UpdateState::UpdateAvailable {
                channel: UpdateChannel::Beta,
                ..
            }
        ),
        "cancelled worker must keep UpdateAvailable state, got {:?}",
        ctrl.state
    );
    assert!(!cancel_dest.exists());
    let final_entries: Vec<String> = std::fs::read_dir(&dir.0)
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        final_entries,
        vec![intel_dmg.to_owned()],
        "only the single SHA-256 verified archive may exist on disk"
    );
}

#[test]
#[ignore = "live network GitHub Releases check + sandboxed notarized build-605 upgrade/rollback handoff (T-347)"]
fn t347_live_github_release_and_sandboxed_build_605_handoff_verification() {
    use neoflash_export::release_http::fetch_text_with_policy;
    use serde_json::json;
    use std::process::Command;

    let artifact_root = std::env::var_os("NEOFLASH_T347_ARTIFACT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from("/Users/miles/Projects/NeoFlash/target/artifacts/t347-updater-live")
        });
    std::fs::create_dir_all(&artifact_root).expect("create t347 artifact dir");

    let ctx = egui::Context::default();
    let mut live_platforms = Vec::new();

    // 1. Live GitHub Releases API check & live SHA256SUMS verification across all 4 targets
    // from a simulated pre-beta.1 version (`0.1.0-alpha.9`) on Beta channel.
    for platform in [
        PlatformTarget::MacosArm64,
        PlatformTarget::MacosX64,
        PlatformTarget::WindowsX64,
        PlatformTarget::LinuxX64,
    ] {
        let mut ctrl = UpdaterController::new(UpdaterConfig {
            current_version: "0.1.0-alpha.9".into(),
            platform,
            check_timeout: Duration::from_secs(20),
            ..Default::default()
        });
        ctrl.start_check(UpdateChannel::Beta, &ctx);
        wait_until_idle(&mut ctrl, &ctx, Duration::from_secs(30));
        let UpdateState::UpdateAvailable {
            channel: UpdateChannel::Beta,
            update,
        } = &ctrl.state
        else {
            panic!(
                "expected live Beta UpdateAvailable for {platform:?}, got {:?}",
                ctrl.state
            );
        };
        assert_eq!(
            update.version, "0.1.0-beta.1",
            "expected live GitHub release tag v0.1.0-beta.1 for {platform:?}"
        );
        assert!(
            validate_release_url(&update.release_notes_url, None).is_ok(),
            "release notes URL failed validation: {}",
            update.release_notes_url
        );
        let target = update
            .download
            .as_ref()
            .unwrap_or_else(|e| panic!("missing verified asset target for {platform:?}: {e}"));
        assert!(validate_release_url(&target.asset_url, None).is_ok());
        assert!(validate_release_url(&target.checksum_url, None).is_ok());

        // Fetch the real published SHA256SUMS manifest from GitHub and verify checksum lookup.
        let sums_manifest = fetch_text_with_policy(
            &target.checksum_url,
            "text/plain",
            "NeoFlash/0.1.0-alpha.9",
            256 * 1024,
            Duration::from_secs(20),
            None,
        )
        .unwrap_or_else(|e| panic!("failed fetching live SHA256SUMS for {platform:?}: {e}"));
        let expected_sha = lookup_sha256_for_asset(&sums_manifest, &target.asset_name)
            .unwrap_or_else(|e| {
                panic!(
                    "live SHA256SUMS missing entry for {}: {e}",
                    target.asset_name
                )
            });
        assert_eq!(expected_sha.len(), 64);

        live_platforms.push(json!({
            "platform": format!("{platform:?}"),
            "platform_label": platform.label(),
            "simulated_from_version": "0.1.0-alpha.9",
            "resolved_version": update.version,
            "release_notes_url": update.release_notes_url,
            "asset_name": target.asset_name,
            "asset_url": target.asset_url,
            "asset_size": target.asset_size,
            "checksum_url": target.checksum_url,
            "published_sha256": expected_sha,
        }));
    }

    // Also verify live UpToDate behavior when current_version == "0.1.0-beta.1" on Beta & Stable.
    let mut ctrl_beta_cur = UpdaterController::new(UpdaterConfig {
        current_version: "0.1.0-beta.1".into(),
        platform: PlatformTarget::MacosArm64,
        check_timeout: Duration::from_secs(20),
        ..Default::default()
    });
    ctrl_beta_cur.start_check(UpdateChannel::Beta, &ctx);
    wait_until_idle(&mut ctrl_beta_cur, &ctx, Duration::from_secs(30));
    assert!(
        matches!(
            &ctrl_beta_cur.state,
            UpdateState::UpToDate {
                channel: UpdateChannel::Beta,
                latest_version: Some(v),
                ..
            } if v == "0.1.0-beta.1"
        ),
        "expected UpToDate at 0.1.0-beta.1 on Beta, got {:?}",
        ctrl_beta_cur.state
    );

    let mut ctrl_stable_cur = UpdaterController::new(UpdaterConfig {
        current_version: "0.1.0-beta.1".into(),
        platform: PlatformTarget::MacosArm64,
        check_timeout: Duration::from_secs(20),
        ..Default::default()
    });
    ctrl_stable_cur.start_check(UpdateChannel::Stable, &ctx);
    wait_until_idle(&mut ctrl_stable_cur, &ctx, Duration::from_secs(30));
    assert!(
        matches!(
            &ctrl_stable_cur.state,
            UpdateState::UpToDate {
                channel: UpdateChannel::Stable,
                latest_version: None,
                ..
            }
        ),
        "expected UpToDate (no stable release yet) on Stable, got {:?}",
        ctrl_stable_cur.state
    );

    // 2. Sandboxed Build-605 -> Newer Build Upgrade & Rollback Handoff
    let backup_605_app = PathBuf::from(
        "/Users/miles/Projects/NeoFlash/target/artifacts/t488-stapled-arm64-605-backup/NeoFlash.app",
    );
    let newer_source_app = PathBuf::from("/Users/miles/Projects/NeoFlash/dist/NeoFlash.app");
    assert!(
        backup_605_app.is_dir(),
        "missing stapled build-605 backup app at {}",
        backup_605_app.display()
    );
    assert!(
        newer_source_app.is_dir(),
        "missing newer dist/NeoFlash.app at {}",
        newer_source_app.display()
    );

    let sandbox_dir = artifact_root.join("sandbox");
    let _ = std::fs::remove_dir_all(&sandbox_dir);
    let sandbox_apps = sandbox_dir.join("Applications");
    let sandbox_home = sandbox_dir.join("Home");
    let app_support = sandbox_home.join("Library/Application Support/NeoFlash");
    let recovery_dir = app_support.join("Recovery");
    let downloads_dir = sandbox_dir.join("Downloads");
    std::fs::create_dir_all(&sandbox_apps).unwrap();
    std::fs::create_dir_all(&recovery_dir).unwrap();
    std::fs::create_dir_all(&downloads_dir).unwrap();

    // Populate user Application Support state (`workspace.json`, recent files, recovery snapshot)
    // and record exact SHA-256 digests to prove zero mutation/loss across upgrade and rollback.
    let mut prefs = WorkspacePrefs::default();
    prefs.general.recent_files = vec![
        "/Users/miles/Projects/NeoFlash/examples/games/fallout-as2.nflash".into(),
        "/Users/miles/Projects/NeoFlash/examples/games/doom-as2.nflash".into(),
    ];
    let workspace_json_path = app_support.join("workspace.json");
    let workspace_bytes = serde_json::to_vec_pretty(&prefs).unwrap();
    std::fs::write(&workspace_json_path, &workspace_bytes).unwrap();
    let recovery_proj_path = recovery_dir.join("session-live-t347.nflash");
    let recovery_meta_path = recovery_dir.join("session-live-t347.json");
    let recovery_proj_bytes = Project::blank().to_json().unwrap().into_bytes();
    let recovery_meta_bytes =
        br#"{"id":"session-live-t347","title":"Vault13_WIP","saved_unix":1795728000}"#.to_vec();
    std::fs::write(&recovery_proj_path, &recovery_proj_bytes).unwrap();
    std::fs::write(&recovery_meta_path, &recovery_meta_bytes).unwrap();

    let snapshot_support_hashes = || -> serde_json::Map<String, serde_json::Value> {
        let mut map = serde_json::Map::new();
        for (rel, path) in [
            ("workspace.json", &workspace_json_path),
            ("Recovery/session-live-t347.nflash", &recovery_proj_path),
            ("Recovery/session-live-t347.json", &recovery_meta_path),
        ] {
            let bytes = std::fs::read(path).expect("read support file");
            map.insert(rel.into(), json!(Sha256::digest_hex(&bytes)));
        }
        map
    };
    let support_hashes_before = snapshot_support_hashes();

    let installed_app = sandbox_apps.join("NeoFlash.app");
    let status = Command::new("/usr/bin/ditto")
        .arg(&backup_605_app)
        .arg(&installed_app)
        .status()
        .expect("ditto build-605 into sandbox");
    assert!(status.success(), "ditto copy of build-605 failed");

    let inspect_app_bundle = |app_path: &std::path::Path| -> serde_json::Value {
        let exe = app_path.join("Contents/MacOS/neoflash-editor");
        let ver_out = Command::new(&exe)
            .args(["--headless", "version"])
            .env("HOME", &sandbox_home)
            .output()
            .expect("run --headless version");
        assert!(
            ver_out.status.success(),
            "--headless version failed: {}",
            String::from_utf8_lossy(&ver_out.stderr)
        );
        let ver_json: serde_json::Value =
            serde_json::from_slice(&ver_out.stdout).expect("parse version json");

        let cs_verify = Command::new("/usr/bin/codesign")
            .args(["--verify", "--deep", "--strict"])
            .arg(app_path)
            .output()
            .expect("codesign --verify");
        let cs_info = Command::new("/usr/bin/codesign")
            .args(["-dvvv"])
            .arg(app_path)
            .output()
            .expect("codesign -dvvv");
        let cs_stderr = String::from_utf8_lossy(&cs_info.stderr).into_owned();
        let cdhash = cs_stderr
            .lines()
            .find_map(|line| line.strip_prefix("CDHash="))
            .unwrap_or("")
            .trim()
            .to_owned();
        json!({
            "version_json": ver_json,
            "codesign_strict_ok": cs_verify.status.success(),
            "cdhash": cdhash,
        })
    };

    let initial_605_info = inspect_app_bundle(&installed_app);
    assert_eq!(initial_605_info["codesign_strict_ok"], json!(true));
    assert_eq!(
        initial_605_info["cdhash"],
        json!("16ca314c9e6174cbb6c88f8d4da1a73282403989"),
        "sandboxed build-605 must preserve notarized CDHash"
    );
    assert_eq!(
        initial_605_info["version_json"]["build_number"],
        json!(605),
        "expected initial build_number 605, got {initial_605_info}"
    );

    // Stage simulated v0.1.0-beta.2 release ZIP from the newer dist/NeoFlash.app bundle.
    let newer_stage_parent = sandbox_dir.join("release-stage");
    std::fs::create_dir_all(&newer_stage_parent).unwrap();
    let newer_staged_app = newer_stage_parent.join("NeoFlash.app");
    assert!(
        Command::new("/usr/bin/ditto")
            .arg(&newer_source_app)
            .arg(&newer_staged_app)
            .status()
            .unwrap()
            .success()
    );
    let release_zip_name = "NeoFlash-0.1.0-beta.2-macos-arm64.zip";
    let release_zip_path = sandbox_dir.join(release_zip_name);
    assert!(
        Command::new("/usr/bin/ditto")
            .args(["-c", "-k", "--sequesterRsrc", "--keepParent"])
            .arg(&newer_staged_app)
            .arg(&release_zip_path)
            .status()
            .unwrap()
            .success(),
        "failed creating v0.1.0-beta.2 release zip"
    );
    let zip_bytes = std::fs::read(&release_zip_path).unwrap();
    let zip_bytes_len = zip_bytes.len();
    let zip_sha256 = Sha256::digest_hex(&zip_bytes);

    let server = FixtureServer::start();
    let base = server.base_url.clone();
    let release_json = json!([
        {
            "tag_name": "v0.1.0-beta.2",
            "html_url": format!("{base}/jazzymiles/NeoFlash/releases/tag/v0.1.0-beta.2"),
            "draft": false,
            "prerelease": true,
            "assets": [
                {
                    "name": release_zip_name,
                    "browser_download_url": format!("{base}/dl/{release_zip_name}"),
                    "size": zip_bytes.len() as u64
                },
                {
                    "name": "NeoFlash-0.1.0-beta.2-SHA256SUMS.txt",
                    "browser_download_url": format!("{base}/dl/NeoFlash-0.1.0-beta.2-SHA256SUMS.txt"),
                    "size": 128
                }
            ]
        }
    ]);
    server.set_route(
        "/repos/jazzymiles/NeoFlash/releases?per_page=20",
        FixtureResponse::Static {
            status: 200,
            headers: vec![("Content-Type".into(), "application/json".into())],
            body: release_json.to_string().into_bytes(),
        },
    );
    server.set_route(
        &format!("/dl/{release_zip_name}"),
        FixtureResponse::Static {
            status: 200,
            headers: vec![("Content-Type".into(), "application/zip".into())],
            body: zip_bytes,
        },
    );

    // 2a. Corrupted SHA-256 fail-closed check: tampered manifest MUST abort and delete partial output!
    let wrong_sha = "0".repeat(64);
    server.set_route(
        "/dl/NeoFlash-0.1.0-beta.2-SHA256SUMS.txt",
        FixtureResponse::Static {
            status: 200,
            headers: vec![("Content-Type".into(), "text/plain".into())],
            body: format!("{wrong_sha}  {release_zip_name}\n").into_bytes(),
        },
    );

    let mut handoff_ctrl = UpdaterController::new(UpdaterConfig {
        api_base_override: Some(base.clone()),
        current_version: "0.1.0-beta.1".into(),
        platform: PlatformTarget::MacosArm64,
        max_download_bytes: 512 * 1024 * 1024,
        ..Default::default()
    });
    handoff_ctrl.start_check(UpdateChannel::Beta, &ctx);
    wait_until_idle(&mut handoff_ctrl, &ctx, Duration::from_secs(10));
    let available_beta2 = match &handoff_ctrl.state {
        UpdateState::UpdateAvailable { update, .. } => update.clone(),
        other => panic!("expected UpdateAvailable for v0.1.0-beta.2, got {other:?}"),
    };

    let tampered_dest = downloads_dir.join("tampered-beta2.zip");
    handoff_ctrl.start_download_to_path(
        UpdateChannel::Beta,
        available_beta2.clone(),
        tampered_dest.clone(),
        &ctx,
    );
    wait_until_idle(&mut handoff_ctrl, &ctx, Duration::from_secs(30));
    let tampered_error_msg = match &handoff_ctrl.state {
        UpdateState::Error { message, .. } => message.clone(),
        other => panic!("expected fail-closed Error on SHA-256 mismatch, got {other:?}"),
    };
    assert!(
        !tampered_dest.exists(),
        "tampered download must not leave unverified archive at destination"
    );
    assert_eq!(
        std::fs::read_dir(&downloads_dir).unwrap().count(),
        0,
        "downloads_dir must have zero .part leftovers after fail-closed rejection"
    );

    // 2b. Authentic SHA-256 verified download -> atomic bundle handoff (605 -> newer)
    server.set_route(
        "/dl/NeoFlash-0.1.0-beta.2-SHA256SUMS.txt",
        FixtureResponse::Static {
            status: 200,
            headers: vec![("Content-Type".into(), "text/plain".into())],
            body: format!("{zip_sha256}  {release_zip_name}\n").into_bytes(),
        },
    );
    let verified_dest = downloads_dir.join(release_zip_name);
    handoff_ctrl.start_download_to_path(
        UpdateChannel::Beta,
        available_beta2,
        verified_dest.clone(),
        &ctx,
    );
    wait_until_idle(&mut handoff_ctrl, &ctx, Duration::from_secs(30));
    assert!(
        matches!(
            &handoff_ctrl.state,
            UpdateState::VerifiedDownloaded { path, version, .. }
                if path == &verified_dest && version == "0.1.0-beta.2"
        ),
        "expected VerifiedDownloaded, got {:?}",
        handoff_ctrl.state
    );
    let downloaded_sha = Sha256::digest_hex(&std::fs::read(&verified_dest).unwrap());
    assert_eq!(downloaded_sha, zip_sha256);

    // Unpack verified archive into staging dir and atomically swap sandbox/Applications/NeoFlash.app
    let unpack_dir = sandbox_dir.join("unpack-beta2");
    std::fs::create_dir_all(&unpack_dir).unwrap();
    assert!(
        Command::new("/usr/bin/ditto")
            .args(["-x", "-k"])
            .arg(&verified_dest)
            .arg(&unpack_dir)
            .status()
            .unwrap()
            .success(),
        "failed unpacking verified update zip"
    );
    let unpacked_app = unpack_dir.join("NeoFlash.app");
    let rollback_backup_app = sandbox_apps.join("NeoFlash-605-rollback.app");
    std::fs::rename(&installed_app, &rollback_backup_app).expect("stage 605 rollback bundle");
    std::fs::rename(&unpacked_app, &installed_app).expect("activate upgraded bundle");

    let upgraded_info = inspect_app_bundle(&installed_app);
    assert_eq!(upgraded_info["codesign_strict_ok"], json!(true));
    let upgraded_build = upgraded_info["version_json"]["build_number"]
        .as_u64()
        .expect("upgraded build_number");
    assert!(
        upgraded_build > 605,
        "expected upgraded build_number > 605, got {upgraded_info}"
    );
    let support_hashes_after_upgrade = snapshot_support_hashes();
    assert_eq!(
        support_hashes_after_upgrade, support_hashes_before,
        "Application Support preferences, recent files, and recovery snapshot must survive upgrade untouched"
    );

    // 2c. Atomic rollback (newer -> 605) & exact notarized CDHash restoration
    let replaced_newer_app = sandbox_apps.join("NeoFlash-upgraded-prev.app");
    std::fs::rename(&installed_app, &replaced_newer_app).expect("park newer bundle");
    std::fs::rename(&rollback_backup_app, &installed_app).expect("restore build-605 bundle");
    let rolled_back_info = inspect_app_bundle(&installed_app);
    assert_eq!(rolled_back_info["codesign_strict_ok"], json!(true));
    assert_eq!(
        rolled_back_info["cdhash"],
        json!("16ca314c9e6174cbb6c88f8d4da1a73282403989"),
        "rolled-back bundle must match exact notarized build-605 CDHash"
    );
    assert_eq!(
        rolled_back_info["version_json"]["build_number"],
        json!(605),
        "rolled-back bundle must report build 605"
    );
    let support_hashes_after_rollback = snapshot_support_hashes();
    assert_eq!(
        support_hashes_after_rollback, support_hashes_before,
        "Application Support state must remain byte-identical after rollback"
    );

    // Clean up ~100+ MB temporary sandbox bundles while retaining compact JSON report evidence.
    std::fs::remove_dir_all(&sandbox_dir).expect("remove temporary sandbox app copies");
    assert!(!sandbox_dir.exists());

    let report = json!({
        "task": "T-347",
        "verified_at_utc": "2026-11-26",
        "github_repo": "jazzymiles/NeoFlash",
        "live_github_releases_check": {
            "endpoint": "https://api.github.com/repos/jazzymiles/NeoFlash/releases?per_page=20",
            "beta_from_pre_beta1": live_platforms,
            "beta_at_0_1_0_beta_1": "UpToDate (0.1.0-beta.1)",
            "stable_at_0_1_0_beta_1": "UpToDate (none)",
        },
        "sandbox_upgrade_and_rollback": {
            "initial_build_605": initial_605_info,
            "simulated_release_archive": {
                "name": release_zip_name,
                "bytes": zip_bytes_len,
                "sha256": zip_sha256,
            },
            "tampered_hash_fail_closed": {
                "rejected": true,
                "error_message": tampered_error_msg,
                "leftover_files_in_downloads": 0,
            },
            "verified_download_sha256": downloaded_sha,
            "upgraded_bundle": upgraded_info,
            "rolled_back_build_605": rolled_back_info,
            "application_support_preserved": {
                "before_sha256": support_hashes_before,
                "after_upgrade_sha256": support_hashes_after_upgrade,
                "after_rollback_sha256": support_hashes_after_rollback,
                "byte_identical": true,
            },
            "sandbox_temp_bundles_cleaned": true,
        }
    });
    let report_path = artifact_root.join("verification-report.json");
    std::fs::write(
        &report_path,
        serde_json::to_string_pretty(&report).unwrap() + "\n",
    )
    .expect("write verification-report.json");
}

// ---------------------------------------------------------------------------
// T-501: Self-update staging, verification, atomic swap, rollback & UI E2E
// ---------------------------------------------------------------------------

fn t501_crc32(bytes: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &b in bytes {
        crc ^= u32::from(b);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

struct SyntheticZipEntry<'a> {
    name: &'a str,
    data: &'a [u8],
    unix_mode: u32,
    use_deflate: bool,
    force_zip64: bool,
}

fn build_synthetic_zip(entries: &[SyntheticZipEntry<'_>]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut cd = Vec::new();

    for entry in entries {
        let lfh_offset = out.len() as u64;
        let name_bytes = entry.name.as_bytes();
        let crc = t501_crc32(entry.data);
        let uncomp_len = entry.data.len() as u64;
        let (method, comp_bytes): (u16, Vec<u8>) = if entry.use_deflate && !entry.data.is_empty() {
            let deflated = neoflash_core::as_binary::binary_transform(
                "compress",
                entry.data,
                "",
                "deflate",
                entry.data.len().saturating_add(4096).max(64),
            )
            .expect("deflate synthetic zip entry");
            (8, deflated)
        } else {
            (0, entry.data.to_vec())
        };
        let comp_len = comp_bytes.len() as u64;

        let mut extra = Vec::new();
        let (lfh_comp_32, lfh_uncomp_32, cd_lfh_32) = if entry.force_zip64 {
            extra.extend_from_slice(&0x0001u16.to_le_bytes());
            extra.extend_from_slice(&24u16.to_le_bytes());
            extra.extend_from_slice(&uncomp_len.to_le_bytes());
            extra.extend_from_slice(&comp_len.to_le_bytes());
            extra.extend_from_slice(&lfh_offset.to_le_bytes());
            (0xFFFF_FFFFu32, 0xFFFF_FFFFu32, 0xFFFF_FFFFu32)
        } else {
            (comp_len as u32, uncomp_len as u32, lfh_offset as u32)
        };

        // Local File Header
        out.extend_from_slice(b"PK\x03\x04");
        out.extend_from_slice(&45u16.to_le_bytes()); // version needed
        out.extend_from_slice(&0x0800u16.to_le_bytes()); // UTF-8 flag
        out.extend_from_slice(&method.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // mod time
        out.extend_from_slice(&0u16.to_le_bytes()); // mod date
        out.extend_from_slice(&crc.to_le_bytes());
        out.extend_from_slice(&lfh_comp_32.to_le_bytes());
        out.extend_from_slice(&lfh_uncomp_32.to_le_bytes());
        out.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
        out.extend_from_slice(&(extra.len() as u16).to_le_bytes());
        out.extend_from_slice(name_bytes);
        out.extend_from_slice(&extra);
        out.extend_from_slice(&comp_bytes);

        // Central Directory Header
        let external_attr = entry.unix_mode << 16;
        cd.extend_from_slice(b"PK\x01\x02");
        cd.extend_from_slice(&((3u16 << 8) | 45u16).to_le_bytes()); // Unix creator OS
        cd.extend_from_slice(&45u16.to_le_bytes());
        cd.extend_from_slice(&0x0800u16.to_le_bytes());
        cd.extend_from_slice(&method.to_le_bytes());
        cd.extend_from_slice(&0u16.to_le_bytes());
        cd.extend_from_slice(&0u16.to_le_bytes());
        cd.extend_from_slice(&crc.to_le_bytes());
        cd.extend_from_slice(&lfh_comp_32.to_le_bytes());
        cd.extend_from_slice(&lfh_uncomp_32.to_le_bytes());
        cd.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
        cd.extend_from_slice(&(extra.len() as u16).to_le_bytes());
        cd.extend_from_slice(&0u16.to_le_bytes()); // comment len
        cd.extend_from_slice(&0u16.to_le_bytes()); // disk start
        cd.extend_from_slice(&0u16.to_le_bytes()); // internal attr
        cd.extend_from_slice(&external_attr.to_le_bytes());
        cd.extend_from_slice(&cd_lfh_32.to_le_bytes());
        cd.extend_from_slice(name_bytes);
        cd.extend_from_slice(&extra);
    }

    let cd_offset = out.len() as u32;
    let cd_size = cd.len() as u32;
    out.extend_from_slice(&cd);

    // End of Central Directory
    out.extend_from_slice(b"PK\x05\x06");
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&cd_size.to_le_bytes());
    out.extend_from_slice(&cd_offset.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

fn build_synthetic_pe64_bytes(machine: u16, opt_magic: u16) -> Vec<u8> {
    let mut pe = vec![0u8; 512];
    pe[0] = b'M';
    pe[1] = b'Z';
    let e_lfanew: u32 = 0x80;
    pe[0x3c..0x40].copy_from_slice(&e_lfanew.to_le_bytes());
    let off = e_lfanew as usize;
    pe[off..off + 4].copy_from_slice(b"PE\0\0");
    pe[off + 4..off + 6].copy_from_slice(&machine.to_le_bytes());
    pe[off + 6..off + 8].copy_from_slice(&1u16.to_le_bytes()); // 1 section
    pe[off + 20..off + 22].copy_from_slice(&240u16.to_le_bytes()); // SizeOfOptionalHeader
    pe[off + 22..off + 24].copy_from_slice(&0x0022u16.to_le_bytes()); // EXECUTABLE_IMAGE | LARGE_ADDRESS_AWARE
    pe[off + 24..off + 26].copy_from_slice(&opt_magic.to_le_bytes());
    pe
}

fn build_synthetic_elf64_bytes(ei_class: u8, e_machine: u16) -> Vec<u8> {
    let mut elf = vec![0u8; 128];
    elf[0..4].copy_from_slice(b"\x7fELF");
    elf[4] = ei_class; // 2 = ELFCLASS64
    elf[5] = 1; // little endian
    elf[6] = 1; // EV_CURRENT
    elf[16..18].copy_from_slice(&3u16.to_le_bytes()); // ET_DYN PIE executable
    elf[18..20].copy_from_slice(&e_machine.to_le_bytes()); // 0x003E = EM_X86_64
    elf[20..24].copy_from_slice(&1u32.to_le_bytes());
    elf
}

fn build_synthetic_tar_gz(entries: &[(&str, u8, u32, &[u8])]) -> Vec<u8> {
    let mut tar = Vec::new();
    for &(name, typeflag, mode, payload) in entries {
        let mut hdr = [0u8; 512];
        let name_bytes = name.as_bytes();
        assert!(name_bytes.len() <= 100);
        hdr[0..name_bytes.len()].copy_from_slice(name_bytes);
        let mode_str = format!("{mode:07o}\0");
        hdr[100..108].copy_from_slice(mode_str.as_bytes());
        hdr[108..116].copy_from_slice(b"0000000\0");
        hdr[116..124].copy_from_slice(b"0000000\0");
        let size_len = if typeflag == b'0' { payload.len() } else { 0 };
        let size_str = format!("{size_len:011o}\0");
        hdr[124..136].copy_from_slice(size_str.as_bytes());
        hdr[136..148].copy_from_slice(b"00000000000\0");
        hdr[148..156].fill(b' ');
        hdr[156] = typeflag;
        if typeflag == b'2' {
            hdr[157..157 + payload.len()].copy_from_slice(payload);
        }
        hdr[257..263].copy_from_slice(b"ustar\0");
        hdr[263..265].copy_from_slice(b"00");
        let chksum: u32 = hdr.iter().map(|&b| u32::from(b)).sum();
        let chk_str = format!("{chksum:06o}\0 ");
        hdr[148..156].copy_from_slice(chk_str.as_bytes());
        tar.extend_from_slice(&hdr);
        if typeflag == b'0' && !payload.is_empty() {
            tar.extend_from_slice(payload);
            let rem = payload.len() % 512;
            if rem != 0 {
                tar.extend(std::iter::repeat_n(0u8, 512 - rem));
            }
        }
    }
    // Two 512-byte zero EOF blocks.
    tar.extend(std::iter::repeat_n(0u8, 1024));

    let deflated = neoflash_core::as_binary::binary_transform(
        "compress",
        &tar,
        "",
        "deflate",
        tar.len() + 4096,
    )
    .expect("deflate synthetic tar");
    let crc = t501_crc32(&tar);
    let isize_val = tar.len() as u32;

    let mut gz = Vec::with_capacity(10 + deflated.len() + 8);
    gz.extend_from_slice(&[0x1f, 0x8b, 0x08, 0x00, 0, 0, 0, 0, 0, 0x03]);
    gz.extend_from_slice(&deflated);
    gz.extend_from_slice(&crc.to_le_bytes());
    gz.extend_from_slice(&isize_val.to_le_bytes());
    gz
}

#[cfg(target_os = "macos")]
fn create_signed_mock_macos_app(
    app_dir: &std::path::Path,
    bundle_id: &str,
    short_version: &str,
    build_num: &str,
) {
    use std::os::unix::fs::PermissionsExt;
    let macos_dir = app_dir.join("Contents/MacOS");
    let res_dir = app_dir.join("Contents/Resources");
    std::fs::create_dir_all(&macos_dir).expect("create Contents/MacOS");
    std::fs::create_dir_all(&res_dir).expect("create Contents/Resources");

    // Copy a genuine system Mach-O binary so `/usr/bin/codesign --verify --deep --strict`
    // performs real Mach-O code signature validation.
    let exe_path = macos_dir.join("neoflash-editor");
    std::fs::copy("/usr/bin/true", &exe_path).expect("copy Mach-O stub");
    std::fs::set_permissions(&exe_path, std::fs::Permissions::from_mode(0o755))
        .expect("chmod 0755 Mach-O stub");

    let plist = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\"><dict>\n\
         <key>CFBundleName</key><string>NeoFlash</string>\n\
         <key>CFBundleIdentifier</key><string>{bundle_id}</string>\n\
         <key>CFBundleExecutable</key><string>neoflash-editor</string>\n\
         <key>CFBundlePackageType</key><string>APPL</string>\n\
         <key>CFBundleShortVersionString</key><string>{short_version}</string>\n\
         <key>CFBundleVersion</key><string>{build_num}</string>\n\
         </dict></plist>\n"
    );
    std::fs::write(app_dir.join("Contents/Info.plist"), plist).expect("write Info.plist");
    std::fs::write(
        res_dir.join("RELEASE-STAMP.txt"),
        format!("NeoFlash {short_version} ({build_num})\n"),
    )
    .expect("write RELEASE-STAMP.txt");

    let sign_out = std::process::Command::new("/usr/bin/codesign")
        .args(["--force", "--deep", "--sign", "-"])
        .arg(app_dir)
        .output()
        .expect("ad-hoc codesign mock NeoFlash.app");
    assert!(
        sign_out.status.success(),
        "ad-hoc codesign failed: {}",
        String::from_utf8_lossy(&sign_out.stderr)
    );
}

#[cfg(target_os = "macos")]
fn zip_macos_app_with_ditto(app_dir: &std::path::Path, zip_dest: &std::path::Path) {
    let out = std::process::Command::new("/usr/bin/ditto")
        .args(["-c", "-k", "--sequesterRsrc", "--keepParent"])
        .arg(app_dir)
        .arg(zip_dest)
        .output()
        .expect("ditto zip mock NeoFlash.app");
    assert!(
        out.status.success(),
        "ditto zip failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[cfg(target_os = "macos")]
#[test]
fn t501_macos_zip_staging_codesign_atomic_swap_rollback_and_readonly_guard() {
    use crate::updater::{
        apply_staged_bundle_swap, parse_macos_info_plist, prepare_verified_install,
    };
    use std::os::unix::fs::PermissionsExt;

    let sandbox = TempDirGuard::new("neoflash-t501-macos-e2e");
    let apps_dir = sandbox.0.join("Applications");
    let app_support = sandbox.0.join("Home/Library/Application Support/NeoFlash");
    std::fs::create_dir_all(&apps_dir).unwrap();
    std::fs::create_dir_all(app_support.join("Recovery")).unwrap();

    // Populate simulated Application Support user state and hash it before any update action.
    let prefs_file = app_support.join("workspace.json");
    let recovery_file = app_support.join("Recovery/active-session.nflash");
    std::fs::write(
        &prefs_file,
        br#"{"channel":"beta","recent":["scene1.nflash"]}"#,
    )
    .unwrap();
    std::fs::write(&recovery_file, Project::blank().to_json().unwrap()).unwrap();
    let prefs_sha_before = Sha256::digest_hex(&std::fs::read(&prefs_file).unwrap());
    let recovery_sha_before = Sha256::digest_hex(&std::fs::read(&recovery_file).unwrap());

    // 1. Create existing installed NeoFlash.app (v0.1.0-beta.1, com.neoflash.editor).
    let installed_app = apps_dir.join("NeoFlash.app");
    create_signed_mock_macos_app(&installed_app, "com.neoflash.editor", "0.1.0-beta.1", "605");

    // 2. Build & sign v0.1.0-beta.2 release ZIP and stage + verify via real `codesign --verify --deep --strict`.
    let build_stage = sandbox.0.join("build-beta2");
    let beta2_src_app = build_stage.join("NeoFlash.app");
    create_signed_mock_macos_app(&beta2_src_app, "com.neoflash.editor", "0.1.0-beta.2", "640");
    let beta2_zip = sandbox.0.join("NeoFlash-0.1.0-beta.2-macos-arm64.zip");
    zip_macos_app_with_ditto(&beta2_src_app, &beta2_zip);

    let prepared = prepare_verified_install(&beta2_zip, Some(&installed_app))
        .expect("stage & codesign-verify v0.1.0-beta.2 NeoFlash.app");
    assert_eq!(
        prepared.bundle_identifier.as_deref(),
        Some("com.neoflash.editor")
    );
    assert_eq!(prepared.bundle_version.as_deref(), Some("0.1.0-beta.2"));
    let staging_root_copy = prepared.staging_root.clone();
    assert!(staging_root_copy.is_dir());

    // 3. Atomic swap: v0.1.0-beta.1 -> v0.1.0-beta.2 with zero leftover .rollback and Application Support untouched.
    apply_staged_bundle_swap(&prepared.staged_payload_path, &installed_app, 42424)
        .expect("atomic swap v0.1.0-beta.1 -> v0.1.0-beta.2");
    drop(prepared);
    assert!(
        !staging_root_copy.exists(),
        "PreparedSelfUpdate Drop must clean up temporary staging directory"
    );
    assert!(!apps_dir.join("NeoFlash.app.rollback-42424").exists());

    let upgraded_plist =
        parse_macos_info_plist(&installed_app.join("Contents/Info.plist")).unwrap();
    assert_eq!(upgraded_plist.bundle_id, "com.neoflash.editor");
    assert_eq!(
        upgraded_plist.short_version.as_deref(),
        Some("0.1.0-beta.2")
    );
    let cs_after = std::process::Command::new("/usr/bin/codesign")
        .args(["--verify", "--deep", "--strict"])
        .arg(&installed_app)
        .status()
        .unwrap();
    assert!(
        cs_after.success(),
        "upgraded NeoFlash.app must pass strict codesign"
    );
    assert_eq!(
        Sha256::digest_hex(&std::fs::read(&prefs_file).unwrap()),
        prefs_sha_before
    );
    assert_eq!(
        Sha256::digest_hex(&std::fs::read(&recovery_file).unwrap()),
        recovery_sha_before
    );

    // 4. Fail-closed Bundle ID mismatch (`com.evil.other` and `org.neoflash.ui-studio`).
    for wrong_id in ["com.evil.other", "org.neoflash.ui-studio"] {
        let wrong_dir = sandbox.0.join(format!("wrong-{wrong_id}"));
        let wrong_app = wrong_dir.join("NeoFlash.app");
        create_signed_mock_macos_app(&wrong_app, wrong_id, "0.1.0-beta.3", "650");
        let wrong_zip = sandbox.0.join(format!("wrong-{wrong_id}.zip"));
        zip_macos_app_with_ditto(&wrong_app, &wrong_zip);
        let err = prepare_verified_install(&wrong_zip, Some(&installed_app))
            .expect_err("bundle ID mismatch must fail closed");
        assert!(
            err.contains("CFBundleIdentifier"),
            "expected CFBundleIdentifier rejection, got: {err}"
        );
    }

    // 5. Fail-closed tampered binary signature (flip bytes in signed Mach-O after signing).
    let tampered_dir = sandbox.0.join("tampered-sig");
    let tampered_app = tampered_dir.join("NeoFlash.app");
    create_signed_mock_macos_app(&tampered_app, "com.neoflash.editor", "0.1.0-beta.3", "651");
    std::fs::write(
        tampered_app.join("Contents/Resources/RELEASE-STAMP.txt"),
        b"TAMPERED AFTER CODESIGN\n",
    )
    .unwrap();
    let tampered_zip = sandbox.0.join("tampered-sig.zip");
    zip_macos_app_with_ditto(&tampered_app, &tampered_zip);
    let sig_err = prepare_verified_install(&tampered_zip, Some(&installed_app))
        .expect_err("tampered signed bundle must fail codesign --verify --deep --strict");
    assert!(
        sig_err.contains("Code signature verification failed"),
        "expected codesign failure, got: {sig_err}"
    );

    // 6. Fail-closed symlink escape inside NeoFlash.app (`../../../outside` and `/etc/hosts`).
    let sym_escape_zip = sandbox.0.join("symlink-escape.zip");
    let zip_bytes = build_synthetic_zip(&[
        SyntheticZipEntry {
            name: "NeoFlash.app/Contents/Info.plist",
            data: b"<plist version=\"1.0\"><dict><key>CFBundleIdentifier</key><string>com.neoflash.editor</string></dict></plist>",
            unix_mode: 0o100644,
            use_deflate: true,
            force_zip64: false,
        },
        SyntheticZipEntry {
            name: "NeoFlash.app/Contents/Resources/escape_link",
            data: b"../../../../etc/passwd",
            unix_mode: 0o120777,
            use_deflate: false,
            force_zip64: false,
        },
    ]);
    std::fs::write(&sym_escape_zip, zip_bytes).unwrap();
    let sym_err = prepare_verified_install(&sym_escape_zip, Some(&installed_app))
        .expect_err("escaping symlink inside NeoFlash.app must fail closed");
    assert!(
        sym_err.contains("symlink"),
        "expected symlink escape rejection, got: {sym_err}"
    );

    // 7. Atomic rollback when activating staged payload fails mid-swap (`NeoFlash.app.rollback-<pid>` -> `NeoFlash.app`).
    let missing_staged = sandbox.0.join("does-not-exist/NeoFlash.app");
    let swap_err = apply_staged_bundle_swap(&missing_staged, &installed_app, 77777)
        .expect_err("missing staged payload must trigger rollback");
    assert!(
        swap_err.contains("restored from rollback backup"),
        "expected rollback confirmation in error, got: {swap_err}"
    );
    assert!(installed_app.is_dir(), "installed_app must be restored");
    assert!(!apps_dir.join("NeoFlash.app.rollback-77777").exists());
    let after_rollback_plist =
        parse_macos_info_plist(&installed_app.join("Contents/Info.plist")).unwrap();
    assert_eq!(
        after_rollback_plist.short_version.as_deref(),
        Some("0.1.0-beta.2")
    );
    assert_eq!(
        Sha256::digest_hex(&std::fs::read(&prefs_file).unwrap()),
        prefs_sha_before
    );
    assert_eq!(
        Sha256::digest_hex(&std::fs::read(&recovery_file).unwrap()),
        recovery_sha_before
    );

    // 8. Read-only parent `/Applications` directory fails closed BEFORE quitting while keeping Reveal in Finder hint.
    if unsafe { libc::geteuid() } != 0 {
        let ro_apps = sandbox.0.join("ReadOnlyApplications");
        let ro_app = ro_apps.join("NeoFlash.app");
        create_signed_mock_macos_app(&ro_app, "com.neoflash.editor", "0.1.0-beta.1", "605");
        std::fs::set_permissions(&ro_apps, std::fs::Permissions::from_mode(0o555)).unwrap();
        let ro_res = prepare_verified_install(&beta2_zip, Some(&ro_app));
        std::fs::set_permissions(&ro_apps, std::fs::Permissions::from_mode(0o755)).unwrap();
        let ro_err =
            ro_res.expect_err("read-only Applications folder must fail closed before quit");
        assert!(
            ro_err.contains("Reveal in Finder") && ro_err.contains("not writable"),
            "expected actionable read-only error mentioning Reveal in Finder, got: {ro_err}"
        );
    }
}

#[test]
fn t501_windows_zip_pe64_linux_targz_elf64_staging_and_zip_slip_rejection() {
    use crate::updater::{
        apply_staged_bundle_swap, can_self_install_for_platform,
        prepare_verified_install_with_config,
    };

    let dir = TempDirGuard::new("neoflash-t501-cross-platform");

    // 1. Windows x64 ZIP (with ZIP64 headers matching scripts/package-portable.py) + PE32+ x64 validation + swap.
    let win_install_dir = dir.0.join("LocalAppData/Programs/NeoFlash");
    let win_appdata = dir.0.join("AppData/Roaming/NeoFlash");
    std::fs::create_dir_all(&win_install_dir).unwrap();
    std::fs::create_dir_all(&win_appdata).unwrap();
    std::fs::write(win_install_dir.join("neoflash-editor.exe"), b"OLD-WIN-EXE").unwrap();
    std::fs::write(
        win_install_dir.join("BUILD_METADATA.json"),
        br#"{"name":"NeoFlash","version":"0.1.0-beta.1"}"#,
    )
    .unwrap();
    std::fs::write(
        win_install_dir.join("PACKAGE-SHA256SUMS.txt"),
        format!("{}  neoflash-editor.exe\n", "0".repeat(64)),
    )
    .unwrap();
    let win_prefs_path = win_appdata.join("workspace.json");
    std::fs::write(&win_prefs_path, br#"{"theme":"dark"}"#).unwrap();

    let pe64_bytes = build_synthetic_pe64_bytes(0x8664, 0x020b);
    let win_zip_path = dir.0.join("NeoFlash-0.1.0-beta.2-windows-x64-preview.zip");
    let win_zip_bytes = build_synthetic_zip(&[
        SyntheticZipEntry {
            name: "NeoFlash-0.1.0-beta.2-windows-x64-preview/neoflash-editor.exe",
            data: &pe64_bytes,
            unix_mode: 0o100755,
            use_deflate: true,
            force_zip64: true,
        },
        SyntheticZipEntry {
            name: "NeoFlash-0.1.0-beta.2-windows-x64-preview/runtime/neoflash_web.js",
            data: b"// web runtime",
            unix_mode: 0o100644,
            use_deflate: true,
            force_zip64: true,
        },
    ]);
    std::fs::write(&win_zip_path, win_zip_bytes).unwrap();
    assert!(can_self_install_for_platform(
        &win_zip_path,
        PlatformTarget::WindowsX64
    ));
    assert!(!can_self_install_for_platform(
        std::path::Path::new("NeoFlash-0.1.0-windows-x64.msix"),
        PlatformTarget::WindowsX64
    ));

    let win_cfg = UpdaterConfig {
        platform: PlatformTarget::WindowsX64,
        install_target_override: Some(win_install_dir.clone()),
        ..Default::default()
    };
    let win_prepared = prepare_verified_install_with_config(&win_zip_path, None, &win_cfg)
        .expect("stage valid Windows x64 PE32+ zip");
    let ps1_helper = std::fs::read_to_string(win_prepared.helper_script_path.as_ref().unwrap())
        .expect("read generated Windows PowerShell swap helper");
    assert!(ps1_helper.contains(".incoming-"));
    assert!(
        ps1_helper
            .contains("Copy-Item -LiteralPath $Staged -Destination $Incoming -Recurse -Force")
    );
    assert!(ps1_helper.contains(".rollback-"));
    assert!(ps1_helper.contains("Move-Item -LiteralPath $Incoming -Destination $Target -Force"));
    assert!(ps1_helper.contains("neoflash-editor.exe"));

    apply_staged_bundle_swap(&win_prepared.staged_payload_path, &win_install_dir, 8888)
        .expect("swap Windows install dir");
    assert_eq!(
        std::fs::read(win_install_dir.join("neoflash-editor.exe")).unwrap(),
        pe64_bytes
    );
    assert!(win_install_dir.join("runtime/neoflash_web.js").is_file());
    assert_eq!(
        std::fs::read(&win_prefs_path).unwrap(),
        br#"{"theme":"dark"}"#
    );

    // Reject 32-bit x86 PE (`0x014c`) on WindowsX64.
    let pe32_bytes = build_synthetic_pe64_bytes(0x014c, 0x010b);
    let bad_pe_zip = dir.0.join("bad-pe32.zip");
    std::fs::write(
        &bad_pe_zip,
        build_synthetic_zip(&[SyntheticZipEntry {
            name: "neoflash-editor.exe",
            data: &pe32_bytes,
            unix_mode: 0o100755,
            use_deflate: true,
            force_zip64: false,
        }]),
    )
    .unwrap();
    let bad_pe_err = prepare_verified_install_with_config(&bad_pe_zip, None, &win_cfg)
        .expect_err("x86 32-bit PE must fail x64 PE64 check");
    assert!(bad_pe_err.contains("0x8664"), "got: {bad_pe_err}");

    // Reject zip-slip (`../../evil.dll`, `..\evil.dll`, `/abs/evil.dll`, `C:\evil.dll`) & symlinks.
    for malicious_name in [
        "../../evil.dll",
        "..\\..\\evil.dll",
        "/etc/evil.dll",
        "C:\\Windows\\evil.dll",
    ] {
        let slip_zip = dir.0.join("zip-slip.zip");
        std::fs::write(
            &slip_zip,
            build_synthetic_zip(&[SyntheticZipEntry {
                name: malicious_name,
                data: &pe64_bytes,
                unix_mode: 0o100644,
                use_deflate: false,
                force_zip64: false,
            }]),
        )
        .unwrap();
        let slip_err = prepare_verified_install_with_config(&slip_zip, None, &win_cfg).unwrap_err();
        assert!(
            slip_err.contains("Unsafe"),
            "expected zip-slip rejection for '{malicious_name}', got: {slip_err}"
        );
    }

    let win_symlink_zip = dir.0.join("win-symlink.zip");
    std::fs::write(
        &win_symlink_zip,
        build_synthetic_zip(&[SyntheticZipEntry {
            name: "NeoFlash/evil-link",
            data: b"../secret",
            unix_mode: 0o120777,
            use_deflate: false,
            force_zip64: false,
        }]),
    )
    .unwrap();
    assert!(
        prepare_verified_install_with_config(&win_symlink_zip, None, &win_cfg)
            .unwrap_err()
            .contains("symlink")
    );

    // 2. Linux x64 .tar.gz ELF64 validation, 0o755 mode, swap, AppImage & .deb handling, and tar-slip rejection.
    let linux_install_dir = dir.0.join("opt/neoflash");
    let linux_xdg_data = dir.0.join("local/share/NeoFlash");
    std::fs::create_dir_all(&linux_install_dir).unwrap();
    std::fs::create_dir_all(&linux_xdg_data).unwrap();
    std::fs::write(linux_install_dir.join("neoflash-editor"), b"OLD-LINUX-ELF").unwrap();
    std::fs::write(
        linux_install_dir.join("BUILD_METADATA.json"),
        br#"{"name":"NeoFlash","version":"0.1.0-beta.1"}"#,
    )
    .unwrap();
    std::fs::write(
        linux_install_dir.join("PACKAGE-SHA256SUMS.txt"),
        format!("{}  neoflash-editor\n", "0".repeat(64)),
    )
    .unwrap();
    std::fs::write(linux_xdg_data.join("workspace.json"), b"{}").unwrap();

    let elf64_bytes = build_synthetic_elf64_bytes(2, 0x003e);
    let linux_tgz_path = dir.0.join("NeoFlash-0.1.0-beta.2-linux-x64-preview.tar.gz");
    let linux_tgz_bytes = build_synthetic_tar_gz(&[
        (
            "NeoFlash-0.1.0-beta.2-linux-x64-preview/neoflash-editor",
            b'0',
            0o755,
            &elf64_bytes,
        ),
        (
            "NeoFlash-0.1.0-beta.2-linux-x64-preview/runtime/neoflash_web.js",
            b'0',
            0o644,
            b"// linux web runtime",
        ),
    ]);
    std::fs::write(&linux_tgz_path, linux_tgz_bytes).unwrap();

    assert!(can_self_install_for_platform(
        &linux_tgz_path,
        PlatformTarget::LinuxX64
    ));
    assert!(can_self_install_for_platform(
        std::path::Path::new("NeoFlash-0.1.0-linux-x86_64.AppImage"),
        PlatformTarget::LinuxX64
    ));
    assert!(!can_self_install_for_platform(
        std::path::Path::new("neoflash_0.1.0_amd64.deb"),
        PlatformTarget::LinuxX64
    ));

    let linux_cfg = UpdaterConfig {
        platform: PlatformTarget::LinuxX64,
        install_target_override: Some(linux_install_dir.clone()),
        ..Default::default()
    };
    let linux_prepared = prepare_verified_install_with_config(&linux_tgz_path, None, &linux_cfg)
        .expect("stage valid Linux ELF64 tar.gz");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(linux_prepared.staged_payload_path.join("neoflash-editor"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o755);
    }
    let sh_helper = std::fs::read_to_string(linux_prepared.helper_script_path.as_ref().unwrap())
        .expect("read generated POSIX sh swap helper");
    assert!(sh_helper.contains(".rollback-"));
    assert!(sh_helper.contains("neoflash-editor"));

    apply_staged_bundle_swap(
        &linux_prepared.staged_payload_path,
        &linux_install_dir,
        9999,
    )
    .expect("swap Linux install dir");
    assert_eq!(
        std::fs::read(linux_install_dir.join("neoflash-editor")).unwrap(),
        elf64_bytes
    );
    assert_eq!(
        std::fs::read(linux_xdg_data.join("workspace.json")).unwrap(),
        b"{}"
    );

    // Reject .deb self-install with clear apt message.
    let deb_path = dir.0.join("neoflash_0.1.0_amd64.deb");
    std::fs::write(&deb_path, b"!<arch>\ndebian-binary").unwrap();
    let deb_err = prepare_verified_install_with_config(&deb_path, None, &linux_cfg).unwrap_err();
    assert!(deb_err.contains("sudo apt install"), "got: {deb_err}");

    // Reject TAR path traversal (`../escape`) and TAR symlink (`typeflag == b'2'`).
    let tar_slip_path = dir.0.join("tar-slip.tar.gz");
    std::fs::write(
        &tar_slip_path,
        build_synthetic_tar_gz(&[("../escaped-binary", b'0', 0o755, &elf64_bytes)]),
    )
    .unwrap();
    assert!(
        prepare_verified_install_with_config(&tar_slip_path, None, &linux_cfg)
            .unwrap_err()
            .contains("Unsafe")
    );

    let tar_sym_path = dir.0.join("tar-symlink.tar.gz");
    std::fs::write(
        &tar_sym_path,
        build_synthetic_tar_gz(&[("NeoFlash/sym", b'2', 0o777, b"../../etc/shadow")]),
    )
    .unwrap();
    assert!(
        prepare_verified_install_with_config(&tar_sym_path, None, &linux_cfg)
            .unwrap_err()
            .contains("symlink")
    );
}

#[cfg(target_os = "macos")]
#[test]
fn t501_ui_install_and_relaunch_dirty_document_prompt_and_clean_swap() {
    use crate::updater::parse_macos_info_plist;

    let sandbox = TempDirGuard::new("neoflash-t501-ui-relaunch");
    let apps_dir = sandbox.0.join("Applications");
    std::fs::create_dir_all(&apps_dir).unwrap();
    let installed_app = apps_dir.join("NeoFlash.app");
    create_signed_mock_macos_app(&installed_app, "com.neoflash.editor", "0.1.0-beta.1", "605");

    let build_stage = sandbox.0.join("build-beta2");
    let beta2_app = build_stage.join("NeoFlash.app");
    create_signed_mock_macos_app(&beta2_app, "com.neoflash.editor", "0.1.0-beta.2", "640");
    let asset_name = "NeoFlash-0.1.0-beta.2-macos-arm64.zip";
    let release_zip_src = sandbox.0.join(asset_name);
    zip_macos_app_with_ditto(&beta2_app, &release_zip_src);
    let zip_bytes = std::fs::read(&release_zip_src).unwrap();
    let zip_sha = Sha256::digest_hex(&zip_bytes);

    let server = FixtureServer::start();
    let base = server.base_url.clone();
    let release_json = serde_json::json!([
        {
            "tag_name": "v0.1.0-beta.2",
            "html_url": format!("{base}/jazzymiles/NeoFlash/releases/tag/v0.1.0-beta.2"),
            "draft": false,
            "prerelease": true,
            "assets": [
                {
                    "name": asset_name,
                    "browser_download_url": format!("{base}/dl/{asset_name}"),
                    "size": zip_bytes.len() as u64
                },
                {
                    "name": "NeoFlash-0.1.0-beta.2-SHA256SUMS.txt",
                    "browser_download_url": format!("{base}/dl/SHA256SUMS.txt"),
                    "size": 128
                }
            ]
        }
    ]);
    server.set_route(
        "/repos/jazzymiles/NeoFlash/releases?per_page=20",
        FixtureResponse::Static {
            status: 200,
            headers: vec![("Content-Type".into(), "application/json".into())],
            body: release_json.to_string().into_bytes(),
        },
    );
    server.set_route(
        &format!("/dl/{asset_name}"),
        FixtureResponse::Static {
            status: 200,
            headers: vec![],
            body: zip_bytes,
        },
    );
    server.set_route(
        "/dl/SHA256SUMS.txt",
        FixtureResponse::Static {
            status: 200,
            headers: vec![],
            body: format!("{zip_sha}  {asset_name}\n").into_bytes(),
        },
    );

    let dl_dest = sandbox.0.join("Downloads").join(asset_name);
    std::fs::create_dir_all(dl_dest.parent().unwrap()).unwrap();

    let mut d = UpdaterUiDriver::new();
    d.editor.updater.config = UpdaterConfig {
        api_base_override: Some(base),
        current_version: "0.1.0-beta.1".into(),
        platform: PlatformTarget::MacosArm64,
        download_dest_override: Some(dl_dest.clone()),
        install_target_override: Some(installed_app.clone()),
        dry_run_relaunch: true,
        ..Default::default()
    };

    // Make the active document dirty so Install and Relaunch must guard unsaved work first!
    d.editor.transact(|ed| {
        ed.project.name = "UnsavedAnimation".into();
    });
    assert!(
        d.editor.dirty(),
        "document must be dirty before self-update"
    );

    d.click("Edit");
    d.click("Preferences… (⌘U)");
    d.click("Updates");
    d.click("Beta");
    d.click("Check now");
    let check_start = Instant::now();
    while d.editor.updater.is_checking() {
        assert!(check_start.elapsed() < Duration::from_secs(5));
        thread::sleep(Duration::from_millis(10));
        d.step(vec![]);
    }
    d.settle();

    d.click("Download");
    let dl_start = Instant::now();
    while d.editor.updater.is_downloading() {
        assert!(dl_start.elapsed() < Duration::from_secs(5));
        thread::sleep(Duration::from_millis(10));
        d.step(vec![]);
    }
    d.settle();

    assert!(d.find("Install and Relaunch").is_some());
    assert!(d.find("Reveal in Finder").is_some());

    // 1. Click Install and Relaunch with unsaved changes -> prompts "Save changes before continuing?",
    //    then click Cancel -> update swap MUST NOT run and staged dir is cleaned up!
    d.click("Install and Relaunch");
    assert!(!d.editor.allow_close);
    assert_eq!(d.editor.updater.committed_self_updates(), 0);
    assert!(
        d.texts()
            .iter()
            .any(|t| t.contains("UnsavedAnimation") && t.contains("will be lost")),
        "expected dirty-document confirmation prompt; texts={:#?}",
        d.texts()
    );
    assert!(d.find("Discard").is_some() && d.find("Save").is_some());
    d.click("Cancel");
    d.settle();
    assert!(!d.editor.allow_close);
    assert!(!d.editor.updater.has_staged_self_update());
    assert_eq!(d.editor.updater.committed_self_updates(), 0);
    assert_eq!(
        parse_macos_info_plist(&installed_app.join("Contents/Info.plist"))
            .unwrap()
            .short_version
            .as_deref(),
        Some("0.1.0-beta.1"),
        "cancelling dirty-document prompt must leave installed app untouched"
    );

    // 2. Re-open Preferences -> Updates, mutate an unconfirmed draft preference (`undo_levels = 77.0`
    //    without clicking Save), download/stage on Beta, click Install and Relaunch, then click Discard ->
    //    quit is confirmed, unconfirmed draft edits are NOT saved, and NeoFlash.app upgrades to 0.1.0-beta.2!
    d.click("Edit");
    d.click("Preferences… (⌘U)");
    if let Some(draft) = &mut d.editor.preferences {
        draft.undo_levels = 77.0;
    }
    d.click("Updates");
    d.click("Beta");
    d.click("Check now");
    let recheck_start = Instant::now();
    while d.editor.updater.is_checking() {
        assert!(recheck_start.elapsed() < Duration::from_secs(5));
        thread::sleep(Duration::from_millis(10));
        d.step(vec![]);
    }
    d.settle();
    d.click("Download");
    let redl_start = Instant::now();
    while d.editor.updater.is_downloading() {
        assert!(redl_start.elapsed() < Duration::from_secs(5));
        thread::sleep(Duration::from_millis(10));
        d.step(vec![]);
    }
    d.settle();
    d.click("Install and Relaunch");
    assert_ne!(
        d.editor.workspace.undo_levels, 77,
        "Install and Relaunch must never save unconfirmed Preferences draft edits"
    );
    assert_eq!(
        d.editor.workspace.updates.channel,
        UpdateChannel::Stable,
        "Install and Relaunch must not save unconfirmed channel draft edit either"
    );
    assert!(
        d.texts()
            .iter()
            .any(|t| t.contains("UnsavedAnimation") && t.contains("will be lost"))
    );
    d.click("Discard");
    d.settle();
    assert!(d.editor.allow_close);
    assert_eq!(d.editor.updater.committed_self_updates(), 1);
    assert_eq!(
        parse_macos_info_plist(&installed_app.join("Contents/Info.plist"))
            .unwrap()
            .short_version
            .as_deref(),
        Some("0.1.0-beta.2"),
        "confirming quit must atomically replace NeoFlash.app with 0.1.0-beta.2"
    );
}

fn build_large_zero_rle_gzip(repeat_258_count: usize) -> (Vec<u8>, u64) {
    // Valid RFC 1951 Fixed-Huffman block:
    //   BFINAL=1, BTYPE=01 (3 bits: 0b011)
    //   1x literal 0x00 (8 bits LSB-first: 0x0C)
    //   `repeat_258_count` x (len=258 sym 285 -> 8 bits 0xA3, dist=1 sym 0 -> 5 bits 0x00)
    //   EOB sym 256 (7 bits: 0)
    let total_uncomp: u64 = 1 + 258 * (repeat_258_count as u64);
    let mut deflate = Vec::with_capacity((repeat_258_count * 13) / 8 + 16);
    let mut bit_acc: u64 = 0;
    let mut bit_cnt: u8 = 0;
    let mut push_bits = |bits: u32, n: u8| {
        bit_acc |= u64::from(bits) << bit_cnt;
        bit_cnt += n;
        while bit_cnt >= 8 {
            deflate.push((bit_acc & 0xFF) as u8);
            bit_acc >>= 8;
            bit_cnt -= 8;
        }
    };
    push_bits(0b011, 3);
    push_bits(0x0C, 8);
    for _ in 0..repeat_258_count {
        push_bits(0x00A3, 13);
    }
    push_bits(0, 7);
    if bit_cnt > 0 {
        deflate.push((bit_acc & 0xFF) as u8);
    }

    // Compute exact IEEE CRC-32 for `total_uncomp` zero bytes via 64 KiB chunks.
    const TABLE: [u32; 256] = {
        let mut t = [0u32; 256];
        let mut i = 0usize;
        while i < 256 {
            let mut c = i as u32;
            let mut k = 0;
            while k < 8 {
                c = if (c & 1) != 0 {
                    (c >> 1) ^ 0xEDB8_8320
                } else {
                    c >> 1
                };
                k += 1;
            }
            t[i] = c;
            i += 1;
        }
        t
    };
    let mut crc: u32 = 0xFFFF_FFFF;
    let mut rem = total_uncomp;
    while rem > 0 {
        let step = rem.min(65_536) as usize;
        for _ in 0..step {
            crc = (crc >> 8) ^ TABLE[(crc as u8) as usize];
        }
        rem -= step as u64;
    }
    let final_crc = !crc;
    let isize_le = (total_uncomp as u32).to_le_bytes();

    let mut gz = Vec::with_capacity(10 + deflate.len() + 8);
    gz.extend_from_slice(&[0x1f, 0x8b, 0x08, 0x00, 0, 0, 0, 0, 0, 0x03]);
    gz.extend_from_slice(&deflate);
    gz.extend_from_slice(&final_crc.to_le_bytes());
    gz.extend_from_slice(&isize_le);
    (gz, total_uncomp)
}

#[test]
fn t501_audit_hardening_wipe_guard_zip_differential_large_stream_and_linux_assets() {
    use crate::updater::{
        MAX_ARCHIVE_TOTAL_BYTES, decompress_gzip_stream_bounded, inspect_zip_safety,
        prepare_verified_install_with_config, reject_dangerous_install_target,
        resolve_portable_exe_install_dir, select_linux_runtime_asset,
    };

    let dir = TempDirGuard::new("neoflash-t501-audit");

    // 1. Portable Directory Wipe Guard: unmarked directories & shared/system roots fail closed!
    let unmarked_downloads = dir.0.join("FakeDownloads");
    std::fs::create_dir_all(&unmarked_downloads).unwrap();
    let loose_exe = unmarked_downloads.join("neoflash-editor.exe");
    std::fs::write(&loose_exe, build_synthetic_pe64_bytes(0x8664, 0x020b)).unwrap();
    let unmarked_err = resolve_portable_exe_install_dir(&loose_exe, PlatformTarget::WindowsX64)
        .expect_err("unmarked directory without BUILD_METADATA.json + PACKAGE-SHA256SUMS.txt must be rejected");
    assert!(
        unmarked_err.contains("BUILD_METADATA.json")
            && unmarked_err.contains("PACKAGE-SHA256SUMS.txt"),
        "got: {unmarked_err}"
    );

    // Adding only one of the two markers is still rejected.
    std::fs::write(unmarked_downloads.join("BUILD_METADATA.json"), b"{}").unwrap();
    assert!(resolve_portable_exe_install_dir(&loose_exe, PlatformTarget::WindowsX64).is_err());

    // Adding both markers on an isolated folder succeeds.
    std::fs::write(
        unmarked_downloads.join("PACKAGE-SHA256SUMS.txt"),
        format!("{}  neoflash-editor.exe\n", "a".repeat(64)),
    )
    .unwrap();
    assert_eq!(
        resolve_portable_exe_install_dir(&loose_exe, PlatformTarget::WindowsX64).unwrap(),
        unmarked_downloads
    );

    // Dangerous system/shared roots are unconditionally rejected even via `install_target_override`.
    let pe_zip = dir.0.join("valid-win.zip");
    std::fs::write(
        &pe_zip,
        build_synthetic_zip(&[SyntheticZipEntry {
            name: "neoflash-editor.exe",
            data: &build_synthetic_pe64_bytes(0x8664, 0x020b),
            unix_mode: 0o100755,
            use_deflate: true,
            force_zip64: false,
        }]),
    )
    .unwrap();

    for bad_root in [
        PathBuf::from("/"),
        PathBuf::from("/usr"),
        PathBuf::from("/usr/bin"),
        PathBuf::from("/usr/local"),
        PathBuf::from("/usr/local/bin"),
        PathBuf::from("/bin"),
        PathBuf::from("/opt"),
        std::env::temp_dir(),
    ] {
        assert!(
            reject_dangerous_install_target(&bad_root, PlatformTarget::LinuxX64).is_err(),
            "expected dangerous target '{}' to be rejected",
            bad_root.display()
        );
    }
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        for user_shared in [
            home.clone(),
            home.join("Downloads"),
            home.join("Desktop"),
            home.join("Documents"),
            home.join(".local/bin"),
        ] {
            let cfg = UpdaterConfig {
                platform: PlatformTarget::WindowsX64,
                install_target_override: Some(user_shared.clone()),
                ..Default::default()
            };
            let err = prepare_verified_install_with_config(&pe_zip, None, &cfg)
                .expect_err("shared user folder must be rejected");
            assert!(
                err.contains("shared or system directory"),
                "expected shared directory rejection for '{}', got: {err}",
                user_shared.display()
            );
        }
    }

    // 2. ZIP Parser Differential: reject LFH/CD filename mismatch, 0x7075/0x6375 Unicode extra
    //    overrides, and unreferenced leading/gap LFH records!
    let mut mismatch_zip = build_synthetic_zip(&[SyntheticZipEntry {
        name: "NeoFlash/safe_name.txt",
        data: b"payload",
        unix_mode: 0o100644,
        use_deflate: false,
        force_zip64: false,
    }]);
    // Mutate LFH filename (`offset 30..30+22`) to `../../evil_escape!.txt` (same byte length 22)
    // while keeping Central Directory filename `NeoFlash/safe_name.txt` untouched!
    assert_eq!(&mismatch_zip[30..52], b"NeoFlash/safe_name.txt");
    mismatch_zip[30..52].copy_from_slice(b"../../evil_escape!.txt");
    let diff_err = inspect_zip_safety(&mismatch_zip, true)
        .expect_err("LFH vs CD filename mismatch must fail closed");
    assert!(
        diff_err.contains("Local File Header filename does not match Central Directory"),
        "got: {diff_err}"
    );

    // Inject Info-ZIP Unicode Path (`0x7075`) extra field into Central Directory.
    let base_zip = build_synthetic_zip(&[SyntheticZipEntry {
        name: "NeoFlash/file.txt",
        data: b"abc",
        unix_mode: 0o100644,
        use_deflate: false,
        force_zip64: false,
    }]);
    let cd_pos = base_zip
        .windows(4)
        .position(|w| w == b"PK\x01\x02")
        .expect("locate CD");
    let eocd_pos = base_zip
        .windows(4)
        .rposition(|w| w == b"PK\x05\x06")
        .expect("locate EOCD");
    let mut unicode_extra_zip = Vec::new();
    unicode_extra_zip.extend_from_slice(&base_zip[..cd_pos]);
    let new_cd_start = unicode_extra_zip.len();
    let mut cd_entry = base_zip[cd_pos..eocd_pos].to_vec();
    let up_extra = [0x75u8, 0x70, 0x06, 0x00, 0x01, 0, 0, 0, 0, b'x']; // tag 0x7075, len 6
    cd_entry[30..32].copy_from_slice(&(up_extra.len() as u16).to_le_bytes());
    cd_entry.extend_from_slice(&up_extra);
    let new_cd_size = cd_entry.len() as u32;
    unicode_extra_zip.extend_from_slice(&cd_entry);
    let mut eocd_rec = base_zip[eocd_pos..].to_vec();
    eocd_rec[12..16].copy_from_slice(&new_cd_size.to_le_bytes());
    eocd_rec[16..20].copy_from_slice(&(new_cd_start as u32).to_le_bytes());
    unicode_extra_zip.extend_from_slice(&eocd_rec);
    let up_err = inspect_zip_safety(&unicode_extra_zip, true)
        .expect_err("0x7075 Info-ZIP Unicode Path extra tag must fail closed");
    assert!(up_err.contains("0x7075"), "got: {up_err}");

    // Inject unreferenced leading LFH before the CD-referenced archive.
    let mut prepended_zip = Vec::new();
    let smuggled = build_synthetic_zip(&[SyntheticZipEntry {
        name: "smuggled.txt",
        data: b"hidden",
        unix_mode: 0o100644,
        use_deflate: false,
        force_zip64: false,
    }]);
    let smuggled_cd = smuggled
        .windows(4)
        .position(|w| w == b"PK\x01\x02")
        .unwrap();
    let lead_len = smuggled_cd;
    prepended_zip.extend_from_slice(&smuggled[..lead_len]);
    prepended_zip.extend_from_slice(&base_zip[..cd_pos]);
    let shifted_cd_offset = prepended_zip.len() as u32;
    let mut shifted_cd = base_zip[cd_pos..eocd_pos].to_vec();
    shifted_cd[42..46].copy_from_slice(&(lead_len as u32).to_le_bytes());
    prepended_zip.extend_from_slice(&shifted_cd);
    let mut shifted_eocd = base_zip[eocd_pos..].to_vec();
    shifted_eocd[16..20].copy_from_slice(&shifted_cd_offset.to_le_bytes());
    prepended_zip.extend_from_slice(&shifted_eocd);
    let gap_err = inspect_zip_safety(&prepended_zip, true)
        .expect_err("unreferenced leading LFH must fail closed");
    assert!(gap_err.contains("unreferenced"), "got: {gap_err}");

    // 3. Streaming GZIP >256 MiB (~258.35 MiB > old MAX_BINARY_BYTES) succeeds in constant memory
    //    and enforces streaming byte cap fail-closed!
    let (gz_258mib, expected_len) = build_large_zero_rle_gzip(1_050_000);
    assert!(
        expected_len > 256 * 1024 * 1024,
        "stream must exceed 256 MiB"
    );
    let decoded_len =
        decompress_gzip_stream_bounded(&gz_258mib, MAX_ARCHIVE_TOTAL_BYTES, std::io::sink())
            .expect("stream-decompress >256 MiB GZIP payload");
    assert_eq!(decoded_len, expected_len);

    let cap_err = decompress_gzip_stream_bounded(&gz_258mib, 250 * 1024 * 1024, std::io::sink())
        .expect_err("exceeding max_uncompressed cap must fail closed");
    assert!(cap_err.contains("safety limit"), "got: {cap_err}");

    // 4. Linux runtime asset preference (`APPIMAGE` -> .AppImage, portable -> .tar.gz, system -> .deb).
    let linux_assets = vec![
        GitHubAsset {
            name: "neoflash_0.1.0_amd64.deb".into(),
            browser_download_url: "https://github.com/jazzymiles/NeoFlash/releases/download/v0.1.0/neoflash_0.1.0_amd64.deb".into(),
            size: 100,
        },
        GitHubAsset {
            name: "NeoFlash-0.1.0-linux-x64-preview.tar.gz".into(),
            browser_download_url: "https://github.com/jazzymiles/NeoFlash/releases/download/v0.1.0/NeoFlash-0.1.0-linux-x64-preview.tar.gz".into(),
            size: 100,
        },
        GitHubAsset {
            name: "NeoFlash-0.1.0-linux-x86_64.AppImage".into(),
            browser_download_url: "https://github.com/jazzymiles/NeoFlash/releases/download/v0.1.0/NeoFlash-0.1.0-linux-x86_64.AppImage".into(),
            size: 100,
        },
    ];
    assert_eq!(
        select_linux_runtime_asset(
            &linux_assets,
            Some(std::ffi::OsStr::new("/tmp/NeoFlash.AppImage")),
            Some(std::path::Path::new("/usr/bin/neoflash-editor")),
        )
        .map(|a| a.name.as_str()),
        Some("NeoFlash-0.1.0-linux-x86_64.AppImage")
    );
    assert_eq!(
        select_linux_runtime_asset(
            &linux_assets,
            None,
            Some(std::path::Path::new("/home/alice/NeoFlash/neoflash-editor")),
        )
        .map(|a| a.name.as_str()),
        Some("NeoFlash-0.1.0-linux-x64-preview.tar.gz")
    );
    assert_eq!(
        select_linux_runtime_asset(
            &linux_assets,
            None,
            Some(std::path::Path::new("/usr/bin/neoflash")),
        )
        .map(|a| a.name.as_str()),
        Some("neoflash_0.1.0_amd64.deb")
    );
}
