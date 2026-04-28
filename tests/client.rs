use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use async_trait::async_trait;
use bcrypt::verify;
use futures_util::{SinkExt, StreamExt};
use prost::Message as _;
use tokio::net::TcpListener;
use tokio::sync::{oneshot, Mutex};
use tokio::time::timeout;
use tokio_stream::wrappers::errors::BroadcastStreamRecvError;
use tokio_tungstenite::accept_hdr_async;
use tokio_tungstenite::tungstenite::handshake::server::{Request, Response};
use tokio_tungstenite::tungstenite::Message as WsMessage;
use turntf::{
    plain_password, Client, ClientEvent, Config, CreateUserRequest, CursorStore, DeliveryMode,
    MemoryCursorStore, Message, MessageCursor,
};

type BoxError = Box<dyn std::error::Error + Send + Sync + 'static>;

#[derive(Clone, PartialEq, prost::Message)]
struct UserRefProto {
    #[prost(int64, tag = "1")]
    node_id: i64,
    #[prost(int64, tag = "2")]
    user_id: i64,
}

#[derive(Clone, PartialEq, prost::Message)]
struct MessageCursorProto {
    #[prost(int64, tag = "1")]
    node_id: i64,
    #[prost(int64, tag = "2")]
    seq: i64,
}

#[derive(Clone, PartialEq, prost::Message)]
struct LoginRequestProto {
    #[prost(message, optional, tag = "1")]
    user: Option<UserRefProto>,
    #[prost(string, tag = "3")]
    password: String,
    #[prost(message, repeated, tag = "4")]
    seen_messages: Vec<MessageCursorProto>,
    #[prost(bool, tag = "5")]
    transient_only: bool,
}

#[derive(Clone, PartialEq, prost::Message)]
struct UserProto {
    #[prost(int64, tag = "1")]
    node_id: i64,
    #[prost(int64, tag = "2")]
    user_id: i64,
    #[prost(string, tag = "3")]
    username: String,
    #[prost(string, tag = "4")]
    role: String,
    #[prost(bytes = "vec", tag = "5")]
    profile_json: Vec<u8>,
    #[prost(bool, tag = "6")]
    system_reserved: bool,
    #[prost(string, tag = "7")]
    created_at: String,
    #[prost(string, tag = "8")]
    updated_at: String,
    #[prost(int64, tag = "9")]
    origin_node_id: i64,
}

#[derive(Clone, PartialEq, prost::Message)]
struct LoginResponseProto {
    #[prost(message, optional, tag = "1")]
    user: Option<UserProto>,
    #[prost(string, tag = "2")]
    protocol_version: String,
}

#[derive(Clone, PartialEq, prost::Message)]
struct MessageProto {
    #[prost(message, optional, tag = "1")]
    recipient: Option<UserRefProto>,
    #[prost(int64, tag = "3")]
    node_id: i64,
    #[prost(int64, tag = "4")]
    seq: i64,
    #[prost(message, optional, tag = "5")]
    sender: Option<UserRefProto>,
    #[prost(bytes = "vec", tag = "6")]
    body: Vec<u8>,
    #[prost(string, tag = "7")]
    created_at_hlc: String,
}

#[derive(Clone, PartialEq, prost::Message)]
struct PacketProto {
    #[prost(uint64, tag = "1")]
    packet_id: u64,
    #[prost(int64, tag = "2")]
    source_node_id: i64,
    #[prost(int64, tag = "3")]
    target_node_id: i64,
    #[prost(message, optional, tag = "4")]
    recipient: Option<UserRefProto>,
    #[prost(message, optional, tag = "5")]
    sender: Option<UserRefProto>,
    #[prost(bytes = "vec", tag = "6")]
    body: Vec<u8>,
    #[prost(int32, tag = "7")]
    delivery_mode: i32,
}

#[derive(Clone, PartialEq, prost::Message)]
struct MessagePushedProto {
    #[prost(message, optional, tag = "1")]
    message: Option<MessageProto>,
}

#[derive(Clone, PartialEq, prost::Message)]
struct PacketPushedProto {
    #[prost(message, optional, tag = "1")]
    packet: Option<PacketProto>,
}

