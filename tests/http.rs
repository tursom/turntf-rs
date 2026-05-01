use std::collections::HashMap;
use std::sync::Arc;

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use bcrypt::verify;
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;
use turntf::{
    plain_password, CreateUserRequest, DeliveryMode, HttpClient, ScanUserMetadataRequest,
    UpdateUserRequest, UpsertUserMetadataRequest, UserRef,
};

#[derive(Debug)]
struct HttpTestRequest {
    method: String,
    path: String,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

#[derive(Debug)]
struct HttpTestResponse {
    status: u16,
    body: Vec<u8>,
    content_type: &'static str,
}

impl HttpTestResponse {
    fn json(value: Value, status: u16) -> Self {
        Self {
            status,
            body: serde_json::to_vec(&value).unwrap(),
            content_type: "application/json",
        }
    }

    fn text(body: &str, status: u16) -> Self {
        Self {
            status,
            body: body.as_bytes().to_vec(),
            content_type: "text/plain; charset=utf-8",
        }
    }
}

async fn spawn_http_server(
    handler: impl Fn(HttpTestRequest) -> HttpTestResponse + Send + Sync + 'static,
) -> (String, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let handler = Arc::new(handler);

    let task = tokio::spawn(async move {
        loop {
            let (stream, _) = match listener.accept().await {
                Ok(value) => value,
                Err(_) => return,
            };
            let handler = Arc::clone(&handler);
            tokio::spawn(async move {
                let _ = handle_http_connection(stream, handler).await;
            });
        }
    });

    (format!("http://{address}"), task)
}

async fn handle_http_connection(
    mut stream: TcpStream,
    handler: Arc<dyn Fn(HttpTestRequest) -> HttpTestResponse + Send + Sync>,
) -> std::io::Result<()> {
    let request = read_http_request(&mut stream).await?;
    let response = handler(request);
    let body = response.body;
    let status = status_text(response.status);
    let header = format!(
        "HTTP/1.1 {} {}\r\nContent-Length: {}\r\nContent-Type: {}\r\nConnection: close\r\n\r\n",
        response.status,
        status,
        body.len(),
        response.content_type
    );
    stream.write_all(header.as_bytes()).await?;
    stream.write_all(&body).await?;
    stream.shutdown().await
}

async fn read_http_request(stream: &mut TcpStream) -> std::io::Result<HttpTestRequest> {
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 1024];

    loop {
        let read = stream.read(&mut chunk).await?;
        if read == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..read]);
        if find_header_end(&buffer).is_some() {
            break;
        }
    }

    let header_end = find_header_end(&buffer).expect("HTTP header terminator");
    let header_text = String::from_utf8_lossy(&buffer[..header_end]);
    let mut lines = header_text.split("\r\n");
    let request_line = lines.next().unwrap();
    let mut request_parts = request_line.split_whitespace();
    let method = request_parts.next().unwrap().to_owned();
    let path = request_parts.next().unwrap().to_owned();

    let mut headers = HashMap::new();
    let mut content_length = 0usize;
    for line in lines {
        if line.is_empty() {
            continue;
        }
        if let Some((name, value)) = line.split_once(':') {
            let value = value.trim().to_owned();
            if name.eq_ignore_ascii_case("content-length") {
                content_length = value.parse().unwrap_or(0);
            }
            headers.insert(name.to_ascii_lowercase(), value);
        }
    }

    let mut body = buffer[(header_end + 4)..].to_vec();
    while body.len() < content_length {
        let read = stream.read(&mut chunk).await?;
        if read == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..read]);
    }
    body.truncate(content_length);

    Ok(HttpTestRequest {
        method,
        path,
        headers,
        body,
    })
}

fn find_header_end(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|window| window == b"\r\n\r\n")
}

fn status_text(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        _ => "Unknown",
    }
}

