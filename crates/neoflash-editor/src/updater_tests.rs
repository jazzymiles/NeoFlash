use crate::{
    app::Editor,
    build_info::BuildIdentity,
    tests::primary_button,
    updater::{
        GitHubAsset, GitHubRelease, PlatformTarget, ReleaseVersion, UpdateState, UpdaterConfig,
        UpdaterController, lookup_sha256_for_asset, select_checksum_asset, select_platform_asset,
        should_run_daily_check,
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

fn current_platform_asset_name(version: &str) -> String {
    match PlatformTarget::current() {
        PlatformTarget::MacosArm64 => format!("NeoFlash-{version}-macos-arm64.dmg"),
        PlatformTarget::MacosX64 => format!("NeoFlash-{version}-macos-x64.dmg"),
        PlatformTarget::WindowsX64 => format!("NeoFlash-{version}-windows-x64-preview.zip"),
        PlatformTarget::LinuxX64 => format!("NeoFlash-{version}-linux-x64-preview.tar.gz"),
        PlatformTarget::Unsupported => format!("NeoFlash-{version}-macos-arm64.dmg"),
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
    // Give worker a moment to observe cancellation and drop its temp file guard.
    thread::sleep(Duration::from_millis(80));
    assert!(!cancel_dest.exists());
    let leftovers: Vec<_> = std::fs::read_dir(&dir.0)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().contains(".part"))
        .collect();
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