#[derive(Clone, PartialEq, prost::Message)]
struct TransientAcceptedProto {
    #[prost(uint64, tag = "1")]
    packet_id: u64,
    #[prost(int64, tag = "2")]
    source_node_id: i64,
    #[prost(int64, tag = "3")]
    target_node_id: i64,
    #[prost(message, optional, tag = "4")]
    recipient: Option<UserRefProto>,
    #[prost(int32, tag = "5")]
    delivery_mode: i32,
}

#[derive(Clone, PartialEq, prost::Message)]
struct SendMessageRequestProto {
    #[prost(uint64, tag = "1")]
    request_id: u64,
    #[prost(message, optional, tag = "2")]
    target: Option<UserRefProto>,
    #[prost(bytes = "vec", tag = "3")]
    body: Vec<u8>,
    #[prost(int32, tag = "4")]
    delivery_kind: i32,
    #[prost(int32, tag = "5")]
    delivery_mode: i32,
    #[prost(int32, tag = "6")]
    sync_mode: i32,
}

#[derive(Clone, PartialEq, prost::Message)]
struct SendMessageResponseProto {
    #[prost(uint64, tag = "1")]
    request_id: u64,
    #[prost(oneof = "send_message_response_proto::Body", tags = "2, 3")]
    body: Option<send_message_response_proto::Body>,
}

mod send_message_response_proto {
    #[derive(Clone, PartialEq, prost::Oneof)]
    pub enum Body {
        #[prost(message, tag = "2")]
        Message(super::MessageProto),
        #[prost(message, tag = "3")]
        TransientAccepted(super::TransientAcceptedProto),
    }
}

#[derive(Clone, PartialEq, prost::Message)]
struct AckMessageProto {
    #[prost(message, optional, tag = "1")]
    cursor: Option<MessageCursorProto>,
}

#[derive(Clone, PartialEq, prost::Message)]
struct PingProto {
    #[prost(uint64, tag = "1")]
    request_id: u64,
}

#[derive(Clone, PartialEq, prost::Message)]
struct PongProto {
    #[prost(uint64, tag = "1")]
    request_id: u64,
}

#[derive(Clone, PartialEq, prost::Message)]
struct ErrorProto {
    #[prost(string, tag = "1")]
    code: String,
    #[prost(string, tag = "2")]
    message: String,
    #[prost(uint64, tag = "3")]
    request_id: u64,
}

#[derive(Clone, PartialEq, prost::Message)]
struct CreateUserRequestProto {
    #[prost(uint64, tag = "1")]
    request_id: u64,
    #[prost(string, tag = "2")]
    username: String,
    #[prost(string, tag = "3")]
    password: String,
    #[prost(bytes = "vec", tag = "4")]
    profile_json: Vec<u8>,
    #[prost(string, tag = "5")]
    role: String,
}

#[derive(Clone, PartialEq, prost::Message)]
struct CreateUserResponseProto {
    #[prost(uint64, tag = "1")]
    request_id: u64,
    #[prost(message, optional, tag = "2")]
    user: Option<UserProto>,
}

#[derive(Clone, PartialEq, prost::Message)]
struct ClientEnvelopeProto {
    #[prost(oneof = "client_envelope_proto::Body", tags = "1, 2, 3, 4, 5")]
    body: Option<client_envelope_proto::Body>,
}

mod client_envelope_proto {
    #[derive(Clone, PartialEq, prost::Oneof)]
    pub enum Body {
        #[prost(message, tag = "1")]
        Login(super::LoginRequestProto),
        #[prost(message, tag = "2")]
        SendMessage(super::SendMessageRequestProto),
        #[prost(message, tag = "3")]
        AckMessage(super::AckMessageProto),
        #[prost(message, tag = "4")]
        Ping(super::PingProto),
        #[prost(message, tag = "5")]
        CreateUser(super::CreateUserRequestProto),
    }
}

