use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use reqwest::{Client as ReqwestClient, Method};
use serde_json::{Map, Value};

use crate::errors::{Error, Result, ServerError};
use crate::mapping::{
    blacklist_entry_from_http, cluster_node_from_http, delete_user_result_from_http,
    event_from_http, expect_object, items_from_payload, json_bytes_to_value,
    logged_in_user_from_http, message_from_http, operations_status_from_http,
    relay_accepted_from_http, subscription_from_http, user_from_http,
};
use crate::password::{plain_password, PasswordInput};
use crate::types::{
    BlacklistEntry, ClusterNode, CreateUserRequest, DeleteUserResult, DeliveryMode, Event,
    LoggedInUser, Message, OperationsStatus, RelayAccepted, Subscription, UpdateUserRequest, User,
    UserRef,
};
use crate::validation::{validate_delivery_mode, validate_positive_i64, validate_user_ref};

#[derive(Clone, Debug)]
pub struct HttpClient {
    base_url: String,
    client: ReqwestClient,
}

impl HttpClient {
    pub fn new(base_url: impl Into<String>) -> Result<Self> {
        let client = ReqwestClient::builder()
            .no_proxy()
            .build()
            .map_err(|err| Error::connection("http client build", err.to_string()))?;
        Self::with_client(base_url, client)
    }