#[tokio::test]
async fn http_client_requests_and_encoding() {
    let (base_url, server) = spawn_http_server(|request| {
        let body = if request.body.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice::<Value>(&request.body).unwrap()
        };

        match (request.method.as_str(), request.path.as_str()) {
            ("POST", "/auth/login") => {
                let password = body.get("password").and_then(Value::as_str).unwrap();
                assert_ne!(password, "root");
                assert!(verify("root", password).unwrap());
                if let Some(login_name) = body.get("login_name").and_then(Value::as_str) {
                    assert_eq!(login_name, "root.login");
                    assert!(body.get("node_id").is_none());
                    assert!(body.get("user_id").is_none());
                    HttpTestResponse::json(json!({ "token": "login-name-token" }), 200)
                } else {
                    assert_eq!(body.get("node_id").and_then(Value::as_i64), Some(4096));
                    assert_eq!(body.get("user_id").and_then(Value::as_i64), Some(1));
                    HttpTestResponse::json(json!({ "token": "admin-token" }), 200)
                }
            }
            ("POST", "/users") => {
                assert_eq!(
                    request.headers.get("authorization").map(String::as_str),
                    Some("Bearer admin-token")
                );
                assert_eq!(
                    body.get("login_name").and_then(Value::as_str),
                    Some("alice.login")
                );
                let password = body.get("password").and_then(Value::as_str).unwrap();
                assert_ne!(password, "alice-password");
                assert!(verify("alice-password", password).unwrap());
                assert!(body.get("profile").is_some());
                assert!(body.get("profile_json").is_none());
                HttpTestResponse::json(
                    json!({
                        "node_id": 4096,
                        "user_id": 1025,
                        "username": body.get("username").unwrap(),
                        "login_name": body.get("login_name").unwrap(),
                        "role": body.get("role").unwrap(),
                        "profile": { "tier": "gold" },
                        "created_at": "hlc-created"
                    }),
                    201,
                )
            }
            ("GET", "/nodes/4096/users/1025") => HttpTestResponse::json(
                json!({
                    "node_id": 4096,
                    "user_id": 1025,
                    "username": "alice",
                    "login_name": "alice.login",
                    "role": "user",
                    "profile": { "tier": "gold" }
                }),
                200,
            ),
            ("PATCH", "/nodes/4096/users/1025") => {
                assert_eq!(body.get("login_name").and_then(Value::as_str), Some(""));
                let password = body.get("password").and_then(Value::as_str).unwrap();
                assert_ne!(password, "new-password");
                assert!(verify("new-password", password).unwrap());
                HttpTestResponse::json(
                    json!({
                        "node_id": 4096,
                        "user_id": 1025,
                        "username": body.get("username").unwrap(),
                        "login_name": body.get("login_name").unwrap(),
                        "role": body.get("role").unwrap(),
                        "profile": body.get("profile").unwrap(),
                    }),
                    200,
                )
            }
            ("DELETE", "/nodes/4096/users/1025") => HttpTestResponse::json(
                json!({ "status": "deleted", "node_id": 4096, "user_id": 1025 }),
                200,
            ),
            ("PUT", "/nodes/4096/users/1025/metadata/session:web:1") => {
                let encoded = STANDARD.encode([0xde, 0xad, 0xbe]);
                assert_eq!(
                    body.get("value").and_then(Value::as_str),
                    Some(encoded.as_str())
                );
                assert_eq!(
                    body.get("expires_at").and_then(Value::as_str),
                    Some("2026-05-01T12:00:00Z")
                );
                HttpTestResponse::json(
                    json!({
                        "owner": { "node_id": 4096, "user_id": 1025 },
                        "key": "session:web:1",
                        "value": encoded,
                        "updated_at": "hlc-meta-upsert",
                        "expires_at": "2026-05-01T12:00:00Z",
                        "origin_node_id": 4096
                    }),
                    201,
                )
            }
            ("GET", "/nodes/4096/users/1025/metadata/session:web:1") => HttpTestResponse::json(
                json!({
                    "owner": { "node_id": 4096, "user_id": 1025 },
                    "key": "session:web:1",
                    "value": STANDARD.encode([0xde, 0xad, 0xbe]),
                    "updated_at": "hlc-meta-get",
                    "expires_at": "2026-05-01T12:00:00Z",
                    "origin_node_id": 4096
                }),
                200,
            ),
            ("GET", path)
                if path == "/nodes/4096/users/1025/metadata?prefix=session:&after=session:web:0&limit=1"
                    || path
                        == "/nodes/4096/users/1025/metadata?prefix=session%3A&after=session%3Aweb%3A0&limit=1" =>
            {
                HttpTestResponse::json(
                    json!({
                        "items": [{
                            "owner": { "node_id": 4096, "user_id": 1025 },
                            "key": "session:web:1",
                            "value": STANDARD.encode([0xde, 0xad, 0xbe]),
                            "updated_at": "hlc-meta-scan",
                            "expires_at": "2026-05-01T12:00:00Z",
                            "origin_node_id": 4096
                        }],
                        "count": 1,
                        "next_after": "session:web:1"
                    }),
                    200,
                )
            }
            ("DELETE", "/nodes/4096/users/1025/metadata/session:web:1") => HttpTestResponse::json(
                json!({
                    "owner": { "node_id": 4096, "user_id": 1025 },
                    "key": "session:web:1",
                    "value": STANDARD.encode([0xde, 0xad, 0xbe]),
                    "updated_at": "hlc-meta-upsert",
                    "deleted_at": "hlc-meta-deleted",
                    "expires_at": "2026-05-01T12:00:00Z",
                    "origin_node_id": 4096
                }),
                200,
            ),
            ("PUT", "/nodes/4096/users/1025/attachments/channel_subscription/4096/2025") => {
                HttpTestResponse::json(
                    json!({
                        "owner": { "node_id": 4096, "user_id": 1025 },
                        "subject": { "node_id": 4096, "user_id": 2025 },
                        "attachment_type": "channel_subscription",
                        "config_json": {},
                        "attached_at": "hlc-sub",
                        "origin_node_id": 4096
                    }),
                    201,
                )
            }
            ("GET", "/nodes/4096/users/1025/attachments?attachment_type=channel_subscription") => {
                HttpTestResponse::json(
                    json!({
                        "items": [{
                            "owner": { "node_id": 4096, "user_id": 1025 },
                            "subject": { "node_id": 4096, "user_id": 2025 },
                            "attachment_type": "channel_subscription",
                            "config_json": {},
                            "attached_at": "hlc-sub",
                            "origin_node_id": 4096
                        }],
                        "count": 1
                    }),
                    200,
                )
            }
            ("DELETE", "/nodes/4096/users/1025/attachments/channel_subscription/4096/2025") => {
                HttpTestResponse::json(
                    json!({
                        "owner": { "node_id": 4096, "user_id": 1025 },
                        "subject": { "node_id": 4096, "user_id": 2025 },
                        "attachment_type": "channel_subscription",
                        "config_json": {},
                        "attached_at": "hlc-sub",
                        "deleted_at": "hlc-unsub",
                        "origin_node_id": 4096
                    }),
                    200,
                )
            }
            ("GET", "/nodes/4096/users/1025/messages?limit=20") => HttpTestResponse::json(
                json!({
                    "items": [{
                        "recipient": { "node_id": 4096, "user_id": 1025 },
                        "node_id": 4096,
                        "seq": 3,
                        "sender": { "node_id": 4096, "user_id": 1 },
                        "body": STANDARD.encode([0xff, 0x00]),
                        "created_at": "hlc1"
                    }],
                    "count": 1
                }),
                200,
            ),
            ("POST", "/nodes/4096/users/1025/messages") => {
                assert_eq!(
                    body.get("body").and_then(Value::as_str),
                    Some(STANDARD.encode([0xff, 0x00]).as_str())
                );
                HttpTestResponse::json(
                    json!({
                        "recipient": { "node_id": 4096, "user_id": 1025 },
                        "node_id": 4096,
                        "seq": 4,
                        "sender": { "node_id": 4096, "user_id": 1 },
                        "body": STANDARD.encode([0xff, 0x00]),
                        "created_at": "hlc2"
                    }),
                    201,
                )
            }
            ("POST", "/nodes/8192/users/1025/messages") => {
                assert_eq!(
                    body.get("delivery_kind").and_then(Value::as_str),
                    Some("transient")
                );
                assert_eq!(
                    body.get("delivery_mode").and_then(Value::as_str),
                    Some("route_retry")
                );
                HttpTestResponse::json(
                    json!({
                        "packet_id": 77,
                        "source_node_id": 4096,
                        "target_node_id": 8192,
                        "recipient": { "node_id": 8192, "user_id": 1025 },
                        "delivery_mode": "route_retry"
                    }),
                    202,
                )
            }
            ("GET", "/cluster/nodes") => HttpTestResponse::json(
                json!([
                    { "node_id": 4096, "is_local": true },
                    {
                        "node_id": 8192,
                        "is_local": false,
                        "configured_url": "ws://127.0.0.1:9081/internal/cluster/ws",
                        "source": "discovered"
                    }
                ]),
                200,
            ),
            ("GET", "/cluster/nodes/4096/logged-in-users") => HttpTestResponse::json(
                json!({
                    "target_node_id": 4096,
                    "items": [
                        { "node_id": 4096, "user_id": 1025, "username": "alice", "login_name": "alice.login" },
                        { "node_id": 4096, "user_id": 1026, "username": "bob", "login_name": "bob.login" }
                    ],
                    "count": 2
                }),
                200,
            ),
            ("PUT", "/nodes/4096/users/1025/attachments/user_blacklist/4096/1027") => {
                HttpTestResponse::json(
                    json!({
                        "owner": { "node_id": 4096, "user_id": 1025 },
                        "subject": { "node_id": 4096, "user_id": 1027 },
                        "attachment_type": "user_blacklist",
                        "config_json": {},
                        "attached_at": "hlc-blocked",
                        "origin_node_id": 4096
                    }),
                    201,
                )
            }
            ("GET", "/nodes/4096/users/1025/attachments?attachment_type=user_blacklist") => {
                HttpTestResponse::json(
                    json!({
                        "items": [{
                            "owner": { "node_id": 4096, "user_id": 1025 },
                            "subject": { "node_id": 4096, "user_id": 1027 },
                            "attachment_type": "user_blacklist",
                            "config_json": {},
                            "attached_at": "hlc-blocked",
                            "origin_node_id": 4096
                        }],
                        "count": 1
                    }),
                    200,
                )
            }
            ("DELETE", "/nodes/4096/users/1025/attachments/user_blacklist/4096/1027") => {
                HttpTestResponse::json(
                    json!({
                        "owner": { "node_id": 4096, "user_id": 1025 },
                        "subject": { "node_id": 4096, "user_id": 1027 },
                        "attachment_type": "user_blacklist",
                        "config_json": {},
                        "attached_at": "hlc-blocked",
                        "deleted_at": "hlc-unblocked",
                        "origin_node_id": 4096
                    }),
                    200,
                )
            }
            ("GET", "/events") => HttpTestResponse::json(
                json!({
                    "items": [{
                        "sequence": 1,
                        "event_id": 2,
                        "event_type": "user_created",
                        "aggregate": "user",
                        "aggregate_node_id": 4096,
                        "aggregate_id": 1025,
                        "hlc": "hlc-event",
                        "origin_node_id": 4096,
                        "event": { "tier": "gold" }
                    }],
                    "count": 1
                }),
                200,
            ),
            ("GET", "/ops/status") => HttpTestResponse::json(
                json!({
                    "node_id": 4096,
                    "message_window_size": 128,
                    "last_event_sequence": 99,
                    "write_gate_ready": true,
                    "conflict_total": 1,
                    "message_trim": { "trimmed_total": 2, "last_trimmed_at": "hlc-trim" },
                    "projection": { "pending_total": 3, "last_failed_at": "hlc-fail" },
                    "event_log_trim": { "trimmed_total": 4, "last_trimmed_at": "hlc-log-trim" },
                    "peers": [{
                        "node_id": 8192,
                        "configured_url": "ws://127.0.0.1:9081/internal/cluster/ws",
                        "source": "discovered",
                        "discovered_url": "ws://127.0.0.1:9081/internal/cluster/ws",
                        "discovery_state": "connected",
                        "last_discovered_at": "hlc-discovered",
                        "last_connected_at": "hlc-connected",
                        "last_discovery_error": "previous error",
                        "connected": true,
                        "session_direction": "outbound",
                        "origins": [{
                            "origin_node_id": 4096,
                            "acked_event_id": 9,
                            "applied_event_id": 8,
                            "unconfirmed_events": 1,
                            "cursor_updated_at": "hlc-cursor",
                            "remote_last_event_id": 10,
                            "pending_catchup": false
                        }]
                    }]
                }),
                200,
            ),
            ("GET", "/metrics") => HttpTestResponse::text("notifier_write_gate_ready 1\n", 200),
            ("GET", "/broken") => HttpTestResponse::json(
                json!({ "code": "unauthorized", "message": "bad token" }),
                401,
            ),
            other => panic!("unexpected route: {other:?}"),
        }
    })
    .await;

    let client = HttpClient::new(base_url).unwrap();

    let token = client.login(4096, 1, "root").await.unwrap();
    assert_eq!(token, "admin-token");
    let login_name_token = client
        .login_by_login_name("root.login", "root")
        .await
        .unwrap();
    assert_eq!(login_name_token, "login-name-token");

    let user = client
        .create_user(
            &token,
            CreateUserRequest {
                username: "alice".into(),
                login_name: Some("alice.login".into()),
                password: Some(plain_password("alice-password").unwrap()),
                profile_json: br#"{"tier":"gold"}"#.to_vec(),
                role: "user".into(),
            },
        )
        .await
        .unwrap();
    assert_eq!(user.user_id, 1025);
    assert_eq!(user.login_name, "alice.login");
    assert_eq!(user.profile_json, br#"{"tier":"gold"}"#);

    let fetched = client
        .get_user(
            &token,
            UserRef {
                node_id: 4096,
                user_id: 1025,
            },
        )
        .await
        .unwrap();
    assert_eq!(fetched.username, "alice");
    assert_eq!(fetched.login_name, "alice.login");

    let updated = client
        .update_user(
            &token,
            UserRef {
                node_id: 4096,
                user_id: 1025,
            },
            UpdateUserRequest {
                username: Some("alice-2".into()),
                login_name: Some(String::new()),
                password: Some(plain_password("new-password").unwrap()),
                profile_json: Some(br#"{"tier":"platinum"}"#.to_vec()),
                role: Some("admin".into()),
            },
        )
        .await
        .unwrap();
    assert_eq!(updated.username, "alice-2");
    assert!(updated.login_name.is_empty());
    assert_eq!(updated.profile_json, br#"{"tier":"platinum"}"#);

    let metadata = client
        .upsert_user_metadata(
            &token,
            UserRef {
                node_id: 4096,
                user_id: 1025,
            },
            "session:web:1",
            UpsertUserMetadataRequest {
                value: vec![0xde, 0xad, 0xbe],
                expires_at: Some("2026-05-01T12:00:00Z".into()),
            },
        )
        .await
        .unwrap();
    assert_eq!(metadata.value, vec![0xde, 0xad, 0xbe]);
    assert_eq!(metadata.expires_at, "2026-05-01T12:00:00Z");

    let loaded_metadata = client
        .get_user_metadata(
            &token,
            UserRef {
                node_id: 4096,
                user_id: 1025,
            },
            "session:web:1",
        )
        .await
        .unwrap();
    assert_eq!(loaded_metadata.updated_at, "hlc-meta-get");

    let scanned_metadata = client
        .scan_user_metadata(
            &token,
            UserRef {
                node_id: 4096,
                user_id: 1025,
            },
            ScanUserMetadataRequest {
                prefix: "session:".into(),
                after: "session:web:0".into(),
                limit: 1,
            },
        )
        .await
        .unwrap();
    assert_eq!(scanned_metadata.count, 1);
    assert_eq!(scanned_metadata.next_after, "session:web:1");
    assert_eq!(scanned_metadata.items[0].key, "session:web:1");

    let deleted_metadata = client
        .delete_user_metadata(
            &token,
            UserRef {
                node_id: 4096,
                user_id: 1025,
            },
            "session:web:1",
        )
        .await
        .unwrap();
    assert_eq!(deleted_metadata.deleted_at, "hlc-meta-deleted");

    let subscription = client
        .create_subscription(
            &token,
            UserRef {
                node_id: 4096,
                user_id: 1025,
            },
            UserRef {
                node_id: 4096,
                user_id: 2025,
            },
        )
        .await
        .unwrap();
    assert_eq!(subscription.channel.user_id, 2025);

    let subscriptions = client
        .list_subscriptions(
            &token,
            UserRef {
                node_id: 4096,
                user_id: 1025,
            },
        )
        .await
        .unwrap();
    assert_eq!(subscriptions.len(), 1);

    let removed = client
        .unsubscribe_channel(
            &token,
            UserRef {
                node_id: 4096,
                user_id: 1025,
            },
            UserRef {
                node_id: 4096,
                user_id: 2025,
            },
        )
        .await
        .unwrap();
    assert_eq!(removed.deleted_at, "hlc-unsub");

    let messages = client
        .list_messages(
            &token,
            UserRef {
                node_id: 4096,
                user_id: 1025,
            },
            20,
        )
        .await
        .unwrap();
    assert_eq!(messages[0].body, vec![0xff, 0x00]);
    assert_eq!(messages[0].created_at_hlc, "hlc1");

    let message = client
        .post_message(
            &token,
            UserRef {
                node_id: 4096,
                user_id: 1025,
            },
            vec![0xff, 0x00],
        )
        .await
        .unwrap();
    assert_eq!(message.seq, 4);

    let packet = client
        .post_packet(
            &token,
            8192,
            UserRef {
                node_id: 8192,
                user_id: 1025,
            },
            vec![0xff, 0x00],
            DeliveryMode::RouteRetry,
        )
        .await
        .unwrap();
    assert_eq!(packet.packet_id, 77);

    let nodes = client.list_cluster_nodes(&token).await.unwrap();
    assert_eq!(nodes.len(), 2);
    assert_eq!(nodes[1].source, "discovered");

    let users = client
        .list_node_logged_in_users(&token, 4096)
        .await
        .unwrap();
    assert_eq!(
        users
            .iter()
            .map(|user| user.username.as_str())
            .collect::<Vec<_>>(),
        vec!["alice", "bob"]
    );
    assert_eq!(
        users
            .iter()
            .map(|user| user.login_name.as_str())
            .collect::<Vec<_>>(),
        vec!["alice.login", "bob.login"]
    );

    let blocked = client
        .block_user(
            &token,
            UserRef {
                node_id: 4096,
                user_id: 1025,
            },
            UserRef {
                node_id: 4096,
                user_id: 1027,
            },
        )
        .await
        .unwrap();
    assert_eq!(blocked.blocked.user_id, 1027);

    let blocked_items = client
        .list_blocked_users(
            &token,
            UserRef {
                node_id: 4096,
                user_id: 1025,
            },
        )
        .await
        .unwrap();
    assert_eq!(blocked_items.len(), 1);

    let unblocked = client
        .unblock_user(
            &token,
            UserRef {
                node_id: 4096,
                user_id: 1025,
            },
            UserRef {
                node_id: 4096,
                user_id: 1027,
            },
        )
        .await
        .unwrap();
    assert_eq!(unblocked.deleted_at, "hlc-unblocked");

    let events = client.list_events(&token, 0, 0).await.unwrap();
    assert_eq!(events[0].event_json, br#"{"tier":"gold"}"#);

    let status = client.operations_status(&token).await.unwrap();
    assert_eq!(status.event_log_trim.unwrap().trimmed_total, 4);
    assert_eq!(status.peers[0].origins[0].remote_last_event_id, 10);

    let metrics = client.metrics(&token).await.unwrap();
    assert!(metrics.contains("notifier_write_gate_ready"));

    let deleted = client
        .delete_user(
            &token,
            UserRef {
                node_id: 4096,
                user_id: 1025,
            },
        )
        .await
        .unwrap();
    assert_eq!(deleted.status, "deleted");
    assert_eq!(deleted.user.user_id, 1025);

    server.abort();
}

#[tokio::test]
async fn http_client_maps_server_errors_and_validates_node_id() {
    let (base_url, server) =
        spawn_http_server(
            |request| match (request.method.as_str(), request.path.as_str()) {
                ("GET", "/cluster/nodes/0/logged-in-users") => {
                    panic!("should not reach invalid node_id route")
                }
                ("GET", "/nodes/4096/users/1025") => HttpTestResponse::json(
                    json!({ "code": "unauthorized", "message": "bad token" }),
                    401,
                ),
                other => panic!("unexpected route: {other:?}"),
            },
        )
        .await;

    let client = HttpClient::new(base_url).unwrap();
    let error = client
        .list_node_logged_in_users("token", 0)
        .await
        .unwrap_err();
    assert!(matches!(error, turntf::Error::Validation(_)));

    let error = client
        .get_user_metadata(
            "token",
            UserRef {
                node_id: 4096,
                user_id: 1025,
            },
            "bad/key",
        )
        .await
        .unwrap_err();
    assert!(matches!(error, turntf::Error::Validation(_)));

    let error = client
        .scan_user_metadata(
            "token",
            UserRef {
                node_id: 4096,
                user_id: 1025,
            },
            ScanUserMetadataRequest {
                prefix: String::new(),
                after: String::new(),
                limit: -1,
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(error, turntf::Error::Validation(_)));

    let error = client
        .get_user(
            "bad-token",
            UserRef {
                node_id: 4096,
                user_id: 1025,
            },
        )
        .await;
    assert!(matches!(error, Err(turntf::Error::Server(server)) if server.code == "unauthorized"));

    server.abort();
}