#[derive(Clone, PartialEq, prost::Message)]
struct ServerEnvelopeProto {
    #[prost(oneof = "server_envelope_proto::Body", tags = "1, 2, 3, 4, 5, 6, 7")]
    body: Option<server_envelope_proto::Body>,
}

mod server_envelope_proto {
    #[derive(Clone, PartialEq, prost::Oneof)]
    pub enum Body {
        #[prost(message, tag = "1")]
        LoginResponse(super::LoginResponseProto),
        #[prost(message, tag = "2")]
        MessagePushed(super::MessagePushedProto),
        #[prost(message, tag = "3")]
        SendMessageResponse(super::SendMessageResponseProto),
        #[prost(message, tag = "4")]
        Error(super::ErrorProto),
        #[prost(message, tag = "5")]
        Pong(super::PongProto),
        #[prost(message, tag = "6")]
        PacketPushed(super::PacketPushedProto),
        #[prost(message, tag = "7")]
        CreateUserResponse(super::CreateUserResponseProto),
    }
}

#[derive(Default)]
struct RecordingState {
    saved: Vec<&'static str>,
}

#[derive(Clone, Default)]
struct RecordingStore {
    state: Arc<Mutex<RecordingState>>,
}

#[async_trait]
impl CursorStore for RecordingStore {
    async fn load_seen_messages(&self) -> Result<Vec<MessageCursor>, BoxError> {
        Ok(Vec::new())
    }

    async fn save_message(&self, _message: Message) -> Result<(), BoxError> {
        self.state.lock().await.saved.push("message");
        Ok(())
    }

    async fn save_cursor(&self, _cursor: MessageCursor) -> Result<(), BoxError> {
        self.state.lock().await.saved.push("cursor");
        Ok(())
    }
}

#[derive(Clone, Default)]
struct FailingStore;

#[async_trait]
impl CursorStore for FailingStore {
    async fn load_seen_messages(&self) -> Result<Vec<MessageCursor>, BoxError> {
        Ok(Vec::new())
    }

    async fn save_message(&self, _message: Message) -> Result<(), BoxError> {
        Err("boom".into())
    }

    async fn save_cursor(&self, _cursor: MessageCursor) -> Result<(), BoxError> {
        Ok(())
    }
}

async fn accept_ws(
    listener: &TcpListener,
    paths: Arc<StdMutex<Vec<String>>>,
) -> tokio_tungstenite::WebSocketStream<tokio::net::TcpStream> {
    let (stream, _) = listener.accept().await.unwrap();
    accept_hdr_async(stream, move |request: &Request, response: Response| {
        paths.lock().unwrap().push(request.uri().path().to_owned());
        Ok(response)
    })
    .await
    .unwrap()
}

async fn read_client_envelope(
    ws: &mut tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>,
) -> ClientEnvelopeProto {
    loop {
        match ws.next().await.unwrap().unwrap() {
            WsMessage::Binary(bytes) => return ClientEnvelopeProto::decode(bytes.as_ref()).unwrap(),
            WsMessage::Ping(_) | WsMessage::Pong(_) => continue,
            other => panic!("unexpected client frame: {other:?}"),
        }
    }
}

async fn send_server_envelope(
    ws: &mut tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>,
    env: ServerEnvelopeProto,
) {
    ws.send(WsMessage::Binary(env.encode_to_vec().into())).await.unwrap();
}

async fn next_event(
    subscription: &mut turntf::ClientSubscription,
) -> Result<ClientEvent, BroadcastStreamRecvError> {
    timeout(Duration::from_secs(2), subscription.next())
        .await
        .expect("timed out waiting for event")
        .expect("stream should yield event")
}

fn user_ref(node_id: i64, user_id: i64) -> UserRefProto {
    UserRefProto { node_id, user_id }
}

fn message_proto(seq: i64) -> MessageProto {
    MessageProto {
        recipient: Some(user_ref(4096, 1025)),
        node_id: 4096,
        seq,
        sender: Some(user_ref(4096, 1)),
        body: if seq == 8 {
            b"payload".to_vec()
        } else {
            vec![0xff, 0x00]
        },
        created_at_hlc: format!("hlc-{seq}"),
    }
}