    pub(crate) fn with_client(base_url: impl Into<String>, client: ReqwestClient) -> Result<Self> {
        let base_url = base_url.into().trim_end_matches('/').to_string();
        if base_url.trim().is_empty() {
            return Err(Error::validation("base_url is required"));
        }
        Ok(Self { base_url, client })
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub async fn login(&self, node_id: i64, user_id: i64, password: impl AsRef<str>) -> Result<String> {
        self.login_with_password(node_id, user_id, plain_password(password.as_ref())?)
            .await
    }

    pub async fn login_with_password(
        &self,
        node_id: i64,
        user_id: i64,
        password: PasswordInput,
    ) -> Result<String> {
        validate_positive_i64(node_id, "node_id")?;
        validate_positive_i64(user_id, "user_id")?;

        let response = self
            .request_json(
                Method::POST,
                "/auth/login",
                "",
                Some(Value::Object(Map::from_iter([
                    ("node_id".into(), Value::from(node_id)),
                    ("user_id".into(), Value::from(user_id)),
                    (
                        "password".into(),
                        Value::from(password.wire_value()?.to_owned()),
                    ),
                ]))),
                &[200],
            )
            .await?;
        let object = expect_object(&response)?;
        match object.get("token").and_then(Value::as_str) {
            Some(token) if !token.is_empty() => Ok(token.to_owned()),
            _ => Err(Error::protocol("empty token in login response")),
        }
    }

    pub async fn create_user(&self, token: &str, request: CreateUserRequest) -> Result<User> {
        if request.username.is_empty() {
            return Err(Error::validation("username is required"));
        }
        if request.role.is_empty() {
            return Err(Error::validation("role is required"));
        }

        let mut body = Map::new();
        body.insert("username".into(), Value::from(request.username));
        body.insert("role".into(), Value::from(request.role));
        if let Some(password) = request.password {
            body.insert("password".into(), Value::from(password.wire_value()?.to_owned()));
        }
        if !request.profile_json.is_empty() {
            body.insert(
                "profile".into(),
                json_bytes_to_value(&request.profile_json, "profile_json")?,
            );
        }

        let response = self
            .request_json(Method::POST, "/users", token, Some(Value::Object(body)), &[200, 201])
            .await?;
        user_from_http(expect_object(&response)?)
    }

    pub async fn create_channel(&self, token: &str, mut request: CreateUserRequest) -> Result<User> {
        if request.role.is_empty() {
            request.role = "channel".to_owned();
        }
        self.create_user(token, request).await
    }

    pub async fn get_user(&self, token: &str, target: UserRef) -> Result<User> {
        validate_user_ref(&target, "target")?;
        let response = self
            .request_json(
                Method::GET,
                &format!("/nodes/{}/users/{}", target.node_id, target.user_id),
                token,
                None,
                &[200],
            )
            .await?;
        user_from_http(expect_object(&response)?)
    }

    pub async fn update_user(
        &self,
        token: &str,
        target: UserRef,
        request: UpdateUserRequest,
    ) -> Result<User> {
        validate_user_ref(&target, "target")?;
        let mut body = Map::new();
        if let Some(username) = request.username {
            body.insert("username".into(), Value::from(username));
        }
        if let Some(password) = request.password {
            body.insert("password".into(), Value::from(password.wire_value()?.to_owned()));
        }
        if let Some(profile_json) = request.profile_json {
            body.insert(
                "profile".into(),
                json_bytes_to_value(&profile_json, "profile_json")?,
            );
        }
        if let Some(role) = request.role {
            body.insert("role".into(), Value::from(role));
        }

        let response = self
            .request_json(
                Method::PATCH,
                &format!("/nodes/{}/users/{}", target.node_id, target.user_id),
                token,
                Some(Value::Object(body)),
                &[200],
            )
            .await?;
        user_from_http(expect_object(&response)?)
    }

    pub async fn delete_user(&self, token: &str, target: UserRef) -> Result<DeleteUserResult> {
        validate_user_ref(&target, "target")?;
        let response = self
            .request_json(
                Method::DELETE,
                &format!("/nodes/{}/users/{}", target.node_id, target.user_id),
                token,
                None,
                &[200],
            )
            .await?;
        delete_user_result_from_http(expect_object(&response)?)
    }

    pub async fn create_subscription(
        &self,
        token: &str,
        user: UserRef,
        channel: UserRef,
    ) -> Result<Subscription> {
        validate_user_ref(&user, "user")?;
        validate_user_ref(&channel, "channel")?;
        let response = self
            .request_json(
                Method::POST,
                &format!("/nodes/{}/users/{}/subscriptions", user.node_id, user.user_id),
                token,
                Some(Value::Object(Map::from_iter([
                    ("channel_node_id".into(), Value::from(channel.node_id)),
                    ("channel_user_id".into(), Value::from(channel.user_id)),
                ]))),
                &[200, 201],
            )
            .await?;
        subscription_from_http(expect_object(&response)?)
    }

    pub async fn subscribe_channel(
        &self,
        token: &str,
        subscriber: UserRef,
        channel: UserRef,
    ) -> Result<Subscription> {
        self.create_subscription(token, subscriber, channel).await
    }

    pub async fn unsubscribe_channel(
        &self,
        token: &str,
        subscriber: UserRef,
        channel: UserRef,
    ) -> Result<Subscription> {
        validate_user_ref(&subscriber, "subscriber")?;
        validate_user_ref(&channel, "channel")?;
        let response = self
            .request_json(
                Method::DELETE,
                &format!(
                    "/nodes/{}/users/{}/subscriptions/{}/{}",
                    subscriber.node_id, subscriber.user_id, channel.node_id, channel.user_id
                ),
                token,
                None,
                &[200],
            )
            .await?;
        subscription_from_http(expect_object(&response)?)
    }

    pub async fn list_subscriptions(&self, token: &str, subscriber: UserRef) -> Result<Vec<Subscription>> {
        validate_user_ref(&subscriber, "subscriber")?;
        let response = self
            .request_json(
                Method::GET,
                &format!("/nodes/{}/users/{}/subscriptions", subscriber.node_id, subscriber.user_id),
                token,
                None,
                &[200],
            )
            .await?;
        items_from_payload(&response, &["items"])?
            .into_iter()
            .map(expect_object)
            .map(|value| value.and_then(subscription_from_http))
            .collect()
    }

    pub async fn list_messages(&self, token: &str, target: UserRef, limit: i32) -> Result<Vec<Message>> {
        validate_user_ref(&target, "target")?;
        let suffix = if limit > 0 {
            format!("?limit={limit}")
        } else {
            String::new()
        };
        let response = self
            .request_json(
                Method::GET,
                &format!("/nodes/{}/users/{}/messages{suffix}", target.node_id, target.user_id),
                token,
                None,
                &[200],
            )
            .await?;
        items_from_payload(&response, &["items"])?
            .into_iter()
            .map(expect_object)
            .map(|value| value.and_then(message_from_http))
            .collect()
    }

    pub async fn post_message(&self, token: &str, target: UserRef, body: Vec<u8>) -> Result<Message> {
        validate_user_ref(&target, "target")?;
        if body.is_empty() {
            return Err(Error::validation("body is required"));
        }

        let response = self
            .request_json(
                Method::POST,
                &format!("/nodes/{}/users/{}/messages", target.node_id, target.user_id),
                token,
                Some(Value::Object(Map::from_iter([(
                    "body".into(),
                    Value::from(STANDARD.encode(body)),
                )]))),
                &[200, 201],
            )
            .await?;
        message_from_http(expect_object(&response)?)
    }

    pub async fn post_packet(
        &self,
        token: &str,
        target_node_id: i64,
        relay_target: UserRef,
        body: Vec<u8>,
        mode: DeliveryMode,
    ) -> Result<RelayAccepted> {
        validate_positive_i64(target_node_id, "target_node_id")?;
        validate_user_ref(&relay_target, "relay_target")?;
        validate_delivery_mode(mode)?;
        if target_node_id != relay_target.node_id {
            return Err(Error::validation(format!(
                "target node ID {target_node_id} does not match target user node_id {}",
                relay_target.node_id
            )));
        }
        if body.is_empty() {
            return Err(Error::validation("body is required"));
        }

        let response = self
            .request_json(
                Method::POST,
                &format!(
                    "/nodes/{}/users/{}/messages",
                    relay_target.node_id, relay_target.user_id
                ),
                token,
                Some(Value::Object(Map::from_iter([
                    ("body".into(), Value::from(STANDARD.encode(body))),
                    ("delivery_kind".into(), Value::from("transient")),
                    ("delivery_mode".into(), Value::from(mode.as_str())),
                ]))),
                &[202],
            )
            .await?;
        relay_accepted_from_http(expect_object(&response)?)
    }

    pub async fn list_cluster_nodes(&self, token: &str) -> Result<Vec<ClusterNode>> {
        let response = self
            .request_json(Method::GET, "/cluster/nodes", token, None, &[200])
            .await?;
        items_from_payload(&response, &["nodes", "items"])?
            .into_iter()
            .map(expect_object)
            .map(|value| value.and_then(cluster_node_from_http))
            .collect()
    }

    pub async fn list_node_logged_in_users(&self, token: &str, node_id: i64) -> Result<Vec<LoggedInUser>> {
        validate_positive_i64(node_id, "node_id")?;
        let response = self
            .request_json(
                Method::GET,
                &format!("/cluster/nodes/{node_id}/logged-in-users"),
                token,
                None,
                &[200],
            )
            .await?;
        items_from_payload(&response, &["items"])?
            .into_iter()
            .map(expect_object)
            .map(|value| value.and_then(logged_in_user_from_http))
            .collect()
    }

    pub async fn block_user(&self, token: &str, owner: UserRef, blocked: UserRef) -> Result<BlacklistEntry> {
        validate_user_ref(&owner, "owner")?;
        validate_user_ref(&blocked, "blocked")?;
        let response = self
            .request_json(
                Method::POST,
                &format!("/nodes/{}/users/{}/blacklist", owner.node_id, owner.user_id),
                token,
                Some(Value::Object(Map::from_iter([
                    ("blocked_node_id".into(), Value::from(blocked.node_id)),
                    ("blocked_user_id".into(), Value::from(blocked.user_id)),
                ]))),
                &[200, 201],
            )
            .await?;
        blacklist_entry_from_http(expect_object(&response)?)
    }

    pub async fn unblock_user(
        &self,
        token: &str,
        owner: UserRef,
        blocked: UserRef,
    ) -> Result<BlacklistEntry> {
        validate_user_ref(&owner, "owner")?;
        validate_user_ref(&blocked, "blocked")?;
        let response = self
            .request_json(
                Method::DELETE,
                &format!(
                    "/nodes/{}/users/{}/blacklist/{}/{}",
                    owner.node_id, owner.user_id, blocked.node_id, blocked.user_id
                ),
                token,
                None,
                &[200],
            )
            .await?;
        blacklist_entry_from_http(expect_object(&response)?)
    }

    pub async fn list_blocked_users(&self, token: &str, owner: UserRef) -> Result<Vec<BlacklistEntry>> {
        validate_user_ref(&owner, "owner")?;
        let response = self
            .request_json(
                Method::GET,
                &format!("/nodes/{}/users/{}/blacklist", owner.node_id, owner.user_id),
                token,
                None,
                &[200],
            )
            .await?;
        items_from_payload(&response, &["items"])?
            .into_iter()
            .map(expect_object)
            .map(|value| value.and_then(blacklist_entry_from_http))
            .collect()
    }

    pub async fn list_events(&self, token: &str, after: i64, limit: i32) -> Result<Vec<Event>> {
        let mut params = Vec::new();
        if after != 0 {
            params.push(format!("after={after}"));
        }
        if limit > 0 {
            params.push(format!("limit={limit}"));
        }
        let suffix = if params.is_empty() {
            String::new()
        } else {
            format!("?{}", params.join("&"))
        };
        let response = self
            .request_json(Method::GET, &format!("/events{suffix}"), token, None, &[200])
            .await?;
        items_from_payload(&response, &["items"])?
            .into_iter()
            .map(expect_object)
            .map(|value| value.and_then(event_from_http))
            .collect()
    }

    pub async fn operations_status(&self, token: &str) -> Result<OperationsStatus> {
        let response = self
            .request_json(Method::GET, "/ops/status", token, None, &[200])
            .await?;
        operations_status_from_http(expect_object(&response)?)
    }

    pub async fn metrics(&self, token: &str) -> Result<String> {
        self.request_text(Method::GET, "/metrics", token, None, &[200]).await
    }

    async fn request_json(
        &self,
        method: Method,
        path: &str,
        token: &str,
        body: Option<Value>,
        statuses: &[u16],
    ) -> Result<Value> {
        let text = self.request_text(method, path, token, body, statuses).await?;
        if text.trim().is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_str(&text)
            .map_err(|err| Error::protocol(format!("invalid JSON response: {err}")))
    }

    async fn request_text(
        &self,
        method: Method,
        path: &str,
        token: &str,
        body: Option<Value>,
        statuses: &[u16],
    ) -> Result<String> {
        let op = format!("{} {}", method.as_str(), path);
        let mut request = self.client.request(method, format!("{}{}", self.base_url, path));
        if !token.is_empty() {
            request = request.bearer_auth(token);
        }
        if let Some(body) = body {
            request = request.json(&body);
        }

        let response = request
            .send()
            .await
            .map_err(|err| Error::connection(op.clone(), err.to_string()))?;
        let status = response.status().as_u16();
        let text = response
            .text()
            .await
            .map_err(|err| Error::connection(op.clone(), err.to_string()))?;
        if statuses.contains(&status) {
            return Ok(text);
        }

        if let Ok(Value::Object(value)) = serde_json::from_str::<Value>(&text) {
            let code = value
                .get("code")
                .and_then(Value::as_str)
                .unwrap_or_else(|| response_status_code(status));
            let message = value
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or_else(|| text.trim());
            let request_id = value.get("request_id").and_then(Value::as_u64).unwrap_or(0);
            return Err(ServerError::new(code, message, request_id).into());
        }

        Err(ServerError::new(response_status_code(status), text.trim(), 0).into())
    }
}

fn response_status_code(status: u16) -> &'static str {
    match status {
        400 => "bad_request",
        401 => "unauthorized",
        403 => "forbidden",
        404 => "not_found",
        409 => "conflict",
        422 => "unprocessable_entity",
        429 => "too_many_requests",
        500 => "internal_error",
        503 => "service_unavailable",
        _ => "http_error",
    }
}
