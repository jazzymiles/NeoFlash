//! End-to-end provider transport, NF2 workflow (`R-097` & `R-099`), redaction, undo and
//! per-document session isolation tests for `T-305`.

use crate::ai_connections::{
    AiConnectionError, AiConnectionStatus, AiPromptRequest, AiProviderKind, AiProviderProfile,
    AiStreamEvent, AiUntrustedProposal, CancellationToken, MAX_AI_RESPONSE_BYTES, MAX_ASSET_BYTES,
    MemorySecretStore, SecretStore, format_provider_request, run_streaming_request,
    test_provider_connection, with_test_loopback_http_override,
};
use crate::app::Editor;
use crate::commands::CommandId;
use crate::panels::ai_dialogs::{
    AiAnswerPhase, AiConnectionModal, AiConnectionModalTarget, ProposalCardState,
};
use crate::panels::function_search::results_for_context;
use crate::panels::preferences::{
    PAGE_AI, PAGE_PUBLISH, PAGE_SHORTCUTS, PAGE_UPDATES, PREFERENCE_PAGES,
};
use eframe::egui::{self, Event, Pos2, Rect, Vec2};
use neoflash_core::{
    Easing, Element, ElementKind, FrameScript, Project, ResourceUnit, Script, ScriptLanguage,
    TimelineVideoSpan, Transform,
};
use neoflash_export::ai_http::MAX_AI_HTTP_LINE_BYTES;
use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
struct CapturedHttpRequest {
    request_line: String,
    headers: Vec<(String, String)>,
    body: String,
}

impl CapturedHttpRequest {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

enum MockReply {
    Raw(Vec<u8>),
    SlowBody {
        head: String,
        chunks: Vec<(Duration, Vec<u8>)>,
        hold_open: Duration,
    },
}

struct LoopbackFixtureServer {
    origin: String,
    captured: Arc<Mutex<Vec<CapturedHttpRequest>>>,
    stop: Arc<std::sync::atomic::AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl LoopbackFixtureServer {
    fn start(replies: Vec<MockReply>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback fixture");
        listener
            .set_nonblocking(true)
            .expect("set_nonblocking on loopback fixture");
        let addr = listener.local_addr().expect("local_addr");
        let origin = format!("http://127.0.0.1:{}", addr.port());
        let queue = Arc::new(Mutex::new(VecDeque::from(replies)));
        let captured = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));

        let q_clone = Arc::clone(&queue);
        let cap_clone = Arc::clone(&captured);
        let stop_clone = Arc::clone(&stop);

        let handle = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(15);
            while !stop_clone.load(std::sync::atomic::Ordering::Acquire)
                && Instant::now() < deadline
            {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let _ = stream.set_nonblocking(false);
                        let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                        let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));

                        if let Some(req) = read_http_request(&mut stream) {
                            cap_clone.lock().unwrap().push(req);
                        }

                        let next = q_clone.lock().unwrap().pop_front();
                        match next {
                            Some(MockReply::Raw(bytes)) => {
                                let _ = stream.write_all(&bytes);
                                let _ = stream.flush();
                            }
                            Some(MockReply::SlowBody {
                                head,
                                chunks,
                                hold_open,
                            }) => {
                                let _ = stream.write_all(head.as_bytes());
                                let _ = stream.flush();
                                for (delay, bytes) in chunks {
                                    if stop_clone.load(std::sync::atomic::Ordering::Acquire) {
                                        break;
                                    }
                                    if !delay.is_zero() {
                                        std::thread::sleep(delay);
                                    }
                                    if stream.write_all(&bytes).is_err() {
                                        break;
                                    }
                                    let _ = stream.flush();
                                }
                                let hold_until = Instant::now() + hold_open;
                                while !stop_clone.load(std::sync::atomic::Ordering::Acquire)
                                    && Instant::now() < hold_until
                                {
                                    std::thread::sleep(Duration::from_millis(10));
                                }
                            }
                            None => {
                                let fallback = http_json_reply(500, r#"{"error":"exhausted"}"#);
                                let _ = stream.write_all(&fallback);
                                let _ = stream.flush();
                            }
                        }
                    }
                    Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                        if q_clone.lock().unwrap().is_empty() {
                            // Keep listening briefly in case tests assert unexpected extra calls,
                            // or exit when explicitly dropped.
                        }
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(_) => break,
                }
            }
        });

        Self {
            origin,
            captured,
            stop,
            handle: Some(handle),
        }
    }

    fn requests(&self) -> Vec<CapturedHttpRequest> {
        self.captured.lock().unwrap().clone()
    }
}

impl Drop for LoopbackFixtureServer {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Release);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

fn read_http_request(stream: &mut std::net::TcpStream) -> Option<CapturedHttpRequest> {
    let cloned = stream.try_clone().ok()?;
    let mut reader = BufReader::new(cloned);
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).ok()? == 0 {
        return None;
    }
    let request_line = request_line.trim_end_matches(['\r', '\n']).to_owned();
    let mut headers = Vec::new();
    let mut content_length = 0usize;
    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line).ok()?;
        if n == 0 || line == "\r\n" || line == "\n" {
            break;
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if let Some((name, value)) = trimmed.split_once(':') {
            let key = name.trim().to_owned();
            let val = value.trim().to_owned();
            if key.eq_ignore_ascii_case("content-length") {
                content_length = val.parse::<usize>().unwrap_or(0);
            }
            headers.push((key, val));
        }
    }
    let mut body_bytes = vec![0u8; content_length];
    if content_length > 0 {
        let _ = reader.read_exact(&mut body_bytes);
    }
    Some(CapturedHttpRequest {
        request_line,
        headers,
        body: String::from_utf8_lossy(&body_bytes).into_owned(),
    })
}

fn http_json_reply(status: u16, body: &str) -> Vec<u8> {
    http_reply_with_headers(status, "application/json", &[], body.as_bytes())
}

fn http_sse_reply(sse_body: &str) -> Vec<u8> {
    http_reply_with_headers(200, "text/event-stream", &[], sse_body.as_bytes())
}

fn http_reply_with_headers(
    status: u16,
    content_type: &str,
    extra_headers: &[(&str, &str)],
    body: &[u8],
) -> Vec<u8> {
    let reason = match status {
        200 => "OK",
        301 => "Moved Permanently",
        302 => "Found",
        307 => "Temporary Redirect",
        308 => "Permanent Redirect",
        401 => "Unauthorized",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        503 => "Service Unavailable",
        _ => "Response",
    };
    let mut head = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n",
        body.len()
    );
    for (k, v) in extra_headers {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    head.push_str("\r\n");
    let mut out = head.into_bytes();
    out.extend_from_slice(body);
    out
}

fn single_ui_frame(
    editor: &mut Editor,
    ctx: &egui::Context,
    time: f64,
    events: Vec<Event>,
) -> egui::FullOutput {
    ctx.run(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1920.0, 1080.0))),
            time: Some(time),
            events,
            ..Default::default()
        },
        |ctx| {
            editor.shortcuts(ctx);
            editor.normalize_selection();
            editor.menu(ctx);
            editor.status_bar(ctx);
            editor.bitmap_textures.begin_frame(&editor.project);
            editor.prepare_docked_panel_focus(ctx);
            if editor.has_document() {
                if !editor.panels_hidden {
                    editor.dock_grid(ctx);
                }
                editor.document_bar(ctx);
                editor.stage(ctx);
            } else {
                editor.no_document_view(ctx);
            }
            editor.draw_dialogs(ctx);
            crate::panels::dialog_helpers::record_end_of_frame_focus(ctx);
        },
    )
}

fn run_ui_frame(
    editor: &mut Editor,
    ctx: &egui::Context,
    time: f64,
    events: Vec<Event>,
) -> egui::FullOutput {
    if events.is_empty() {
        // Settle newly opened `egui::Modal` areas past their first-frame sizing pass.
        let _ = single_ui_frame(editor, ctx, time, vec![]);
        single_ui_frame(editor, ctx, time + 0.016, vec![])
    } else {
        single_ui_frame(editor, ctx, time, events)
    }
}

fn collect_shape_texts(shape: &egui::epaint::Shape, out: &mut Vec<String>) {
    match shape {
        egui::epaint::Shape::Text(t) => {
            out.push(t.galley.job.text.clone());
        }
        egui::epaint::Shape::Vec(shapes) => {
            for s in shapes {
                collect_shape_texts(s, out);
            }
        }
        _ => {}
    }
}