#[tokio::test]
async fn client_login_message_ack_send_packet_create_user_and_ping() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let paths = Arc::new(StdMutex::new(Vec::new()));
    let paths_for_server = Arc::clone(&paths);

    let server = tokio::spawn(async move {
        let mut ws = accept_ws(&listener, paths_for_server).await;
        let login = read_client_envelope(&mut ws).await;
        let client_envelope_proto::Body::Login(login) = login.body.unwrap() else {
            panic!("expected login request");
        };
        assert_eq!(login.user.unwrap().user_id, 1025);
        assert_ne!(login.password, "alice-password");
        assert!(verify("alice-password", &login.password).unwrap());
        assert!(login.seen_messages.is_empty());

        send_server_envelope(
            &mut ws,
            ServerEnvelopeProto {
                body: Some(server_envelope_proto::Body::LoginResponse(LoginResponseProto {
                    user: Some(UserProto {
                        node_id: 4096,
                        user_id: 1025,
                        username: "alice".into(),
                        role: "user".into(),
                        profile_json: Vec::new(),
                        system_reserved: false,
                        created_at: String::new(),
                        updated_at: String::new(),
                        origin_node_id: 4096,
                    }),
                    protocol_version: "client-v1alpha1".into(),
                })),
            },
        )
        .await;

        send_server_envelope(
            &mut ws,
            ServerEnvelopeProto {
                body: Some(server_envelope_proto::Body::MessagePushed(MessagePushedProto {
                    message: Some(message_proto(7)),
                })),
            },
        )
        .await;

        let ack = read_client_envelope(&mut ws).await;
        let client_envelope_proto::Body::AckMessage(ack) = ack.body.unwrap() else {
            panic!("expected ack message");
        };
        let cursor = ack.cursor.unwrap();
        assert_eq!(cursor.node_id, 4096);
        assert_eq!(cursor.seq, 7);

        send_server_envelope(
            &mut ws,
            ServerEnvelopeProto {
                body: Some(server_envelope_proto::Body::PacketPushed(PacketPushedProto {
                    packet: Some(PacketProto {
                        packet_id: 99,
                        source_node_id: 4096,
                        target_node_id: 4096,
                        recipient: Some(user_ref(4096, 1025)),
                        sender: Some(user_ref(4096, 1)),
                        body: b"pkt".to_vec(),
                        delivery_mode: 1,
                    }),
                })),
            },
        )
        .await;

        let send_message = read_client_envelope(&mut ws).await;
        let client_envelope_proto::Body::SendMessage(send_message) = send_message.body.unwrap() else {
            panic!("expected send_message request");
        };
        assert_eq!(send_message.request_id, 1);
        assert_eq!(send_message.body, b"payload");
        send_server_envelope(
            &mut ws,
            ServerEnvelopeProto {
                body: Some(server_envelope_proto::Body::SendMessageResponse(
                    SendMessageResponseProto {
                        request_id: send_message.request_id,
                        body: Some(send_message_response_proto::Body::Message(message_proto(8))),
                    },
                )),
            },
        )
        .await;

        let send_packet = read_client_envelope(&mut ws).await;
        let client_envelope_proto::Body::SendMessage(send_packet) = send_packet.body.unwrap() else {
            panic!("expected transient send_message request");
        };
        assert_eq!(send_packet.delivery_kind, 2);
        assert_eq!(send_packet.delivery_mode, 2);
        send_server_envelope(
            &mut ws,
            ServerEnvelopeProto {
                body: Some(server_envelope_proto::Body::SendMessageResponse(
                    SendMessageResponseProto {
                        request_id: send_packet.request_id,
                        body: Some(send_message_response_proto::Body::TransientAccepted(
                            TransientAcceptedProto {
                                packet_id: 77,
                                source_node_id: 4096,
                                target_node_id: 8192,
                                recipient: Some(user_ref(8192, 1025)),
                                delivery_mode: 2,
                            },
                        )),
                    },
                )),
            },
        )
        .await;

        let create_user = read_client_envelope(&mut ws).await;
        let client_envelope_proto::Body::CreateUser(create_user) = create_user.body.unwrap() else {
            panic!("expected create_user request");
        };
        assert_eq!(create_user.username, "alice");
        assert!(verify("alice-password", &create_user.password).unwrap());
        send_server_envelope(
            &mut ws,
            ServerEnvelopeProto {
                body: Some(server_envelope_proto::Body::CreateUserResponse(
                    CreateUserResponseProto {
                        request_id: create_user.request_id,
                        user: Some(UserProto {
                            node_id: 4096,
                            user_id: 1026,
                            username: "alice".into(),
                            role: "user".into(),
                            profile_json: Vec::new(),
                            system_reserved: false,
                            created_at: String::new(),
                            updated_at: String::new(),
                            origin_node_id: 4096,
                        }),
                    },
                )),
            },
        )
        .await;

        let ping = read_client_envelope(&mut ws).await;
        let client_envelope_proto::Body::Ping(ping) = ping.body.unwrap() else {
            panic!("expected ping");
        };
        send_server_envelope(
            &mut ws,
            ServerEnvelopeProto {
                body: Some(server_envelope_proto::Body::Pong(PongProto {
                    request_id: ping.request_id,
                })),
            },
        )
        .await;
    });

    let store = RecordingStore::default();
    let config = Config {
        base_url: format!("http://{address}"),
        credentials: turntf::Credentials {
            node_id: 4096,
            user_id: 1025,
            password: plain_password("alice-password").unwrap(),
        },
        cursor_store: Arc::new(store.clone()),
        ping_interval: Duration::from_secs(3600),
        ..Config::new(
            format!("http://{address}"),
            turntf::Credentials {
                node_id: 4096,
                user_id: 1025,
                password: plain_password("alice-password").unwrap(),
            },
        )
    };
    let client = Client::new(config).unwrap();
    let mut events = client.subscribe().await;

    client.connect().await.unwrap();

    assert!(matches!(next_event(&mut events).await.unwrap(), ClientEvent::Login(_)));
    assert!(matches!(next_event(&mut events).await.unwrap(), ClientEvent::Message(message) if message.seq == 7));
    assert!(matches!(next_event(&mut events).await.unwrap(), ClientEvent::Packet(packet) if packet.packet_id == 99));

    let message = client
        .send_message(
            turntf::UserRef {
                node_id: 4096,
                user_id: 1025,
            },
            b"payload".to_vec(),
        )
        .await
        .unwrap();
    assert_eq!(message.seq, 8);

    let packet = client
        .send_packet(
            turntf::UserRef {
                node_id: 8192,
                user_id: 1025,
            },
            b"pkt".to_vec(),
            DeliveryMode::RouteRetry,
        )
        .await
        .unwrap();
    assert_eq!(packet.packet_id, 77);

    let user = client
        .create_user(CreateUserRequest {
            username: "alice".into(),
            password: Some(plain_password("alice-password").unwrap()),
            profile_json: Vec::new(),
            role: "user".into(),
        })
        .await
        .unwrap();
    assert_eq!(user.user_id, 1026);

    client.ping().await.unwrap();
    client.close().await.unwrap();
    server.await.unwrap();

    assert_eq!(&*paths.lock().unwrap(), &["/ws/client".to_string()]);
    assert_eq!(&store.state.lock().await.saved, &["message", "cursor", "message", "cursor"]);
}

