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
    MemoryCursorStore, Message, MessageCursor, ScanUserMetadataRequest, SessionRef,
    UpsertUserMetadataRequest,
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
    #[prost(message, optional, tag = "3")]
    session_ref: Option<SessionRefProto>,
}

#[derive(Clone, PartialEq, prost::Message)]
struct SessionRefProto {
    #[prost(int64, tag = "1")]
    serving_node_id: i64,
    #[prost(string, tag = "2")]
    session_id: String,
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
    #[prost(message, optional, tag = "8")]
    target_session: Option<SessionRefProto>,
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
    #[prost(message, optional, tag = "6")]
    target_session: Option<SessionRefProto>,
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
    #[prost(message, optional, tag = "7")]
    target_session: Option<SessionRefProto>,
}

#[derive(Clone, PartialEq, prost::Message)]
struct OnlineNodePresenceProto {
    #[prost(int64, tag = "1")]
    serving_node_id: i64,
    #[prost(int32, tag = "2")]
    session_count: i32,
    #[prost(string, tag = "3")]
    transport_hint: String,
}

#[derive(Clone, PartialEq, prost::Message)]
struct ResolvedSessionProto {
    #[prost(message, optional, tag = "1")]
    session: Option<SessionRefProto>,
    #[prost(string, tag = "2")]
    transport: String,
    #[prost(bool, tag = "3")]
    transient_capable: bool,
}

#[derive(Clone, PartialEq, prost::Message)]
struct ResolveUserSessionsRequestProto {
    #[prost(uint64, tag = "1")]
    request_id: u64,
    #[prost(message, optional, tag = "2")]
    user: Option<UserRefProto>,
}

#[derive(Clone, PartialEq, prost::Message)]
struct ResolveUserSessionsResponseProto {
    #[prost(uint64, tag = "1")]
    request_id: u64,
    #[prost(message, optional, tag = "2")]
    user: Option<UserRefProto>,
    #[prost(message, repeated, tag = "3")]
    presence: Vec<OnlineNodePresenceProto>,
    #[prost(message, repeated, tag = "4")]
    items: Vec<ResolvedSessionProto>,
    #[prost(int32, tag = "5")]
    count: i32,
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
struct StringFieldProto {
    #[prost(string, tag = "1")]
    value: String,
}

#[derive(Clone, PartialEq, prost::Message)]
struct UserMetadataProto {
    #[prost(message, optional, tag = "1")]
    owner: Option<UserRefProto>,
    #[prost(string, tag = "2")]
    key: String,
    #[prost(bytes = "vec", tag = "3")]
    value: Vec<u8>,
    #[prost(string, tag = "4")]
    updated_at: String,
    #[prost(string, tag = "5")]
    deleted_at: String,
    #[prost(string, tag = "6")]
    expires_at: String,
    #[prost(int64, tag = "7")]
    origin_node_id: i64,
}

#[derive(Clone, PartialEq, prost::Message)]
struct GetUserMetadataRequestProto {
    #[prost(uint64, tag = "1")]
    request_id: u64,
    #[prost(message, optional, tag = "2")]
    owner: Option<UserRefProto>,
    #[prost(string, tag = "3")]
    key: String,
}

#[derive(Clone, PartialEq, prost::Message)]
struct UpsertUserMetadataRequestProto {
    #[prost(uint64, tag = "1")]
    request_id: u64,
    #[prost(message, optional, tag = "2")]
    owner: Option<UserRefProto>,
    #[prost(string, tag = "3")]
    key: String,
    #[prost(bytes = "vec", tag = "4")]
    value: Vec<u8>,
    #[prost(message, optional, tag = "5")]
    expires_at: Option<StringFieldProto>,
}

#[derive(Clone, PartialEq, prost::Message)]
struct DeleteUserMetadataRequestProto {
    #[prost(uint64, tag = "1")]
    request_id: u64,
    #[prost(message, optional, tag = "2")]
    owner: Option<UserRefProto>,
    #[prost(string, tag = "3")]
    key: String,
}

#[derive(Clone, PartialEq, prost::Message)]
struct ScanUserMetadataRequestProto {
    #[prost(uint64, tag = "1")]
    request_id: u64,
    #[prost(message, optional, tag = "2")]
    owner: Option<UserRefProto>,
    #[prost(string, tag = "3")]
    prefix: String,
    #[prost(string, tag = "4")]
    after: String,
    #[prost(int32, tag = "5")]
    limit: i32,
}

#[derive(Clone, PartialEq, prost::Message)]
struct GetUserMetadataResponseProto {
    #[prost(uint64, tag = "1")]
    request_id: u64,
    #[prost(message, optional, tag = "2")]
    metadata: Option<UserMetadataProto>,
}

#[derive(Clone, PartialEq, prost::Message)]
struct UpsertUserMetadataResponseProto {
    #[prost(uint64, tag = "1")]
    request_id: u64,
    #[prost(message, optional, tag = "2")]
    metadata: Option<UserMetadataProto>,
}

#[derive(Clone, PartialEq, prost::Message)]
struct DeleteUserMetadataResponseProto {
    #[prost(uint64, tag = "1")]
    request_id: u64,
    #[prost(message, optional, tag = "2")]
    metadata: Option<UserMetadataProto>,
}

#[derive(Clone, PartialEq, prost::Message)]
struct ScanUserMetadataResponseProto {
    #[prost(uint64, tag = "1")]
    request_id: u64,
    #[prost(message, repeated, tag = "2")]
    items: Vec<UserMetadataProto>,
    #[prost(int32, tag = "3")]
    count: i32,
    #[prost(string, tag = "4")]
    next_after: String,
}

#[derive(Clone, PartialEq, prost::Message)]
struct ClientEnvelopeProto {
    #[prost(
        oneof = "client_envelope_proto::Body",
        tags = "1, 2, 3, 4, 5, 18, 19, 20, 21, 22"
    )]
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
        #[prost(message, tag = "18")]
        ResolveUserSessions(super::ResolveUserSessionsRequestProto),
        #[prost(message, tag = "19")]
        GetUserMetadata(super::GetUserMetadataRequestProto),
        #[prost(message, tag = "20")]
        UpsertUserMetadata(super::UpsertUserMetadataRequestProto),
        #[prost(message, tag = "21")]
        DeleteUserMetadata(super::DeleteUserMetadataRequestProto),
        #[prost(message, tag = "22")]
        ScanUserMetadata(super::ScanUserMetadataRequestProto),
    }
}