fn ax_labels(output: &egui::FullOutput) -> Vec<String> {
    let mut labels: Vec<String> = output
        .platform_output
        .accesskit_update
        .as_ref()
        .map(|u| {
            u.nodes
                .iter()
                .flat_map(|(_, n)| {
                    [n.label().map(str::to_owned), n.value().map(str::to_owned)]
                        .into_iter()
                        .flatten()
                })
                .collect()
        })
        .unwrap_or_default();
    for clipped in &output.shapes {
        collect_shape_texts(&clipped.shape, &mut labels);
    }
    labels
}

fn sample_prompt_request(profile_id: &str, stream: bool) -> AiPromptRequest {
    let project = Project::blank();
    let root_id = project.root.clone();
    let pack = crate::ai_connections::AiContextPack::from_editor_selection(
        &project,
        &root_id,
        0,
        &[],
        None,
    );
    AiPromptRequest {
        request_id: "req-fixture".into(),
        profile_id: profile_id.into(),
        model: None,
        system_prompt: "Return concise reviewable proposals.".into(),
        user_prompt: "Add a stop() call on frame 1.".into(),
        context_pack: pack,
        temperature: Some(0.1),
        max_output_tokens: 256,
        stream,
    }
}

#[test]
fn t305_all_four_providers_streaming_nonstreaming_and_test_connection() {
    let store = MemorySecretStore::new();
    store
        .set_secret("openai-p", "sk-openai-fixture-key-1234567890")
        .unwrap();
    store
        .set_secret("anthropic-p", "sk-ant-api03-fixture-key-1234567890")
        .unwrap();
    store
        .set_secret("gemini-p", "AIzaSyGeminiFixtureKey123456789012345")
        .unwrap();

    // 1. OpenAI: Test connection + Streaming SSE + Non-streaming JSON
    let openai_server = LoopbackFixtureServer::start(vec![
        MockReply::Raw(http_json_reply(200, r#"{"data":[{"id":"gpt-4.1"}]}"#)),
        MockReply::Raw(http_sse_reply(concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"Stop frame 1.\\n\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"```as3\\nstop();\\n```\"}}]}\n\n",
            "data: [DONE]\n\n"
        ))),
        MockReply::Raw(http_json_reply(
            200,
            r#"{"choices":[{"message":{"content":"Non-streaming OpenAI explanation."}}]}"#,
        )),
    ]);

    with_test_loopback_http_override(&openai_server.origin, || {
        let profile = AiProviderProfile::new(
            "openai-p",
            "OpenAI",
            AiProviderKind::OpenAi,
            &openai_server.origin,
            "gpt-4.1",
        );
        let msg = test_provider_connection(&profile, &store).expect("OpenAI test connection");
        assert_eq!(msg, "Connected · gpt-4.1");

        let mut deltas = String::new();
        let proposals = run_streaming_request(
            &profile,
            &sample_prompt_request("openai-p", true),
            &store,
            &CancellationToken::new(),
            |ev| {
                if let AiStreamEvent::TextDelta { delta, .. } = ev {
                    deltas.push_str(&delta);
                }
            },
        )
        .expect("OpenAI streaming");
        assert!(deltas.contains("stop();"));
        assert!(proposals.iter().any(|p| matches!(
            p,
            AiUntrustedProposal::CodeSnippetProposal { code, .. } if code.contains("stop();")
        )));

        let non_stream = run_streaming_request(
            &profile,
            &sample_prompt_request("openai-p", false),
            &store,
            &CancellationToken::new(),
            |_| {},
        )
        .expect("OpenAI non-streaming");
        assert!(matches!(
            non_stream.first(),
            Some(AiUntrustedProposal::ExplanationText { body, .. })
                if body.contains("Non-streaming OpenAI explanation.")
        ));
    });
    let openai_reqs = openai_server.requests();
    assert_eq!(openai_reqs.len(), 3);
    assert_eq!(openai_reqs[0].request_line, "GET /v1/models HTTP/1.1");
    assert_eq!(
        openai_reqs[0].header("authorization"),
        Some("Bearer sk-openai-fixture-key-1234567890")
    );
    assert_eq!(
        openai_reqs[1].request_line,
        "POST /v1/chat/completions HTTP/1.1"
    );
    assert_eq!(openai_reqs[1].header("accept"), Some("text/event-stream"));
    assert_eq!(openai_reqs[2].header("accept"), Some("application/json"));

    // 2. Anthropic: Test connection + Streaming SSE + Non-streaming JSON
    let anthropic_server = LoopbackFixtureServer::start(vec![
        MockReply::Raw(http_json_reply(
            200,
            r#"{"data":[{"id":"claude-sonnet-4-20250514"}]}"#,
        )),
        MockReply::Raw(http_sse_reply(concat!(
            "event: message_start\ndata: {\"type\":\"message_start\"}\n\n",
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"Anthropic stream ok\"}}\n\n",
            "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n"
        ))),
        MockReply::Raw(http_json_reply(
            200,
            r#"{"content":[{"type":"text","text":"Anthropic JSON ok"}]}"#,
        )),
    ]);

    with_test_loopback_http_override(&anthropic_server.origin, || {
        let profile = AiProviderProfile::new(
            "anthropic-p",
            "Anthropic",
            AiProviderKind::Anthropic,
            &anthropic_server.origin,
            "claude-sonnet-4-20250514",
        );
        let msg = test_provider_connection(&profile, &store).expect("Anthropic test connection");
        assert_eq!(msg, "Connected · claude-sonnet-4-20250514");

        let stream_res = run_streaming_request(
            &profile,
            &sample_prompt_request("anthropic-p", true),
            &store,
            &CancellationToken::new(),
            |_| {},
        )
        .expect("Anthropic streaming");
        assert!(matches!(
            stream_res.first(),
            Some(AiUntrustedProposal::ExplanationText { body, .. }) if body == "Anthropic stream ok"
        ));

        let json_res = run_streaming_request(
            &profile,
            &sample_prompt_request("anthropic-p", false),
            &store,
            &CancellationToken::new(),
            |_| {},
        )
        .expect("Anthropic non-streaming");
        assert!(matches!(
            json_res.first(),
            Some(AiUntrustedProposal::ExplanationText { body, .. }) if body == "Anthropic JSON ok"
        ));
    });
    let anthropic_reqs = anthropic_server.requests();
    assert_eq!(anthropic_reqs.len(), 3);
    assert_eq!(anthropic_reqs[0].request_line, "GET /v1/models HTTP/1.1");
    assert_eq!(anthropic_reqs[1].request_line, "POST /v1/messages HTTP/1.1");
    assert_eq!(
        anthropic_reqs[1].header("x-api-key"),
        Some("sk-ant-api03-fixture-key-1234567890")
    );
    assert_eq!(
        anthropic_reqs[1].header("anthropic-version"),
        Some("2023-06-01")
    );

    // 3. Google Gemini: Test connection + Streaming SSE + Non-streaming JSON (key ONLY in header!)
    let gemini_server = LoopbackFixtureServer::start(vec![
        MockReply::Raw(http_json_reply(200, r#"{"name":"models/gemini-2.5-pro"}"#)),
        MockReply::Raw(http_sse_reply(concat!(
            "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"Gemini \"}]}}]}\n\n",
            "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"SSE ok\"}]}}]}\n\n"
        ))),
        MockReply::Raw(http_json_reply(
            200,
            r#"{"candidates":[{"content":{"parts":[{"text":"Gemini JSON ok"}]}}]}"#,
        )),
    ]);

    with_test_loopback_http_override(&gemini_server.origin, || {
        let profile = AiProviderProfile::new(
            "gemini-p",
            "Google Gemini",
            AiProviderKind::Gemini,
            &gemini_server.origin,
            "gemini-2.5-pro",
        );
        let msg = test_provider_connection(&profile, &store).expect("Gemini test connection");
        assert_eq!(msg, "Connected · gemini-2.5-pro");

        let stream_res = run_streaming_request(
            &profile,
            &sample_prompt_request("gemini-p", true),
            &store,
            &CancellationToken::new(),
            |_| {},
        )
        .expect("Gemini streaming");
        assert!(matches!(
            stream_res.first(),
            Some(AiUntrustedProposal::ExplanationText { body, .. }) if body == "Gemini SSE ok"
        ));

        let json_res = run_streaming_request(
            &profile,
            &sample_prompt_request("gemini-p", false),
            &store,
            &CancellationToken::new(),
            |_| {},
        )
        .expect("Gemini non-streaming");
        assert!(matches!(
            json_res.first(),
            Some(AiUntrustedProposal::ExplanationText { body, .. }) if body == "Gemini JSON ok"
        ));
    });
    let gemini_reqs = gemini_server.requests();
    assert_eq!(gemini_reqs.len(), 3);
    assert_eq!(
        gemini_reqs[0].request_line,
        "GET /v1beta/models/gemini-2.5-pro HTTP/1.1"
    );
    assert_eq!(
        gemini_reqs[1].request_line,
        "POST /v1beta/models/gemini-2.5-pro:streamGenerateContent?alt=sse HTTP/1.1"
    );
    assert_eq!(
        gemini_reqs[2].request_line,
        "POST /v1beta/models/gemini-2.5-pro:generateContent HTTP/1.1"
    );
    for req in &gemini_reqs {
        assert!(
            !req.request_line.contains("key=")
                && !req
                    .request_line
                    .contains("AIzaSyGeminiFixtureKey123456789012345"),
            "Gemini API key must never appear in URL query: {}",
            req.request_line
        );
        assert_eq!(
            req.header("x-goog-api-key"),
            Some("AIzaSyGeminiFixtureKey123456789012345")
        );
    }

    // 4. Local OpenAI-compatible: Plain HTTP loopback allowed WITHOUT override and WITHOUT secret
    let local_server = LoopbackFixtureServer::start(vec![
        MockReply::Raw(http_json_reply(200, r#"{"data":[{"id":"qwen2.5-coder"}]}"#)),
        MockReply::Raw(http_sse_reply(concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"Local loopback ok\"}}]}\n\n",
            "data: [DONE]\n\n"
        ))),
        MockReply::Raw(http_json_reply(
            200,
            r#"{"choices":[{"message":{"content":"Local JSON ok"}}]}"#,
        )),
    ]);
    let local_profile = AiProviderProfile::new(
        "local-p",
        "Local Ollama",
        AiProviderKind::LocalOpenAiCompatible,
        &local_server.origin,
        "qwen2.5-coder",
    );
    assert_eq!(
        test_provider_connection(&local_profile, &store).expect("Local test connection"),
        "Connected · qwen2.5-coder"
    );
    let local_stream = run_streaming_request(
        &local_profile,
        &sample_prompt_request("local-p", true),
        &store,
        &CancellationToken::new(),
        |_| {},
    )
    .expect("Local streaming");
    assert!(matches!(
        local_stream.first(),
        Some(AiUntrustedProposal::ExplanationText { body, .. }) if body == "Local loopback ok"
    ));
    let local_json = run_streaming_request(
        &local_profile,
        &sample_prompt_request("local-p", false),
        &store,
        &CancellationToken::new(),
        |_| {},
    )
    .expect("Local non-streaming");
    assert!(matches!(
        local_json.first(),
        Some(AiUntrustedProposal::ExplanationText { body, .. }) if body == "Local JSON ok"
    ));
}

#[test]
fn t305_transport_security_redirects_downgrade_query_keys_and_edge_cases() {
    let store = MemorySecretStore::new();
    let leaked_key = "sk-live-secret-echoed-by-server-9988776655";
    store.set_secret("cloud", leaked_key).unwrap();

    // 1. Plaintext HTTP rejected for all 3 cloud providers outside test override (even loopback).
    for kind in [
        AiProviderKind::OpenAi,
        AiProviderKind::Anthropic,
        AiProviderKind::Gemini,
    ] {
        let plain = AiProviderProfile::new("cloud", "Cloud", kind, "http://127.0.0.1:9999", "m1");
        let err = run_streaming_request(
            &plain,
            &sample_prompt_request("cloud", true),
            &store,
            &CancellationToken::new(),
            |_| {},
        )
        .unwrap_err();
        assert!(matches!(err, AiConnectionError::InvalidEndpoint { .. }));
    }

    // 2. Query-string credentials rejected before any network traffic.
    for bad_url in [
        "https://generativelanguage.googleapis.com?key=AIzaBad",
        "https://api.openai.com?api_key=sk-bad",
        "http://127.0.0.1:11434?x-goog-api-key=bad",
    ] {
        let kind = if bad_url.starts_with("http://") {
            AiProviderKind::LocalOpenAiCompatible
        } else {
            AiProviderKind::Gemini
        };
        let prof = AiProviderProfile::new("cloud", "BadQuery", kind, bad_url, "m1");
        assert!(matches!(
            test_provider_connection(&prof, &store),
            Err(AiConnectionError::InvalidEndpoint { .. })
        ));
    }

    // 3. Cross-host redirect (302) AND HTTPS->HTTP downgrade redirect (307) are rejected and
    // NEVER followed to the canary server.
    let canary_target = LoopbackFixtureServer::start(vec![MockReply::Raw(http_json_reply(
        200,
        r#"{"choices":[{"message":{"content":"SHOULD_NEVER_FOLLOW"}}]}"#,
    ))]);
    let downgrade_location = format!("{}/v1/chat/completions", canary_target.origin);
    let redirect_server = LoopbackFixtureServer::start(vec![
        MockReply::Raw(http_reply_with_headers(
            302,
            "text/plain",
            &[("Location", "https://evil.example.test/v1/chat/completions")],
            b"redirecting cross-host",
        )),
        MockReply::Raw(http_reply_with_headers(
            307,
            "text/plain",
            &[("Location", downgrade_location.as_str())],
            b"redirecting downgrade",
        )),
    ]);
    with_test_loopback_http_override(&redirect_server.origin, || {
        let mut prof = AiProviderProfile::new(
            "cloud",
            "OpenAI",
            AiProviderKind::OpenAi,
            &redirect_server.origin,
            "gpt-4.1",
        );
        prof.max_retries = 2;
        let err_cross = run_streaming_request(
            &prof,
            &sample_prompt_request("cloud", false),
            &store,
            &CancellationToken::new(),
            |_| {},
        )
        .unwrap_err();
        assert!(
            matches!(err_cross, AiConnectionError::InvalidEndpoint { .. }),
            "expected redirect rejection, got {err_cross:?}"
        );

        let err_downgrade = run_streaming_request(
            &prof,
            &sample_prompt_request("cloud", true),
            &store,
            &CancellationToken::new(),
            |_| {},
        )
        .unwrap_err();
        assert!(
            matches!(err_downgrade, AiConnectionError::InvalidEndpoint { .. }),
            "expected downgrade redirect rejection, got {err_downgrade:?}"
        );
    });
    assert!(
        canary_target.requests().is_empty(),
        "ureq client must never follow redirects to secondary target"
    );

    // 4. HTTP 401 fails immediately without retrying and redacts echoed secrets.
    let auth_body = format!(r#"{{"error":"Invalid key {leaked_key}"}}"#);
    let auth_server =
        LoopbackFixtureServer::start(vec![MockReply::Raw(http_json_reply(401, &auth_body))]);
    with_test_loopback_http_override(&auth_server.origin, || {
        let mut prof = AiProviderProfile::new(
            "cloud",
            "OpenAI",
            AiProviderKind::OpenAi,
            &auth_server.origin,
            "gpt-4.1",
        );
        prof.max_retries = 3;
        let err = run_streaming_request(
            &prof,
            &sample_prompt_request("cloud", true),
            &store,
            &CancellationToken::new(),
            |_| {},
        )
        .unwrap_err();
        let msg = err.to_string();
        assert!(matches!(
            err,
            AiConnectionError::HttpStatus { status: 401, .. }
        ));
        assert!(
            !msg.contains(leaked_key),
            "echoed key must be redacted: {msg}"
        );
        assert!(msg.contains("[REDACTED_SECRET]"));
    });
    assert_eq!(auth_server.requests().len(), 1, "401 must not retry");

    // 5. HTTP 429 (with Retry-After) + HTTP 503 retry recovery.
    let retry_server = LoopbackFixtureServer::start(vec![
        MockReply::Raw(http_reply_with_headers(
            429,
            "application/json",
            &[("Retry-After-Ms", "30")],
            br#"{"error":"rate_limited"}"#,
        )),
        MockReply::Raw(http_json_reply(503, r#"{"error":"overloaded"}"#)),
        MockReply::Raw(http_sse_reply(concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"Recovered after 429 and 503\"}}]}\n\n",
            "data: [DONE]\n\n"
        ))),
    ]);
    with_test_loopback_http_override(&retry_server.origin, || {
        let mut prof = AiProviderProfile::new(
            "cloud",
            "OpenAI",
            AiProviderKind::OpenAi,
            &retry_server.origin,
            "gpt-4.1",
        );
        prof.max_retries = 2;
        let mut saw_rate_limit = false;
        let mut retry_reasons = Vec::new();
        let res = run_streaming_request(
            &prof,
            &sample_prompt_request("cloud", true),
            &store,
            &CancellationToken::new(),
            |ev| match ev {
                AiStreamEvent::RateLimited { retry_after_ms, .. } => {
                    saw_rate_limit = true;
                    assert_eq!(retry_after_ms, Some(30));
                }
                AiStreamEvent::RetryScheduled { reason, .. } => {
                    retry_reasons.push(reason);
                }
                _ => {}
            },
        )
        .expect("retry recovery");
        assert!(saw_rate_limit);
        assert_eq!(retry_reasons, vec!["HTTP 429", "HTTP 503"]);
        assert!(matches!(
            res.first(),
            Some(AiUntrustedProposal::ExplanationText { body, .. })
                if body == "Recovered after 429 and 503"
        ));
    });
    assert_eq!(retry_server.requests().len(), 3);

    // 6. Timeout handling.
    let timeout_server = LoopbackFixtureServer::start(vec![MockReply::SlowBody {
        head: "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\r\n".into(),
        chunks: vec![],
        hold_open: Duration::from_millis(450),
    }]);
    let mut timeout_prof = AiProviderProfile::new(
        "local-timeout",
        "Local",
        AiProviderKind::LocalOpenAiCompatible,
        &timeout_server.origin,
        "m1",
    );
    timeout_prof.timeout_ms = 60;
    timeout_prof.max_retries = 0;
    let timeout_err = run_streaming_request(
        &timeout_prof,
        &sample_prompt_request("local-timeout", true),
        &store,
        &CancellationToken::new(),
        |_| {},
    )
    .unwrap_err();
    assert!(
        matches!(timeout_err, AiConnectionError::Timeout { .. }),
        "expected Timeout, got {timeout_err:?}"
    );

    // 7. Malformed SSE JSON, oversized SSE line (> MAX_AI_HTTP_LINE_BYTES), and oversized
    // cumulative SSE stream (> MAX_AI_RESPONSE_BYTES).
    let oversized_line = format!("data: {}\n\n", "x".repeat(MAX_AI_HTTP_LINE_BYTES + 64));
    let chunk_payload = "a".repeat(256 * 1024);
    let chunk_event =
        format!("data: {{\"choices\":[{{\"delta\":{{\"content\":\"{chunk_payload}\"}}}}]}}\n\n");
    let repeat_count = (MAX_AI_RESPONSE_BYTES / chunk_payload.len()) + 2;
    let oversized_stream = chunk_event.repeat(repeat_count);

    let bad_sse_server = LoopbackFixtureServer::start(vec![
        MockReply::Raw(http_sse_reply("data: {not-valid-json\n\n")),
        MockReply::Raw(http_sse_reply(&oversized_line)),
        MockReply::Raw(http_sse_reply(&oversized_stream)),
    ]);
    let mut bad_prof = AiProviderProfile::new(
        "local-bad",
        "Local",
        AiProviderKind::LocalOpenAiCompatible,
        &bad_sse_server.origin,
        "m1",
    );
    bad_prof.max_retries = 0;
    for label in [
        "malformed_json",
        "oversized_line",
        "oversized_total_response",
    ] {
        let err = run_streaming_request(
            &bad_prof,
            &sample_prompt_request("local-bad", true),
            &store,
            &CancellationToken::new(),
            |_| {},
        )
        .unwrap_err();
        assert!(
            matches!(err, AiConnectionError::Protocol { .. }),
            "{label}: expected Protocol error, got {err:?}"
        );
    }
}

#[test]
fn t305_preferences_ai_crud_default_selection_and_ready_command_gating() {
    assert_eq!(PREFERENCE_PAGES.len(), 10);
    assert_eq!(PAGE_PUBLISH, 7);
    assert_eq!(PAGE_AI, 8);
    assert_eq!(PAGE_UPDATES, 9);
    assert_eq!(PREFERENCE_PAGES[PAGE_AI], "AI");

    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let mut editor = Editor::from_project(&ctx, Project::demo());

    // Hidden before any connection is Ready (`R-097` item 4).
    assert!(!editor.has_ready_ai_connection());
    assert!(!editor.open_send_to_ai());
    assert!(!results_for_context("Send to AI", true, false).contains(&CommandId::EditSendToAi));

    // Render Preferences ▸ AI empty state.
    editor.open_preferences(PAGE_AI);
    let out_empty = run_ui_frame(&mut editor, &ctx, 0.1, vec![]);
    let empty_labels = ax_labels(&out_empty);
    assert!(empty_labels.iter().any(|l| l == "No AI connections"));
    assert!(empty_labels.iter().any(|l| l == "Add connection…"));

    // VerifyShortcuts page also hides Send to AI while no connection is Ready.
    editor.open_preferences(PAGE_SHORTCUTS);
    let out_shortcuts_before = run_ui_frame(&mut editor, &ctx, 0.2, vec![]);
    assert!(
        !ax_labels(&out_shortcuts_before)
            .iter()
            .any(|l| l.contains("Send to AI")),
        "Shortcuts page must hide Send to AI until a connection is Ready"
    );

    // Open Add connection modal (`480 pt`) and check provider default URL auto-switching.
    editor.open_preferences(PAGE_AI);
    let draft_ai = editor.preferences.as_ref().unwrap().ai.clone();
    let mut modal = AiConnectionModal::new_add(&draft_ai, AiConnectionModalTarget::Preferences);
    assert_eq!(modal.base_url, "https://api.openai.com");
    modal.set_provider(AiProviderKind::Anthropic, &draft_ai);
    assert_eq!(modal.base_url, "https://api.anthropic.com");
    modal.set_provider(AiProviderKind::Gemini, &draft_ai);
    assert_eq!(modal.base_url, "https://generativelanguage.googleapis.com");
    modal.set_provider(AiProviderKind::LocalOpenAiCompatible, &draft_ai);
    assert_eq!(modal.base_url, "http://127.0.0.1:11434");

    // Custom Base URL must NOT be overwritten when switching provider.
    modal.base_url = "https://gateway.internal.example".into();
    modal.set_provider(AiProviderKind::OpenAi, &draft_ai);
    assert_eq!(modal.base_url, "https://gateway.internal.example");

    // Configure & test a real OpenAI profile against local fixture, then Save modal.
    let conn_server = LoopbackFixtureServer::start(vec![
        MockReply::Raw(http_json_reply(200, r#"{"data":[{"id":"gpt-4.1"}]}"#)),
        MockReply::Raw(http_json_reply(
            200,
            r#"{"data":[{"id":"claude-sonnet-4-20250514"}]}"#,
        )),
    ]);

    modal.display_name = "Studio OpenAI".into();
    modal.base_url = conn_server.origin.clone();
    modal.default_model = "gpt-4.1".into();
    modal.api_key_input = "sk-studio-openai-secret-111122223333".into();
    editor.ai_connection_modal = Some(modal.clone());
    let _ = run_ui_frame(&mut editor, &ctx, 0.3, vec![]);

    with_test_loopback_http_override(&conn_server.origin, || {
        modal.run_test(&editor.ai_manager);
    });
    assert_eq!(modal.test_outcome, Some(Ok("Connected · gpt-4.1".into())));
    let first_id = modal.profile_id.clone();
    editor.ai_connection_modal = None;
    editor.save_ai_connection_modal(modal);
    assert_eq!(
        editor.ai_manager.profile_status(
            editor
                .preferences
                .as_ref()
                .unwrap()
                .ai
                .profile(&first_id)
                .unwrap()
        ),
        AiConnectionStatus::Ready
    );

    // Add a second profile (Anthropic): verify NoKey -> NotTested -> Ready transitions.
    let draft_ai_2 = editor.preferences.as_ref().unwrap().ai.clone();
    let mut modal2 = AiConnectionModal::new_add(&draft_ai_2, AiConnectionModalTarget::Preferences);
    modal2.set_provider(AiProviderKind::Anthropic, &draft_ai_2);
    modal2.display_name = "Studio Anthropic".into();
    modal2.base_url = conn_server.origin.clone();
    let second_id = modal2.profile_id.clone();
    let profile_no_key = modal2.build_profile();
    assert_eq!(
        editor.ai_manager.profile_status(&profile_no_key),
        AiConnectionStatus::NoKey
    );

    modal2.api_key_input = "sk-ant-api03-studio-secret-444455556666".into();
    editor.save_ai_connection_modal(modal2);
    let saved_p2 = editor
        .preferences
        .as_ref()
        .unwrap()
        .ai
        .profile(&second_id)
        .unwrap()
        .clone();
    assert_eq!(
        editor.ai_manager.profile_status(&saved_p2),
        AiConnectionStatus::NotTested
    );

    // Edit modal never exposes stored key in `api_key_input`, and blank input preserves the key.
    let mut edit_modal = AiConnectionModal::new_edit(
        &saved_p2,
        editor.ai_manager.has_secret(&second_id),
        AiConnectionModalTarget::Preferences,
    );
    assert!(edit_modal.api_key_input.is_empty());
    assert!(edit_modal.has_stored_key);
    with_test_loopback_http_override(&conn_server.origin, || {
        edit_modal.run_test(&editor.ai_manager);
    });
    assert_eq!(
        edit_modal.test_outcome,
        Some(Ok("Connected · claude-sonnet-4-6".into()))
    );
    editor.save_ai_connection_modal(edit_modal);

    // Select second profile as Default connection and Save Preferences.
    {
        let draft = editor.preferences.as_mut().unwrap();
        draft.ai.default_profile_id = Some(second_id.clone());
    }
    let draft_to_save = editor.preferences.take().unwrap();
    editor.save_preferences(&ctx, &draft_to_save);

    assert!(editor.has_ready_ai_connection());
    assert!(results_for_context("Send to AI", true, true).contains(&CommandId::EditSendToAi));
    assert!(editor.open_send_to_ai());
    assert_eq!(
        editor.ai_session.send_dialog.as_ref().unwrap().profile_id,
        second_id,
        "Send to AI must default to the configured Default connection"
    );
    editor.ai_session.cancel_and_clear();

    // Removing a profile in Preferences draft then cancelling keeps workspace + secret intact;
    // saving deletes the removed profile's secret from SecretStore.
    editor.open_preferences(PAGE_AI);
    editor
        .preferences
        .as_mut()
        .unwrap()
        .ai
        .remove_profile(&first_id);
    editor
        .preferences
        .as_mut()
        .unwrap()
        .removed_ai_secrets
        .push(first_id.clone());
    editor.preferences = None; // Cancel
    assert!(editor.workspace.ai.profile(&first_id).is_some());
    assert!(editor.ai_manager.has_secret(&first_id));

    editor.open_preferences(PAGE_AI);
    editor
        .preferences
        .as_mut()
        .unwrap()
        .ai
        .remove_profile(&first_id);
    editor
        .preferences
        .as_mut()
        .unwrap()
        .removed_ai_secrets
        .push(first_id.clone());
    let draft_after_remove = editor.preferences.take().unwrap();
    editor.save_preferences(&ctx, &draft_after_remove);
    assert!(editor.workspace.ai.profile(&first_id).is_none());
    assert!(!editor.ai_manager.has_secret(&first_id));
    assert_eq!(
        editor.workspace.ai.default_profile_id.as_deref(),
        Some(second_id.as_str())
    );
}

#[test]
fn t305_zero_secrets_leak_across_prefs_project_recovery_output_and_debug() {
    let ctx = egui::Context::default();
    let mut editor = Editor::from_project(&ctx, Project::demo());

    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/artifacts/t305-zero-leak");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    editor.recovery.set_dir(Some(dir.join("Recovery")));
    editor.workspace.general.autosave = true;
    editor.workspace.general.autosave_minutes = 1;

    let openai_secret = "sk-proj-UltraSecretOpenAiKey998877665544";
    let anthropic_secret = "sk-ant-api03-UltraSecretAnthropicKey99887766";
    let gemini_secret = "AIzaSyUltraSecretGeminiKey9988776655443322";
    let all_secrets = [openai_secret, anthropic_secret, gemini_secret];

    let fail_body = format!(
        r#"{{"error":"Rejected credentials {openai_secret} / {anthropic_secret} / {gemini_secret}"}}"#
    );
    let server =
        LoopbackFixtureServer::start(vec![MockReply::Raw(http_json_reply(401, &fail_body))]);

    for (id, kind, secret) in [
        ("p-oa", AiProviderKind::OpenAi, openai_secret),
        ("p-an", AiProviderKind::Anthropic, anthropic_secret),
        ("p-gm", AiProviderKind::Gemini, gemini_secret),
    ] {
        let mut p = AiProviderProfile::new(id, id, kind, &server.origin, "model-x");
        p.max_retries = 0;
        editor.workspace.ai.upsert_profile(p);
        editor.ai_manager.set_secret(id, secret).unwrap();
        editor.ai_manager.set_test_state(
            id,
            crate::ai_connections::AiProfileTestState::Connected {
                model: "model-x".into(),
            },
        );
    }

    // Dirty the project, save `.nflash`, and write crash recovery snapshot.
    editor.transact_named("Rename project", |ed| {
        ed.project.name = "Confidential Project".into();
    });
    editor.tick_autosave(0.0, 1_000);
    editor.tick_autosave(70.0, 1_070);
    let saved_nflash = dir.join("confidential.nflash");
    assert!(editor.save_to_path(saved_nflash.clone()));

    // Re-dirty so Recovery/ also holds active `.nflash` + `.json` snapshot files on disk.
    editor.transact_named("Second edit", |ed| {
        ed.project.name = "Confidential Project v2".into();
    });
    editor.tick_autosave(140.0, 1_140);
    editor.tick_autosave(210.0, 1_210);

    // Run a failing request that echoes all 3 secrets in its HTTP 401 body.
    assert!(editor.open_send_to_ai());
    editor.ai_session.send_dialog.as_mut().unwrap().request_text =
        format!("Explain frame 1 (note: {openai_secret})");
    with_test_loopback_http_override(&server.origin, || {
        assert!(editor.submit_send_to_ai());
        let deadline = Instant::now() + Duration::from_secs(3);
        while editor.ai_session.worker.is_some() && Instant::now() < deadline {
            editor.poll_ai_session(&ctx);
            std::thread::sleep(Duration::from_millis(5));
        }
    });
    assert!(matches!(
        editor.ai_session.answer_dialog.as_ref().map(|a| &a.phase),
        Some(AiAnswerPhase::Failed { .. })
    ));

    let mut modal_with_typed_secret =
        AiConnectionModal::new_add(&editor.workspace.ai, AiConnectionModalTarget::Workspace);
    modal_with_typed_secret.api_key_input = openai_secret.into();

    let formatted = with_test_loopback_http_override(&server.origin, || {
        format_provider_request(
            editor.workspace.ai.profile("p-oa").unwrap(),
            &sample_prompt_request("p-oa", true),
            Some(openai_secret),
        )
        .unwrap()
    });

    let mut surfaces: Vec<(&str, String)> = vec![
        (
            "WorkspacePrefs JSON",
            serde_json::to_string(&editor.workspace).unwrap(),
        ),
        (
            "Project JSON",
            serde_json::to_string(&editor.project).unwrap(),
        ),
        (
            "Saved .nflash",
            std::fs::read_to_string(&saved_nflash).unwrap(),
        ),
        ("Editor output", format!("{:?}", editor.output)),
        (
            "AiConnectionManager Debug",
            format!("{:?}", editor.ai_manager),
        ),
        (
            "AiConnectionModal Debug",
            format!("{modal_with_typed_secret:?}"),
        ),
        ("FormattedProviderRequest Debug", format!("{formatted:?}")),
        (
            "AiAnswerDialog Debug",
            format!("{:?}", editor.ai_session.answer_dialog),
        ),
    ];

    let recovery_dir = dir.join("Recovery");
    let mut recovery_count = 0usize;
    if let Ok(entries) = std::fs::read_dir(&recovery_dir) {
        for entry in entries.flatten() {
            let bytes = std::fs::read(entry.path()).unwrap_or_default();
            surfaces.push((
                "Recovery snapshot file",
                String::from_utf8_lossy(&bytes).into_owned(),
            ));
            recovery_count += 1;
        }
    }
    assert!(
        recovery_count >= 1,
        "expected crash recovery files to be written"
    );

    for (surface_name, text) in &surfaces {
        for secret in all_secrets {
            assert!(
                !text.contains(secret),
                "Secret leaked in {surface_name}: found raw secret in {text}"
            );
        }
    }
}

#[test]
fn t305_send_to_ai_consent_pack_omissions_and_checkbox_filtering() {
    let ctx = egui::Context::default();
    ctx.enable_accesskit();

    let mut project = Project::blank();
    project.name = "Space Opera".into();
    project.symbols[0].name = "HeroClip".into();
    project.symbols[0].timeline.frame_count = 5;
    // Ensure keyframe at frame index 2 (1-based frame 3).
    project.symbols[0].timeline.layers[0].keyframes[0]
        .elements
        .push(Element {
            id: "el-ship".into(),
            name: "ship_sprite".into(),
            transform: Transform {
                x: 120.0,
                y: 240.0,
                ..Default::default()
            },
            kind: ElementKind::Bitmap {
                resource_id: "res-png".into(),
                width: 32,
                height: 32,
                smoothing: false,
            },
            ..Default::default()
        });
    project.symbols[0].timeline.layers[0].keyframes[0].video =
        Some(TimelineVideoSpan::embedded("res-vid"));
    project.symbols[0].timeline.scripts.push(FrameScript {
        frame: 0,
        script: Script {
            language: ScriptLanguage::ActionScript3,
            source: "const tok = \"Bearer inline_secret_token_987654\";\nstop();\n".into(),
        },
    });
    project
        .resources
        .push(ResourceUnit::from_bytes("res-png", "ship.png", "image/png", &[1, 2, 3, 4]).unwrap());
    project.resources.push(
        ResourceUnit::from_bytes("res-vid", "cutscene.mp4", "video/mp4", &[5, 6, 7, 8]).unwrap(),
    );
    project.resources.push(
        ResourceUnit::from_bytes(
            "res-huge",
            "atlas.png",
            "image/png",
            &vec![0u8; MAX_ASSET_BYTES + 2048],
        )
        .unwrap(),
    );

    let mut editor = Editor::from_project(&ctx, project);
    editor.select_all();
    editor.library_selected = Some("res-huge".into());

    let server = LoopbackFixtureServer::start(vec![
        MockReply::Raw(http_sse_reply(
            "data: {\"choices\":[{\"delta\":{\"content\":\"First reply\"}}]}\n\ndata: [DONE]\n\n",
        )),
        MockReply::Raw(http_sse_reply(
            "data: {\"choices\":[{\"delta\":{\"content\":\"Second reply\"}}]}\n\ndata: [DONE]\n\n",
        )),
    ]);
    let local_profile = AiProviderProfile::new(
        "local-consent",
        "Local Ollama",
        AiProviderKind::LocalOpenAiCompatible,
        &server.origin,
        "qwen2.5-coder",
    );
    editor.workspace.ai.upsert_profile(local_profile);
    editor.ai_manager.set_test_state(
        "local-consent",
        crate::ai_connections::AiProfileTestState::Connected {
            model: "qwen2.5-coder".into(),
        },
    );

    // Open Send to AI (`560 pt`), verify pack + omissions, and verify Cancel sends 0 requests.
    assert!(editor.open_send_to_ai());
    let out = run_ui_frame(&mut editor, &ctx, 0.1, vec![]);
    let labels = ax_labels(&out);
    assert!(labels.iter().any(|l| l == "Send to AI"));
    assert!(labels.iter().any(|l| l.contains("What will be sent")));

    let omitted = editor
        .ai_session
        .send_dialog
        .as_ref()
        .unwrap()
        .context_pack
        .omitted_items_note()
        .expect("omitted assets note");
    assert!(
        omitted.contains("cutscene.mp4 (video is not an allowed type)"),
        "{omitted}"
    );
    assert!(
        omitted.contains("atlas.png (larger than 64 KB)"),
        "{omitted}"
    );

    // Press Escape to cancel Send to AI -> zero network calls!
    let _ = run_ui_frame(
        &mut editor,
        &ctx,
        0.2,
        vec![Event::Key {
            key: egui::Key::Escape,
            physical_key: Some(egui::Key::Escape),
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    assert!(editor.ai_session.send_dialog.is_none());
    assert_eq!(
        server.requests().len(),
        0,
        "Cancel must never transmit data"
    );

    // Reopen, untick `script_excerpt` and `asset_list`, send, and verify stripped HTTP payload.
    assert!(editor.open_send_to_ai());
    {
        let dlg = editor.ai_session.send_dialog.as_mut().unwrap();
        dlg.request_text = "Inspect the selected sprite.".into();
        dlg.selection.script_excerpt = false;
        dlg.selection.asset_list = false;
    }
    assert!(editor.submit_send_to_ai());
    let deadline = Instant::now() + Duration::from_secs(3);
    while editor.ai_session.worker.is_some() && Instant::now() < deadline {
        editor.poll_ai_session(&ctx);
        std::thread::sleep(Duration::from_millis(5));
    }

    let req1_body = &server.requests()[0].body;
    assert!(req1_body.contains("Space Opera"), "{req1_body}");
    assert!(req1_body.contains("HeroClip"), "{req1_body}");
    assert!(req1_body.contains("ship_sprite"), "{req1_body}");
    assert!(
        !req1_body.contains("script_excerpt") && !req1_body.contains("stop();"),
        "unticked script_excerpt must be stripped before transmission: {req1_body}"
    );
    assert!(
        !req1_body.contains("selected_assets") && !req1_body.contains("ship.png"),
        "unticked asset_list must be stripped before transmission: {req1_body}"
    );

    // Reopen, untick `project_and_symbol` and `selected_elements`, keep `script_excerpt` ticked.
    assert!(editor.open_send_to_ai());
    {
        let dlg = editor.ai_session.send_dialog.as_mut().unwrap();
        dlg.request_text = "Check script only.".into();
        dlg.selection.project_and_symbol = false;
        dlg.selection.selected_elements = false;
        dlg.selection.script_excerpt = true;
        dlg.selection.asset_list = false;
    }
    assert!(editor.submit_send_to_ai());
    let deadline = Instant::now() + Duration::from_secs(3);
    while editor.ai_session.worker.is_some() && Instant::now() < deadline {
        editor.poll_ai_session(&ctx);
        std::thread::sleep(Duration::from_millis(5));
    }

    let req2_body = &server.requests()[1].body;
    assert!(
        !req2_body.contains("Space Opera") && !req2_body.contains("ship_sprite"),
        "unticked project/elements must be stripped: {req2_body}"
    );
    assert!(req2_body.contains("script_excerpt"), "{req2_body}");
    assert!(
        !req2_body.contains("inline_secret_token_987654")
            && req2_body.contains("[REDACTED_SECRET]"),
        "inline Bearer token inside script excerpt must be redacted: {req2_body}"
    );
}

#[test]
fn t305_ai_answer_streaming_stop_proposal_validation_apply_and_single_step_undo() {
    let ctx = egui::Context::default();
    ctx.enable_accesskit();

    let mut project = Project::blank();
    project.symbols[0].timeline.frame_count = 20;
    project.symbols[0].timeline.layers[0].name = "Layer 1".into();
    project.symbols[0].timeline.scripts.push(FrameScript {
        frame: 0,
        script: Script {
            language: ScriptLanguage::ActionScript3,
            source: "// header\n// footer\n".into(),
        },
    });

    let mut editor = Editor::from_project(&ctx, project);
    let baseline_project = editor.project.clone();

    let proposal_response = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"Use stop() on frame 1 and ease Layer 1.\\n\\n\"}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\"```json\\n[\\n",
        "  {\\\"proposal_kind\\\":\\\"code_snippet_proposal\\\",\\\"language\\\":\\\"ActionScript3\\\",\\\"target_hint\\\":\\\"Frame script\\\",\\\"code\\\":\\\"stop();\\\\n\\\",\\\"explanation\\\":\\\"Halt main timeline on frame 1.\\\"},\\n",
        "  {\\\"proposal_kind\\\":\\\"keyframe_timing_proposal\\\",\\\"layer_name\\\":\\\"Layer 1\\\",\\\"start_frame\\\":1,\\\"end_frame\\\":12,\\\"easing_hint\\\":\\\"ease_out\\\",\\\"notes\\\":\\\"Decelerate smoothly.\\\"},\\n",
        "  {\\\"proposal_kind\\\":\\\"keyframe_timing_proposal\\\",\\\"layer_name\\\":\\\"Ghost Layer\\\",\\\"start_frame\\\":1,\\\"end_frame\\\":12,\\\"easing_hint\\\":\\\"linear\\\",\\\"notes\\\":\\\"Missing layer test.\\\"}\\n",
        "]\\n```\"}}]}\n\n",
        "data: [DONE]\n\n"
    );

    let server = LoopbackFixtureServer::start(vec![
        MockReply::SlowBody {
            head: "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\r\n".into(),
            chunks: vec![(
                Duration::ZERO,
                b"data: {\"choices\":[{\"delta\":{\"content\":\"Partial answer before stop\"}}]}\n\n"
                    .to_vec(),
            )],
            hold_open: Duration::from_secs(3),
        },
        MockReply::Raw(http_sse_reply(proposal_response)),
    ]);

    let local_profile = AiProviderProfile::new(
        "local-proposals",
        "Local Ollama",
        AiProviderKind::LocalOpenAiCompatible,
        &server.origin,
        "qwen2.5-coder",
    );
    editor.workspace.ai.upsert_profile(local_profile);
    editor.ai_manager.set_test_state(
        "local-proposals",
        crate::ai_connections::AiProfileTestState::Connected {
            model: "qwen2.5-coder".into(),
        },
    );

    // Part A: Mid-stream Stop preserves partial text, shows "Request cancelled.", and never mutates project.
    assert!(editor.open_send_to_ai());
    editor.ai_session.send_dialog.as_mut().unwrap().request_text = "Stream then stop.".into();
    assert!(editor.submit_send_to_ai());

    let stream_deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < stream_deadline {
        editor.poll_ai_session(&ctx);
        if editor
            .ai_session
            .answer_dialog
            .as_ref()
            .is_some_and(|a| a.streamed_text.contains("Partial answer before stop"))
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let cancel_tok = editor
        .ai_session
        .worker
        .as_ref()
        .expect("worker still streaming")
        .cancellation_token();
    let _ = run_ui_frame(&mut editor, &ctx, 0.1, vec![]);
    editor.ai_session.stop_active_request();
    assert!(cancel_tok.is_cancelled());
    let answer_after_stop = editor.ai_session.answer_dialog.as_ref().unwrap();
    assert_eq!(answer_after_stop.phase, AiAnswerPhase::Cancelled);
    assert_eq!(
        answer_after_stop.streamed_text,
        "Partial answer before stop"
    );
    let out_cancelled = run_ui_frame(&mut editor, &ctx, 0.2, vec![]);
    assert!(
        ax_labels(&out_cancelled)
            .iter()
            .any(|l| l.contains("Request cancelled.")),
        "Cancelled dialog must display 'Request cancelled.'"
    );
    assert_eq!(editor.project, baseline_project);

    // Part B: Full proposal review (`600 pt`), validation, Dismiss, Apply & single-step Undo.
    assert!(editor.open_send_to_ai());
    editor.ai_session.send_dialog.as_mut().unwrap().request_text =
        "Propose frame script and easing.".into();
    assert!(editor.submit_send_to_ai());
    let complete_deadline = Instant::now() + Duration::from_secs(3);
    while editor.ai_session.worker.is_some() && Instant::now() < complete_deadline {
        editor.poll_ai_session(&ctx);
        std::thread::sleep(Duration::from_millis(5));
    }

    let out_completed = run_ui_frame(&mut editor, &ctx, 0.3, vec![]);
    let completed_labels = ax_labels(&out_completed);
    assert!(
        completed_labels
            .iter()
            .any(|l| l.eq_ignore_ascii_case("Proposals")),
        "Expected Proposals overline in {completed_labels:?}"
    );
    assert!(completed_labels.iter().any(|l| l == "Insert into Actions"));
    assert!(completed_labels.iter().any(|l| l == "Apply to timeline"));
    assert!(
        completed_labels
            .iter()
            .any(|l| l.contains("Layer “Ghost Layer” is not in the current timeline.")),
        "Missing layer validation reason must be rendered on the card"
    );
    assert_eq!(
        editor.project, baseline_project,
        "Receiving and rendering AI proposals must never mutate the document"
    );

    // Check timing validation rules: missing layer, out-of-bounds range, and locked layer.
    assert!(
        editor
            .validate_ai_timing_proposal("Ghost Layer", 1, 12)
            .is_err()
    );
    assert!(
        editor
            .validate_ai_timing_proposal("Layer 1", 1, 99)
            .is_err()
    );
    editor.symbol_mut().timeline.layers[0].locked = true;
    assert!(
        editor
            .validate_ai_timing_proposal("Layer 1", 1, 12)
            .is_err()
    );
    editor.symbol_mut().timeline.layers[0].locked = false;

    // Dismissing a card leaves the document untouched.
    editor.ai_session.answer_dialog.as_mut().unwrap().cards[2].state = ProposalCardState::Dismissed;
    assert_eq!(editor.project, baseline_project);

    // Position the Actions caret right after `"// header\n"` (char index 10) and apply code proposal.
    let actions_id = editor.actions_editor_id();
    let mut caret_state = egui::text_edit::TextEditState::default();
    caret_state
        .cursor
        .set_char_range(Some(egui::text::CCursorRange::one(
            egui::text::CCursor::new(10),
        )));
    caret_state.store(&ctx, actions_id);

    let undo_before_code = editor.history.undo_len();
    assert!(editor.apply_ai_code_proposal(&ctx, ScriptLanguage::ActionScript3, "stop();\n"));
    assert_eq!(
        editor.history.undo_len(),
        undo_before_code + 1,
        "Insert into Actions must create exactly ONE Undo step"
    );
    assert_eq!(editor.history.undo_label(), Some("Insert AI code proposal"));
    editor
        .project
        .validate()
        .expect("valid project after code proposal");
    assert_eq!(
        editor.symbol().timeline.scripts[0].script.source,
        "// header\nstop();\n// footer\n"
    );

    editor.undo();
    assert_eq!(
        editor.project, baseline_project,
        "Single Undo must restore exact baseline project after code proposal"
    );

    // Apply the valid timing proposal (`Layer 1`, frames 1..=12, `ease_out`).
    let undo_before_timing = editor.history.undo_len();
    let msg = editor
        .apply_ai_timing_proposal("Layer 1", 1, 12, "ease_out")
        .expect("apply timing proposal");
    assert_eq!(msg, "Applied to layer Layer 1, frames 1 – 12.");
    assert_eq!(
        editor.history.undo_len(),
        undo_before_timing + 1,
        "Apply to timeline must create exactly ONE Undo step"
    );
    assert_eq!(
        editor.history.undo_label(),
        Some("Apply AI timing proposal")
    );
    editor
        .project
        .validate()
        .expect("valid project after timing proposal");

    let layer = &editor.symbol().timeline.layers[0];
    assert_eq!(layer.keyframes[0].frame, 0);
    assert_eq!(layer.keyframes[0].tween, Some(Easing::EaseOut));
    assert!(layer.keyframes.iter().any(|k| k.frame == 11));

    editor.undo();
    assert_eq!(
        editor.project, baseline_project,
        "Single Undo must restore exact baseline project after timing proposal"
    );
}

#[test]
fn t305_document_sessions_isolate_ai_dialogs_proposals_and_cancel_on_tab_close() {
    let ctx = egui::Context::default();
    let mut doc_alpha = Project::blank();
    doc_alpha.name = "Doc Alpha".into();
    let mut doc_beta = Project::blank();
    doc_beta.name = "Doc Beta".into();
    let mut doc_gamma = Project::blank();
    doc_gamma.name = "Doc Gamma".into();

    let mut editor = Editor::from_project(&ctx, doc_alpha);
    let server = LoopbackFixtureServer::start(vec![
        MockReply::Raw(http_sse_reply(concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"```as3\\nstop();\\n```\"}}]}\n\n",
            "data: [DONE]\n\n"
        ))),
        MockReply::SlowBody {
            head: "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\r\n".into(),
            chunks: vec![(
                Duration::ZERO,
                b"data: {\"choices\":[{\"delta\":{\"content\":\"Gamma streaming...\"}}]}\n\n"
                    .to_vec(),
            )],
            hold_open: Duration::from_secs(4),
        },
    ]);

    editor.workspace.ai.upsert_profile(AiProviderProfile::new(
        "local-iso",
        "Local",
        AiProviderKind::LocalOpenAiCompatible,
        &server.origin,
        "qwen2.5-coder",
    ));
    editor.ai_manager.set_test_state(
        "local-iso",
        crate::ai_connections::AiProfileTestState::Connected {
            model: "qwen2.5-coder".into(),
        },
    );

    editor.open_new_session_tab(&ctx, doc_beta.clone(), None);
    assert_eq!(editor.session_count(), 2);

    // Switch to Tab 0 (Doc Alpha) and complete an AI proposal request.
    assert!(editor.switch_session_tab(&ctx, 0));
    assert_eq!(editor.project.name, "Doc Alpha");
    assert!(editor.open_send_to_ai());
    editor.ai_session.send_dialog.as_mut().unwrap().request_text = "Add stop to Alpha.".into();
    assert!(editor.submit_send_to_ai());
    let deadline = Instant::now() + Duration::from_secs(3);
    while editor.ai_session.worker.is_some() && Instant::now() < deadline {
        editor.poll_ai_session(&ctx);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(
        editor
            .ai_session
            .answer_dialog
            .as_ref()
            .unwrap()
            .cards
            .len(),
        1
    );

    // Switch to Tab 1 (Doc Beta): Beta has NO open AI dialog, NO proposals, and NO mutations.
    assert!(editor.switch_session_tab(&ctx, 1));
    assert_eq!(editor.project.name, "Doc Beta");
    assert!(!editor.ai_session.has_open_modal());
    assert!(editor.ai_session.worker.is_none());
    assert_eq!(editor.project, doc_beta);

    // Switch back to Tab 0 (Doc Alpha): Alpha's proposal card is preserved; applying it mutates ONLY Alpha.
    assert!(editor.switch_session_tab(&ctx, 0));
    assert_eq!(editor.project.name, "Doc Alpha");
    assert_eq!(
        editor
            .ai_session
            .answer_dialog
            .as_ref()
            .unwrap()
            .cards
            .len(),
        1
    );
    assert!(editor.apply_ai_code_proposal(&ctx, ScriptLanguage::ActionScript3, "stop();\n"));
    assert_eq!(editor.symbol().timeline.scripts.len(), 1);

    assert!(editor.switch_session_tab(&ctx, 1));
    assert_eq!(
        editor.project, doc_beta,
        "Applying an AI proposal in Tab 0 must never leak into Tab 1"
    );

    // Open clean Tab 2 (Doc Gamma), start an in-flight streaming request, switch away, then close Tab 2.
    let gamma_idx = editor.open_new_session_tab(&ctx, doc_gamma, None);
    assert_eq!(gamma_idx, 2);
    assert!(editor.open_send_to_ai());
    editor.ai_session.send_dialog.as_mut().unwrap().request_text = "Long stream in Gamma.".into();
    assert!(editor.submit_send_to_ai());
    let gamma_cancel = editor
        .ai_session
        .worker
        .as_ref()
        .expect("Gamma worker active")
        .cancellation_token();
    assert!(!gamma_cancel.is_cancelled());

    // Switch to Tab 1 while Gamma streams in the background, then close Tab 2 -> cancels immediately!
    assert!(editor.switch_session_tab(&ctx, 1));
    assert!(!gamma_cancel.is_cancelled());
    assert!(editor.close_session_tab(&ctx, 2));
    assert!(
        gamma_cancel.is_cancelled(),
        "Closing a document tab must immediately cancel its in-flight AI request"
    );
    assert_eq!(editor.session_count(), 2);
    assert!(!editor.ai_session.has_open_modal());
}

#[test]
fn t311_ai_proposal_language_resolution_secret_error_surfacing_and_custom_redaction() {
    #[derive(Default)]
    struct TestStorage(std::collections::BTreeMap<String, String>);
    impl eframe::Storage for TestStorage {
        fn get_string(&self, key: &str) -> Option<String> {
            self.0.get(key).cloned()
        }
        fn set_string(&mut self, key: &str, value: String) {
            self.0.insert(key.to_owned(), value);
        }
        fn flush(&mut self) {}
    }

    let ctx = egui::Context::default();

    // 1. Generic ````actionscript` (`ActionScript3`) proposal in an AS2 project creates compiled AS2 Frame & Object scripts.
    let mut as2_project = Project::demo();
    as2_project.script_language = Some(ScriptLanguage::ActionScript2);
    as2_project.symbols[0].timeline.scripts.clear();
    let mut editor = Editor::from_project(&ctx, as2_project);
    editor.script_target = crate::app::ScriptTarget::Frame;
    editor.frame = 0;
    assert!(editor.apply_ai_code_proposal(&ctx, ScriptLanguage::ActionScript3, "stop();\n"));
    assert_eq!(
        editor.symbol().timeline.scripts[0].script.language,
        ScriptLanguage::ActionScript2,
        "new Frame script from generic AS fence must use effective_script_language()"
    );

    // Add an editable element on Frame 1 and target `ScriptTarget::Object`.
    let elem_id = "elem-btn-t311".to_owned();
    let layer_id = editor.symbol().timeline.layers[0].id.clone();
    editor.symbol_mut().timeline.layers[0].keyframes[0]
        .elements
        .push(Element {
            id: elem_id.clone(),
            name: "playBtn".into(),
            transform: Transform::default(),
            kind: ElementKind::Rectangle {
                width: 80.0,
                height: 32.0,
                fill: [40, 120, 220, 255],
                stroke: [0, 0, 0, 0],
                stroke_width: 0.0,
            },
            ..Element::default()
        });
    editor.set_selection(vec![crate::selection::SelectionTarget {
        layer: layer_id,
        element: elem_id.clone(),
    }]);
    editor.script_target = crate::app::ScriptTarget::Object;
    assert!(editor.apply_ai_code_proposal(
        &ctx,
        ScriptLanguage::ActionScript3,
        "gotoAndPlay(2);\n"
    ));
    let elem = editor.symbol().timeline.layers[0].keyframes[0]
        .elements
        .iter()
        .find(|e| e.id == elem_id)
        .unwrap();
    assert_eq!(elem.scripts.len(), 1);
    assert_eq!(
        elem.scripts[0].script.language,
        ScriptLanguage::ActionScript2
    );

    // 2. Custom non-prefixed token redaction + `set_secret` failure surfacing + Workspace persistence.
    let custom_secret = "custom_gateway_secret_t311_xyz987";
    crate::ai_connections::register_redacted_secret(custom_secret);
    let scrubbed = crate::ai_connections::redact_sensitive_tokens(&format!(
        "upstream rejected token {custom_secret} on port 443"
    ));
    assert!(!scrubbed.contains(custom_secret));
    assert!(scrubbed.contains("[REDACTED_SECRET]"));

    // Read-only `EnvSecretStore` fails `set_secret` -> modal stays open with redacted error + toast.
    editor.ai_manager = crate::ai_connections::AiConnectionManager::new(std::sync::Arc::new(
        crate::ai_connections::EnvSecretStore::default(),
    ));
    let mut failing_modal =
        AiConnectionModal::new_add(&editor.workspace.ai, AiConnectionModalTarget::Workspace);
    failing_modal.display_name = "Env ReadOnly".into();
    failing_modal.kind = AiProviderKind::OpenAi;
    failing_modal.base_url = "https://api.openai.com".into();
    failing_modal.default_model = "gpt-test".into();
    failing_modal.api_key_input = custom_secret.into();
    editor.save_ai_connection_modal(failing_modal);
    assert!(
        editor.ai_connection_modal.is_some(),
        "modal must remain open when SecretStore::set_secret fails"
    );
    let err_msg = editor
        .ai_connection_modal
        .as_ref()
        .unwrap()
        .test_outcome
        .as_ref()
        .expect("error outcome recorded")
        .as_ref()
        .expect_err("expected error");
    assert!(!err_msg.contains(custom_secret));
    assert_eq!(editor.status, *err_msg);

    // Dismissing the modal and running `notification_host` promotes the queued Error toast.
    editor.ai_connection_modal = None;
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        editor.notification_host(ctx, None);
    });
    assert!(
        editor
            .notifications
            .visible_toasts()
            .iter()
            .any(|(kind, _)| *kind == neoflash_ui::components::ToastType::Error)
    );

    // Succeeding `Workspace` modal save requests workspace persistence and flushes to storage.
    editor.ai_manager = crate::ai_connections::AiConnectionManager::in_memory();
    let mut ws_modal =
        AiConnectionModal::new_add(&editor.workspace.ai, AiConnectionModalTarget::Workspace);
    ws_modal.display_name = "Local Ollama".into();
    ws_modal.kind = AiProviderKind::LocalOpenAiCompatible;
    ws_modal.base_url = "http://127.0.0.1:11434".into();
    ws_modal.default_model = "qwen2.5-coder".into();
    editor.save_ai_connection_modal(ws_modal);
    assert!(editor.persist_workspace_requested);
    let mut storage = TestStorage::default();
    editor.flush_workspace_to_storage(&mut storage);
    assert!(!editor.persist_workspace_requested);
    let saved_json = storage
        .0
        .get(crate::workspace::PREFS_KEY)
        .expect("workspace prefs saved");
    assert!(saved_json.contains("Local Ollama"));
}