#[tokio::test]
async fn client_reconnects_with_seen_messages_and_realtime_path() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let paths = Arc::new(StdMutex::new(Vec::new()));
    let paths_for_server = Arc::clone(&paths);
    let (seen_tx, seen_rx) = oneshot::channel();

    let server = tokio::spawn(async move {
        let mut first = accept_ws(&listener, Arc::clone(&paths_for_server)).await;
        let login = read_client_envelope(&mut first).await;
        let client_envelope_proto::Body::Login(login) = login.body.unwrap() else {
            panic!("expected first login");
        };
        assert!(login.transient_only);
        assert!(login.seen_messages.is_empty());
        send_server_envelope(
            &mut first,
            ServerEnvelopeProto {
                body: Some(server_envelope_proto::Body::LoginResponse(LoginResponseProto {
                    user: Some(UserProto {
                        node_id: 4096,
                        user_id: 1025,
                        username: "alice".into(),
                        role: "user".into(),
                        profile_json: Vec::new(),
                        system_reserved: false,
                        created_at: String::new(),
                        updated_at: String::new(),
                        origin_node_id: 4096,
                    }),
                    protocol_version: "client-v1alpha1".into(),
                })),
            },
        )
        .await;
        send_server_envelope(
            &mut first,
            ServerEnvelopeProto {
                body: Some(server_envelope_proto::Body::MessagePushed(MessagePushedProto {
                    message: Some(message_proto(7)),
                })),
            },
        )
        .await;
        let _ack = read_client_envelope(&mut first).await;
        first.close(None).await.unwrap();

        let mut second = accept_ws(&listener, paths_for_server).await;
        let login = read_client_envelope(&mut second).await;
        let client_envelope_proto::Body::Login(login) = login.body.unwrap() else {
            panic!("expected second login");
        };
        seen_tx
            .send(
                login.seen_messages
                    .into_iter()
                    .map(|cursor| (cursor.node_id, cursor.seq))
                    .collect::<Vec<_>>(),
            )
            .ok();
        send_server_envelope(
            &mut second,
            ServerEnvelopeProto {
                body: Some(server_envelope_proto::Body::LoginResponse(LoginResponseProto {
                    user: Some(UserProto {
                        node_id: 4096,
                        user_id: 1025,
                        username: "alice".into(),
                        role: "user".into(),
                        profile_json: Vec::new(),
                        system_reserved: false,
                        created_at: String::new(),
                        updated_at: String::new(),
                        origin_node_id: 4096,
                    }),
                    protocol_version: "client-v1alpha1".into(),
                })),
            },
        )
        .await;
        tokio::time::sleep(Duration::from_millis(200)).await;
    });

    let mut config = Config::new(
        format!("http://{address}/base"),
        turntf::Credentials {
            node_id: 4096,
            user_id: 1025,
            password: plain_password("alice-password").unwrap(),
        },
    );
    config.realtime_stream = true;
    config.transient_only = true;
    config.initial_reconnect_delay = Duration::from_millis(10);
    config.max_reconnect_delay = Duration::from_millis(20);
    config.ping_interval = Duration::from_secs(3600);
    config.cursor_store = Arc::new(MemoryCursorStore::new());

    let client = Client::new(config).unwrap();
    client.connect().await.unwrap();

    let seen = timeout(Duration::from_secs(2), seen_rx)
        .await
        .expect("timed out waiting for reconnect")
        .unwrap();
    assert_eq!(seen, vec![(4096, 7)]);

    client.close().await.unwrap();
    server.await.unwrap();

    assert_eq!(
        &*paths.lock().unwrap(),
        &["/base/ws/realtime".to_string(), "/base/ws/realtime".to_string()]
    );
}