#[derive(Clone, PartialEq, prost::Message)]
struct ServerEnvelopeProto {
    #[prost(
        oneof = "server_envelope_proto::Body",
        tags = "1, 2, 3, 4, 5, 6, 7, 20, 21, 22, 23, 24"
    )]
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
        #[prost(message, tag = "20")]
        ResolveUserSessionsResponse(super::ResolveUserSessionsResponseProto),
        #[prost(message, tag = "21")]
        GetUserMetadataResponse(super::GetUserMetadataResponseProto),
        #[prost(message, tag = "22")]
        UpsertUserMetadataResponse(super::UpsertUserMetadataResponseProto),
        #[prost(message, tag = "23")]
        DeleteUserMetadataResponse(super::DeleteUserMetadataResponseProto),
        #[prost(message, tag = "24")]
        ScanUserMetadataResponse(super::ScanUserMetadataResponseProto),
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
            WsMessage::Binary(bytes) => {
                return ClientEnvelopeProto::decode(bytes.as_ref()).unwrap()
            }
            WsMessage::Ping(_) | WsMessage::Pong(_) => continue,
            other => panic!("unexpected client frame: {other:?}"),
        }
    }
}

async fn send_server_envelope(
    ws: &mut tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>,
    env: ServerEnvelopeProto,
) {
    ws.send(WsMessage::Binary(env.encode_to_vec().into()))
        .await
        .unwrap();
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

fn session_ref(session_id: &str) -> SessionRefProto {
    SessionRefProto {
        serving_node_id: 4096,
        session_id: session_id.to_owned(),
    }
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

fn user_metadata_proto(updated_at: &str, deleted_at: &str) -> UserMetadataProto {
    UserMetadataProto {
        owner: Some(user_ref(4096, 1025)),
        key: "session:web:1".into(),
        value: vec![0x00, 0xfe],
        updated_at: updated_at.into(),
        deleted_at: deleted_at.into(),
        expires_at: "2026-05-01T12:00:00Z".into(),
        origin_node_id: 4096,
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
                body: Some(server_envelope_proto::Body::LoginResponse(
                    LoginResponseProto {
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
                        session_ref: Some(session_ref("session-main")),
                    },
                )),
            },
        )
        .await;

        send_server_envelope(
            &mut ws,
            ServerEnvelopeProto {
                body: Some(server_envelope_proto::Body::MessagePushed(
                    MessagePushedProto {
                        message: Some(message_proto(7)),
                    },
                )),
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
                body: Some(server_envelope_proto::Body::PacketPushed(
                    PacketPushedProto {
                        packet: Some(PacketProto {
                            packet_id: 99,
                            source_node_id: 4096,
                            target_node_id: 4096,
                            recipient: Some(user_ref(4096, 1025)),
                            sender: Some(user_ref(4096, 1)),
                            body: b"pkt".to_vec(),
                            delivery_mode: 1,
                            target_session: None,
                        }),
                    },
                )),
            },
        )
        .await;

        let send_message = read_client_envelope(&mut ws).await;
        let client_envelope_proto::Body::SendMessage(send_message) = send_message.body.unwrap()
        else {
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
        let client_envelope_proto::Body::SendMessage(send_packet) = send_packet.body.unwrap()
        else {
            panic!("expected transient send_message request");
        };
        assert_eq!(send_packet.delivery_kind, 2);
        assert_eq!(send_packet.delivery_mode, 2);
        assert!(send_packet.target_session.is_none());
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
                                target_session: None,
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

    let login_event = next_event(&mut events).await.unwrap();
    assert!(matches!(
        login_event,
        ClientEvent::Login(ref info) if info.session_ref.session_id == "session-main"
    ));
    assert!(
        matches!(next_event(&mut events).await.unwrap(), ClientEvent::Message(message) if message.seq == 7)
    );
    assert!(
        matches!(next_event(&mut events).await.unwrap(), ClientEvent::Packet(packet) if packet.packet_id == 99)
    );

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
    assert_eq!(
        &store.state.lock().await.saved,
        &["message", "cursor", "message", "cursor"]
    );
}

#[tokio::test]
async fn client_resolves_sessions_and_targets_transient_delivery() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let paths = Arc::new(StdMutex::new(Vec::new()));
    let paths_for_server = Arc::clone(&paths);

    let server = tokio::spawn(async move {
        let mut ws = accept_ws(&listener, paths_for_server).await;
        let login = read_client_envelope(&mut ws).await;
        let client_envelope_proto::Body::Login(_) = login.body.unwrap() else {
            panic!("expected login request");
        };

        send_server_envelope(
            &mut ws,
            ServerEnvelopeProto {
                body: Some(server_envelope_proto::Body::LoginResponse(
                    LoginResponseProto {
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
                        session_ref: Some(session_ref("login-session")),
                    },
                )),
            },
        )
        .await;

        let resolve = read_client_envelope(&mut ws).await;
        let client_envelope_proto::Body::ResolveUserSessions(resolve) = resolve.body.unwrap()
        else {
            panic!("expected resolve_user_sessions request");
        };
        assert_eq!(resolve.request_id, 1);
        assert_eq!(resolve.user, Some(user_ref(4096, 1025)));
        send_server_envelope(
            &mut ws,
            ServerEnvelopeProto {
                body: Some(server_envelope_proto::Body::ResolveUserSessionsResponse(
                    ResolveUserSessionsResponseProto {
                        request_id: resolve.request_id,
                        user: Some(user_ref(4096, 1025)),
                        presence: vec![OnlineNodePresenceProto {
                            serving_node_id: 4096,
                            session_count: 1,
                            transport_hint: "ws".into(),
                        }],
                        items: vec![ResolvedSessionProto {
                            session: Some(session_ref("target-session")),
                            transport: "ws".into(),
                            transient_capable: true,
                        }],
                        count: 1,
                    },
                )),
            },
        )
        .await;

        let send_packet = read_client_envelope(&mut ws).await;
        let client_envelope_proto::Body::SendMessage(send_packet) = send_packet.body.unwrap()
        else {
            panic!("expected targeted transient send_message request");
        };
        assert_eq!(send_packet.request_id, 2);
        assert_eq!(send_packet.delivery_kind, 2);
        assert_eq!(send_packet.delivery_mode, 1);
        let target_session = send_packet.target_session.expect("target_session");
        assert_eq!(target_session.session_id, "target-session");
        send_server_envelope(
            &mut ws,
            ServerEnvelopeProto {
                body: Some(server_envelope_proto::Body::SendMessageResponse(
                    SendMessageResponseProto {
                        request_id: send_packet.request_id,
                        body: Some(send_message_response_proto::Body::TransientAccepted(
                            TransientAcceptedProto {
                                packet_id: 88,
                                source_node_id: 4096,
                                target_node_id: 4096,
                                recipient: Some(user_ref(4096, 1025)),
                                delivery_mode: 1,
                                target_session: Some(session_ref("target-session")),
                            },
                        )),
                    },
                )),
            },
        )
        .await;

        send_server_envelope(
            &mut ws,
            ServerEnvelopeProto {
                body: Some(server_envelope_proto::Body::PacketPushed(
                    PacketPushedProto {
                        packet: Some(PacketProto {
                            packet_id: 88,
                            source_node_id: 4096,
                            target_node_id: 4096,
                            recipient: Some(user_ref(4096, 1025)),
                            sender: Some(user_ref(4096, 1)),
                            body: b"targeted".to_vec(),
                            delivery_mode: 1,
                            target_session: Some(session_ref("target-session")),
                        }),
                    },
                )),
            },
        )
        .await;
    });

    let mut config = Config::new(
        format!("http://{address}"),
        turntf::Credentials {
            node_id: 4096,
            user_id: 1025,
            password: plain_password("alice-password").unwrap(),
        },
    );
    config.ping_interval = Duration::from_secs(3600);
    let client = Client::new(config).unwrap();
    let mut events = client.subscribe().await;

    client.connect().await.unwrap();

    let login_info = match next_event(&mut events).await.unwrap() {
        ClientEvent::Login(info) => info,
        other => panic!("expected login event, got {other:?}"),
    };
    assert_eq!(login_info.session_ref.session_id, "login-session");

    let resolved = client
        .resolve_user_sessions(turntf::UserRef {
            node_id: 4096,
            user_id: 1025,
        })
        .await
        .unwrap();
    assert_eq!(resolved.user.node_id, 4096);
    assert_eq!(resolved.user.user_id, 1025);
    assert_eq!(resolved.presence.len(), 1);
    assert_eq!(resolved.presence[0].session_count, 1);
    let target_session = SessionRef {
        serving_node_id: 4096,
        session_id: "target-session".into(),
    };
    assert_eq!(resolved.sessions.len(), 1);
    assert_eq!(resolved.sessions[0].session, target_session);
    assert_eq!(resolved.sessions[0].transport, "ws");
    assert!(resolved.sessions[0].transient_capable);

    let accepted = client
        .send_packet_to_session(
            turntf::UserRef {
                node_id: 4096,
                user_id: 1025,
            },
            b"targeted".to_vec(),
            DeliveryMode::BestEffort,
            target_session.clone(),
        )
        .await
        .unwrap();
    assert_eq!(accepted.packet_id, 88);
    assert_eq!(accepted.target_session, Some(target_session.clone()));

    let packet = match next_event(&mut events).await.unwrap() {
        ClientEvent::Packet(packet) => packet,
        other => panic!("expected packet event, got {other:?}"),
    };
    assert_eq!(packet.packet_id, 88);
    assert_eq!(packet.target_session, Some(target_session));

    client.close().await.unwrap();
    server.await.unwrap();
}

#[tokio::test]
async fn client_user_metadata_crud_and_scan() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let paths = Arc::new(StdMutex::new(Vec::new()));
    let paths_for_server = Arc::clone(&paths);

    let server = tokio::spawn(async move {
        let mut ws = accept_ws(&listener, paths_for_server).await;
        let login = read_client_envelope(&mut ws).await;
        let client_envelope_proto::Body::Login(_) = login.body.unwrap() else {
            panic!("expected login request");
        };

        send_server_envelope(
            &mut ws,
            ServerEnvelopeProto {
                body: Some(server_envelope_proto::Body::LoginResponse(
                    LoginResponseProto {
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
                        session_ref: Some(session_ref("metadata-session")),
                    },
                )),
            },
        )
        .await;

        let upsert = read_client_envelope(&mut ws).await;
        let client_envelope_proto::Body::UpsertUserMetadata(upsert) = upsert.body.unwrap() else {
            panic!("expected upsert_user_metadata request");
        };
        assert_eq!(upsert.request_id, 1);
        assert_eq!(upsert.owner, Some(user_ref(4096, 1025)));
        assert_eq!(upsert.key, "session:web:1");
        assert_eq!(upsert.value, vec![0x00, 0xfe]);
        assert_eq!(
            upsert.expires_at.as_ref().map(|value| value.value.as_str()),
            Some("2026-05-01T12:00:00Z")
        );
        send_server_envelope(
            &mut ws,
            ServerEnvelopeProto {
                body: Some(server_envelope_proto::Body::UpsertUserMetadataResponse(
                    UpsertUserMetadataResponseProto {
                        request_id: upsert.request_id,
                        metadata: Some(user_metadata_proto("hlc-meta-upsert", "")),
                    },
                )),
            },
        )
        .await;

        let get = read_client_envelope(&mut ws).await;
        let client_envelope_proto::Body::GetUserMetadata(get) = get.body.unwrap() else {
            panic!("expected get_user_metadata request");
        };
        assert_eq!(get.request_id, 2);
        assert_eq!(get.owner, Some(user_ref(4096, 1025)));
        assert_eq!(get.key, "session:web:1");
        send_server_envelope(
            &mut ws,
            ServerEnvelopeProto {
                body: Some(server_envelope_proto::Body::GetUserMetadataResponse(
                    GetUserMetadataResponseProto {
                        request_id: get.request_id,
                        metadata: Some(user_metadata_proto("hlc-meta-get", "")),
                    },
                )),
            },
        )
        .await;

        let scan = read_client_envelope(&mut ws).await;
        let client_envelope_proto::Body::ScanUserMetadata(scan) = scan.body.unwrap() else {
            panic!("expected scan_user_metadata request");
        };
        assert_eq!(scan.request_id, 3);
        assert_eq!(scan.owner, Some(user_ref(4096, 1025)));
        assert_eq!(scan.prefix, "session:");
        assert_eq!(scan.after, "session:web:0");
        assert_eq!(scan.limit, 1);
        send_server_envelope(
            &mut ws,
            ServerEnvelopeProto {
                body: Some(server_envelope_proto::Body::ScanUserMetadataResponse(
                    ScanUserMetadataResponseProto {
                        request_id: scan.request_id,
                        items: vec![user_metadata_proto("hlc-meta-scan", "")],
                        count: 1,
                        next_after: "session:web:1".into(),
                    },
                )),
            },
        )
        .await;

        let delete = read_client_envelope(&mut ws).await;
        let client_envelope_proto::Body::DeleteUserMetadata(delete) = delete.body.unwrap() else {
            panic!("expected delete_user_metadata request");
        };
        assert_eq!(delete.request_id, 4);
        assert_eq!(delete.owner, Some(user_ref(4096, 1025)));
        assert_eq!(delete.key, "session:web:1");
        send_server_envelope(
            &mut ws,
            ServerEnvelopeProto {
                body: Some(server_envelope_proto::Body::DeleteUserMetadataResponse(
                    DeleteUserMetadataResponseProto {
                        request_id: delete.request_id,
                        metadata: Some(user_metadata_proto("hlc-meta-upsert", "hlc-meta-deleted")),
                    },
                )),
            },
        )
        .await;
    });

    let mut config = Config::new(
        format!("http://{address}"),
        turntf::Credentials {
            node_id: 4096,
            user_id: 1025,
            password: plain_password("alice-password").unwrap(),
        },
    );
    config.ping_interval = Duration::from_secs(3600);
    let client = Client::new(config).unwrap();

    client.connect().await.unwrap();

    let metadata = client
        .upsert_user_metadata(
            turntf::UserRef {
                node_id: 4096,
                user_id: 1025,
            },
            "session:web:1",
            UpsertUserMetadataRequest {
                value: vec![0x00, 0xfe],
                expires_at: Some("2026-05-01T12:00:00Z".into()),
            },
        )
        .await
        .unwrap();
    assert_eq!(metadata.value, vec![0x00, 0xfe]);
    assert_eq!(metadata.updated_at, "hlc-meta-upsert");
    assert_eq!(metadata.expires_at, "2026-05-01T12:00:00Z");

    let loaded = client
        .get_user_metadata(
            turntf::UserRef {
                node_id: 4096,
                user_id: 1025,
            },
            "session:web:1",
        )
        .await
        .unwrap();
    assert_eq!(loaded.updated_at, "hlc-meta-get");

    let scanned = client
        .scan_user_metadata(
            turntf::UserRef {
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
    assert_eq!(scanned.count, 1);
    assert_eq!(scanned.next_after, "session:web:1");
    assert_eq!(scanned.items.len(), 1);
    assert_eq!(scanned.items[0].updated_at, "hlc-meta-scan");

    let deleted = client
        .delete_user_metadata(
            turntf::UserRef {
                node_id: 4096,
                user_id: 1025,
            },
            "session:web:1",
        )
        .await
        .unwrap();
    assert_eq!(deleted.deleted_at, "hlc-meta-deleted");

    client.close().await.unwrap();
    server.await.unwrap();
}

#[tokio::test]
async fn client_user_metadata_validates_inputs_before_rpc() {
    let mut config = Config::new(
        "http://127.0.0.1:65535",
        turntf::Credentials {
            node_id: 4096,
            user_id: 1025,
            password: plain_password("alice-password").unwrap(),
        },
    );
    config.ping_interval = Duration::from_secs(3600);
    let client = Client::new(config).unwrap();

    let error = client
        .get_user_metadata(
            turntf::UserRef {
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
            turntf::UserRef {
                node_id: 4096,
                user_id: 1025,
            },
            ScanUserMetadataRequest {
                prefix: "bad/prefix".into(),
                after: String::new(),
                limit: 1,
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(error, turntf::Error::Validation(_)));

    let error = client
        .scan_user_metadata(
            turntf::UserRef {
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
                body: Some(server_envelope_proto::Body::LoginResponse(
                    LoginResponseProto {
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
                        session_ref: Some(session_ref("reconnect-first")),
                    },
                )),
            },
        )
        .await;
        send_server_envelope(
            &mut first,
            ServerEnvelopeProto {
                body: Some(server_envelope_proto::Body::MessagePushed(
                    MessagePushedProto {
                        message: Some(message_proto(7)),
                    },
                )),
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
                login
                    .seen_messages
                    .into_iter()
                    .map(|cursor| (cursor.node_id, cursor.seq))
                    .collect::<Vec<_>>(),
            )
            .ok();
        send_server_envelope(
            &mut second,
            ServerEnvelopeProto {
                body: Some(server_envelope_proto::Body::LoginResponse(
                    LoginResponseProto {
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
                        session_ref: Some(session_ref("reconnect-second")),
                    },
                )),
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
        &[
            "/base/ws/realtime".to_string(),
            "/base/ws/realtime".to_string()
        ]
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
                body: Some(server_envelope_proto::Body::LoginResponse(
                    LoginResponseProto {
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
                        session_ref: Some(session_ref("persist-failure")),
                    },
                )),
            },
        )
        .await;
        send_server_envelope(
            &mut ws,
            ServerEnvelopeProto {
                body: Some(server_envelope_proto::Body::MessagePushed(
                    MessagePushedProto {
                        message: Some(message_proto(7)),
                    },
                )),
            },
        )
        .await;
        let acked = timeout(Duration::from_millis(200), read_client_envelope(&mut ws))
            .await
            .is_ok();
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
    assert!(matches!(
        next_event(&mut events).await.unwrap(),
        ClientEvent::Login(_)
    ));
    let error_event = next_event(&mut events).await.unwrap();
    assert!(matches!(
        error_event,
        ClientEvent::Error(turntf::Error::Store(_))
    ));
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
                body: Some(server_envelope_proto::Body::LoginResponse(
                    LoginResponseProto {
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
                        session_ref: Some(session_ref("lagged-subscription")),
                    },
                )),
            },
        )
        .await;
        let _ = flood_rx.await;
        for packet_id in 1..=3 {
            send_server_envelope(
                &mut ws,
                ServerEnvelopeProto {
                    body: Some(server_envelope_proto::Body::PacketPushed(
                        PacketPushedProto {
                            packet: Some(PacketProto {
                                packet_id,
                                source_node_id: 4096,
                                target_node_id: 4096,
                                recipient: Some(user_ref(4096, 1025)),
                                sender: Some(user_ref(4096, 1)),
                                body: b"pkt".to_vec(),
                                delivery_mode: 1,
                                target_session: None,
                            }),
                        },
                    )),
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
    assert!(matches!(
        next_event(&mut events).await.unwrap(),
        ClientEvent::Login(_)
    ));
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
        match timeout(Duration::from_secs(1), events.next())
            .await
            .unwrap()
        {
            Some(Ok(ClientEvent::Disconnect(_))) => continue,
            None => break,
            other => panic!("unexpected post-close event: {other:?}"),
        }
    }

    server.abort();
}