#[tokio::test]
async fn client_does_not_ack_on_persist_failure_and_emits_error() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let paths = Arc::new(StdMutex::new(Vec::new()));
    let paths_for_server = Arc::clone(&paths);
    let (acked_tx, acked_rx) = oneshot::channel();

    let server = tokio::spawn(async move {
        let mut ws = accept_ws(&listener, paths_for_server).await;
        let _login = read_client_envelope(&mut ws).await;
        send_server_envelope(
            &mut ws,
            ServerEnvelopeProto {
                body: Some(server_envelope_proto::Body::LoginResponse(LoginResponseProto {
                    user: Some(UserProto {
                        node_id: 4096,
                        user_id: 1025,
                        username: "alice".into(),
                        role: "user".into(),
                        profile_json: Vec::new(),
                        system_reserved: false,
                        created_at: String::new(),
                        updated_at: String::new(),
                        origin_node_id: 4096,
                    }),
                    protocol_version: "client-v1alpha1".into(),
                })),
            },
        )
        .await;
        send_server_envelope(
            &mut ws,
            ServerEnvelopeProto {
                body: Some(server_envelope_proto::Body::MessagePushed(MessagePushedProto {
                    message: Some(message_proto(7)),
                })),
            },
        )
        .await;
        let acked = timeout(Duration::from_millis(200), read_client_envelope(&mut ws)).await.is_ok();
        acked_tx.send(acked).ok();
    });

    let mut config = Config::new(
        format!("http://{address}"),
        turntf::Credentials {
            node_id: 4096,
            user_id: 1025,
            password: plain_password("alice-password").unwrap(),
        },
    );
    config.cursor_store = Arc::new(FailingStore);
    config.ping_interval = Duration::from_secs(3600);
    let client = Client::new(config).unwrap();
    let mut events = client.subscribe().await;

    client.connect().await.unwrap();
    assert!(matches!(next_event(&mut events).await.unwrap(), ClientEvent::Login(_)));
    let error_event = next_event(&mut events).await.unwrap();
    assert!(matches!(error_event, ClientEvent::Error(turntf::Error::Store(_))));
    assert!(!acked_rx.await.unwrap());

    client.close().await.unwrap();
    server.await.unwrap();
}

#[tokio::test]
async fn client_subscription_reports_lagged_and_closes() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let paths = Arc::new(StdMutex::new(Vec::new()));
    let paths_for_server = Arc::clone(&paths);
    let (flood_tx, flood_rx) = oneshot::channel::<()>();

    let server = tokio::spawn(async move {
        let mut ws = accept_ws(&listener, paths_for_server).await;
        let _login = read_client_envelope(&mut ws).await;
        send_server_envelope(
            &mut ws,
            ServerEnvelopeProto {
                body: Some(server_envelope_proto::Body::LoginResponse(LoginResponseProto {
                    user: Some(UserProto {
                        node_id: 4096,
                        user_id: 1025,
                        username: "alice".into(),
                        role: "user".into(),
                        profile_json: Vec::new(),
                        system_reserved: false,
                        created_at: String::new(),
                        updated_at: String::new(),
                        origin_node_id: 4096,
                    }),
                    protocol_version: "client-v1alpha1".into(),
                })),
            },
        )
        .await;
        let _ = flood_rx.await;
        for packet_id in 1..=3 {
            send_server_envelope(
                &mut ws,
                ServerEnvelopeProto {
                    body: Some(server_envelope_proto::Body::PacketPushed(PacketPushedProto {
                        packet: Some(PacketProto {
                            packet_id,
                            source_node_id: 4096,
                            target_node_id: 4096,
                            recipient: Some(user_ref(4096, 1025)),
                            sender: Some(user_ref(4096, 1)),
                            body: b"pkt".to_vec(),
                            delivery_mode: 1,
                        }),
                    })),
                },
            )
            .await;
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
    });

    let mut config = Config::new(
        format!("http://{address}"),
        turntf::Credentials {
            node_id: 4096,
            user_id: 1025,
            password: plain_password("alice-password").unwrap(),
        },
    );
    config.event_channel_capacity = 1;
    config.ping_interval = Duration::from_secs(3600);
    let client = Client::new(config).unwrap();
    let mut events = client.subscribe().await;

    client.connect().await.unwrap();
    assert!(matches!(next_event(&mut events).await.unwrap(), ClientEvent::Login(_)));
    flood_tx.send(()).ok();

    match next_event(&mut events).await {
        Err(BroadcastStreamRecvError::Lagged(count)) => assert!(count >= 1),
        other => panic!("expected lagged event, got {other:?}"),
    }

    let packet_event = loop {
        match next_event(&mut events).await {
            Ok(ClientEvent::Packet(packet)) => break packet,
            Err(BroadcastStreamRecvError::Lagged(_)) => continue,
            other => panic!("expected packet event after lagged, got {other:?}"),
        }
    };
    assert!((1..=3).contains(&packet_event.packet_id));

    client.close().await.unwrap();

    loop {
        match timeout(Duration::from_secs(1), events.next()).await.unwrap() {
            Some(Ok(ClientEvent::Disconnect(_))) => continue,
            None => break,
            other => panic!("unexpected post-close event: {other:?}"),
        }
    }

    server.abort();
}
