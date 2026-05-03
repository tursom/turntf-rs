use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use reqwest::{Client as ReqwestClient, Method};
use serde_json::{Map, Value};
use url::form_urlencoded;

use crate::errors::{Error, Result, ServerError};
use crate::mapping::{
    attachment_from_http, blacklist_entry_from_attachment, cluster_node_from_http,
    delete_user_result_from_http, event_from_http, expect_object, items_from_payload,
    json_bytes_to_value, logged_in_user_from_http, message_from_http, operations_status_from_http,
    relay_accepted_from_http, subscription_from_attachment, user_from_http,
    user_metadata_from_http, user_metadata_scan_result_from_http,
};
use crate::password::{plain_password, PasswordInput};
use crate::types::{
    Attachment, AttachmentType, BlacklistEntry, ClusterNode, CreateUserRequest, DeleteUserResult,
    DeliveryMode, Event, ListUsersRequest, LoggedInUser, Message, OperationsStatus, RelayAccepted,
    ScanUserMetadataRequest, Subscription, UpdateUserRequest, UpsertUserMetadataRequest, User,
    UserMetadata, UserMetadataScanResult, UserRef,
};
use crate::validation::{
    normalize_login_name, normalize_optional_filter, validate_delivery_mode, validate_login_name,
    validate_optional_user_metadata_key_fragment, validate_optional_user_ref,
    validate_positive_i64, validate_user_metadata_key, validate_user_metadata_scan_limit,
    validate_user_ref,
};

/// HTTP 客户端，用于通过 REST API 与 turntf 服务器通信。
///
/// `HttpClient` 提供了基于 HTTP REST 的 API 操作，包括用户认证、用户管理、
/// 元数据管理、消息收发、频道订阅和集群状态查询等功能。
///
/// 每个 API 请求需要提供认证令牌（token），可通过 `login` 或 `login_by_login_name` 获取。
///
/// # 示例
///
/// ```ignore
/// use turntf::HttpClient;
///
/// let client = HttpClient::new("http://localhost:8080")?;
/// let token = client.login(1, 42, "password").await?;
/// ```
#[derive(Clone, Debug)]
pub struct HttpClient {
    base_url: String,
    client: ReqwestClient,
}

impl HttpClient {
    /// 创建一个新的 `HttpClient` 实例。
    ///
    /// 默认使用 `reqwest` 客户端，禁用系统代理。
    ///
    /// # 参数
    /// - `base_url` - 服务器的基础 URL，如 `"http://localhost:8080"`
    ///
    /// # Errors
    /// 如果 `base_url` 为空或空字符串，返回 `Error::Validation`。
    /// 如果 HTTP 客户端初始化失败，返回 `Error::Connection`。
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

    /// 获取当前 HTTP 客户端的基础 URL。
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// 使用节点 ID 和用户 ID 进行登录。
    ///
    /// 这是使用明文密码的便捷方法，内部会自动对密码进行 bcrypt 哈希处理。
    ///
    /// # 参数
    /// - `node_id` - 节点 ID（必须为正数）
    /// - `user_id` - 用户 ID（必须为正数）
    /// - `password` - 明文密码
    ///
    /// # 返回值
    /// 成功时返回认证令牌字符串。
    ///
    /// # Errors
    /// 如果 `node_id` 或 `user_id` 无效（小于等于 0），返回 `Error::Validation`。
    /// 如果密码为空或服务器认证失败，返回相应的错误。
    pub async fn login(
        &self,
        node_id: i64,
        user_id: i64,
        password: impl AsRef<str>,
    ) -> Result<String> {
        self.login_with_password(node_id, user_id, plain_password(password.as_ref())?)
            .await
    }

    /// 使用节点 ID、用户 ID 和已处理的密码输入进行登录。
    ///
    /// # 参数
    /// - `node_id` - 节点 ID（必须为正数）
    /// - `user_id` - 用户 ID（必须为正数）
    /// - `password` - 密码输入（可使用 `plain_password()` 或 `hashed_password()` 创建）
    ///
    /// # 返回值
    /// 成功时返回认证令牌字符串。
    ///
    /// # Errors
    /// 如果 `node_id` 或 `user_id` 无效，返回 `Error::Validation`。
    /// 如果服务器认证失败，返回相应的错误。
    pub async fn login_with_password(
        &self,
        node_id: i64,
        user_id: i64,
        password: PasswordInput,
    ) -> Result<String> {
        validate_positive_i64(node_id, "node_id")?;
        validate_positive_i64(user_id, "user_id")?;

        self.login_with_body(Map::from_iter([
            ("node_id".into(), Value::from(node_id)),
            ("user_id".into(), Value::from(user_id)),
            (
                "password".into(),
                Value::from(password.wire_value()?.to_owned()),
            ),
        ]))
        .await
    }

    /// 使用登录名和明文密码进行登录。
    ///
    /// # 参数
    /// - `login_name` - 用户的登录名
    /// - `password` - 明文密码
    ///
    /// # 返回值
    /// 成功时返回认证令牌字符串。
    ///
    /// # Errors
    /// 如果登录名为空，返回 `Error::Validation`。
    /// 如果服务器认证失败，返回相应的错误。
    pub async fn login_by_login_name(
        &self,
        login_name: impl AsRef<str>,
        password: impl AsRef<str>,
    ) -> Result<String> {
        self.login_by_login_name_with_password(
            login_name.as_ref(),
            plain_password(password.as_ref())?,
        )
        .await
    }

    /// 使用登录名和已处理的密码输入进行登录。
    ///
    /// # 参数
    /// - `login_name` - 用户的登录名
    /// - `password` - 密码输入
    ///
    /// # 返回值
    /// 成功时返回认证令牌字符串。
    ///
    /// # Errors
    /// 如果登录名为空，返回 `Error::Validation`。
    /// 如果服务器认证失败，返回相应的错误。
    pub async fn login_by_login_name_with_password(
        &self,
        login_name: impl AsRef<str>,
        password: PasswordInput,
    ) -> Result<String> {
        let login_name = normalize_login_name(login_name.as_ref());
        validate_login_name(&login_name, "login_name")?;
        self.login_with_body(Map::from_iter([
            ("login_name".into(), Value::from(login_name)),
            (
                "password".into(),
                Value::from(password.wire_value()?.to_owned()),
            ),
        ]))
        .await
    }

    /// 创建新用户。
    ///
    /// # 参数
    /// - `token` - 认证令牌
    /// - `request` - 创建用户的请求参数
    ///
    /// # Errors
    /// 如果 `username` 或 `role` 为空，返回 `Error::Validation`。
    /// 如果角色为 `"channel"` 但提供了 `login_name`，返回 `Error::Validation`。
    /// 如果服务器处理失败，返回相应的错误。
    pub async fn create_user(&self, token: &str, request: CreateUserRequest) -> Result<User> {
        let CreateUserRequest {
            username,
            login_name,
            password,
            profile_json,
            role,
        } = request;
        if username.is_empty() {
            return Err(Error::validation("username is required"));
        }
        if role.is_empty() {
            return Err(Error::validation("role is required"));
        }
        let login_name = login_name
            .map(|value| normalize_login_name(&value))
            .filter(|value| !value.is_empty());
        if role.trim() == "channel" && login_name.is_some() {
            return Err(Error::validation(
                "login_name requires a login-enabled user",
            ));
        }

        let mut body = Map::new();
        body.insert("username".into(), Value::from(username));
        body.insert("role".into(), Value::from(role));
        if let Some(login_name) = login_name {
            body.insert("login_name".into(), Value::from(login_name));
        }
        if let Some(password) = password {
            body.insert(
                "password".into(),
                Value::from(password.wire_value()?.to_owned()),
            );
        }
        if !profile_json.is_empty() {
            body.insert(
                "profile".into(),
                json_bytes_to_value(&profile_json, "profile_json")?,
            );
        }

        let response = self
            .request_json(
                Method::POST,
                "/users",
                token,
                Some(Value::Object(body)),
                &[200, 201],
            )
            .await?;
        user_from_http(expect_object(&response)?)
    }

    /// 创建频道（角色为 `"channel"` 的用户）。
    ///
    /// 这是 `create_user` 的便捷方法，会自动将角色设置为 `"channel"`。
    ///
    /// # 参数
    /// - `token` - 认证令牌
    /// - `request` - 创建用户的请求参数（角色会被自动覆盖为 `"channel"`）
    ///
    /// # Errors
    /// 同 `create_user`。
    pub async fn create_channel(
        &self,
        token: &str,
        mut request: CreateUserRequest,
    ) -> Result<User> {
        if request.role.is_empty() {
            request.role = "channel".to_owned();
        }
        self.create_user(token, request).await
    }

    /// 获取用户信息。
    ///
    /// # 参数
    /// - `token` - 认证令牌
    /// - `target` - 目标用户引用
    ///
    /// # Errors
    /// 如果 `target` 的 `node_id` 或 `user_id` 无效，返回 `Error::Validation`。
    /// 如果用户不存在或权限不足，返回相应的服务器错误。
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

    /// 更新用户信息。
    ///
    /// 所有字段均为可选，只更新提供的字段。未提供的字段保持不变。
    ///
    /// # 参数
    /// - `token` - 认证令牌
    /// - `target` - 目标用户引用
    /// - `request` - 更新请求，包含要更新的字段
    ///
    /// # Errors
    /// 如果 `target` 无效，返回 `Error::Validation`。
    /// 如果角色为 `"channel"` 但提供了 `login_name`，返回 `Error::Validation`。
    pub async fn update_user(
        &self,
        token: &str,
        target: UserRef,
        request: UpdateUserRequest,
    ) -> Result<User> {
        validate_user_ref(&target, "target")?;
        let UpdateUserRequest {
            username,
            login_name,
            password,
            profile_json,
            role,
        } = request;
        let login_name = login_name.map(|value| normalize_login_name(&value));
        if role.as_deref().map(str::trim) == Some("channel")
            && matches!(login_name.as_deref(), Some(value) if !value.is_empty())
        {
            return Err(Error::validation(
                "login_name requires a login-enabled user",
            ));
        }
        let mut body = Map::new();
        if let Some(username) = username {
            body.insert("username".into(), Value::from(username));
        }
        if let Some(login_name) = login_name {
            body.insert("login_name".into(), Value::from(login_name));
        }
        if let Some(password) = password {
            body.insert(
                "password".into(),
                Value::from(password.wire_value()?.to_owned()),
            );
        }
        if let Some(profile_json) = profile_json {
            body.insert(
                "profile".into(),
                json_bytes_to_value(&profile_json, "profile_json")?,
            );
        }
        if let Some(role) = role {
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

    /// 删除用户。
    ///
    /// # 参数
    /// - `token` - 认证令牌
    /// - `target` - 要删除的目标用户引用
    ///
    /// # Errors
    /// 如果 `target` 无效，返回 `Error::Validation`。
    /// 如果用户不存在或权限不足，返回相应的服务器错误。
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

    /// 获取当前用户可通讯的活跃用户列表。
    ///
    /// # 参数
    /// - `token` - 认证令牌
    /// - `request` - 过滤条件；`name` 为空时不按名称过滤，`uid={0,0}` 时不按 uid 过滤
    ///
    /// # Errors
    /// 如果 `uid` 只填写了一部分或包含非正数，返回 `Error::Validation`。
    pub async fn list_users(&self, token: &str, request: ListUsersRequest) -> Result<Vec<User>> {
        let ListUsersRequest { name, uid } = request;
        let name = normalize_optional_filter(&name);
        let has_uid = validate_optional_user_ref(&uid, "request.uid")?;

        let mut query = form_urlencoded::Serializer::new(String::new());
        if let Some(name) = name {
            query.append_pair("name", &name);
        }
        if has_uid {
            query.append_pair("uid", &format!("{}:{}", uid.node_id, uid.user_id));
        }
        let query = query.finish();
        let path = if query.is_empty() {
            "/users".to_owned()
        } else {
            format!("/users?{query}")
        };

        let response = self
            .request_json(Method::GET, &path, token, None, &[200])
            .await?;
        items_from_payload(&response, &["items"])?
            .into_iter()
            .map(expect_object)
            .map(|value| value.and_then(user_from_http))
            .collect()
    }

    /// 获取用户元数据。
    ///
    /// # 参数
    /// - `token` - 认证令牌
    /// - `owner` - 元数据所属的用户
    /// - `key` - 元数据键名
    ///
    /// # Errors
    /// 如果 `owner` 无效或 `key` 格式不正确，返回 `Error::Validation`。
    pub async fn get_user_metadata(
        &self,
        token: &str,
        owner: UserRef,
        key: impl Into<String>,
    ) -> Result<UserMetadata> {
        validate_user_ref(&owner, "owner")?;
        let key = key.into();
        validate_user_metadata_key(&key, "key")?;
        let response = self
            .request_json(
                Method::GET,
                &format!(
                    "/nodes/{}/users/{}/metadata/{key}",
                    owner.node_id, owner.user_id
                ),
                token,
                None,
                &[200],
            )
            .await?;
        user_metadata_from_http(expect_object(&response)?)
    }

    /// 创建或更新用户元数据。
    ///
    /// 如果指定键的元数据已存在，则更新其值；否则创建新的元数据条目。
    ///
    /// # 参数
    /// - `token` - 认证令牌
    /// - `owner` - 元数据所属的用户
    /// - `key` - 元数据键名
    /// - `request` - 元数据值及过期时间
    ///
    /// # Errors
    /// 如果 `owner` 无效或 `key` 格式不正确，返回 `Error::Validation`。
    pub async fn upsert_user_metadata(
        &self,
        token: &str,
        owner: UserRef,
        key: impl Into<String>,
        request: UpsertUserMetadataRequest,
    ) -> Result<UserMetadata> {
        validate_user_ref(&owner, "owner")?;
        let key = key.into();
        validate_user_metadata_key(&key, "key")?;
        let mut body =
            Map::from_iter([("value".into(), Value::from(STANDARD.encode(request.value)))]);
        if let Some(expires_at) = request.expires_at {
            body.insert("expires_at".into(), Value::from(expires_at));
        }

        let response = self
            .request_json(
                Method::PUT,
                &format!(
                    "/nodes/{}/users/{}/metadata/{key}",
                    owner.node_id, owner.user_id
                ),
                token,
                Some(Value::Object(body)),
                &[200, 201],
            )
            .await?;
        user_metadata_from_http(expect_object(&response)?)
    }

    /// 删除用户元数据。
    ///
    /// # 参数
    /// - `token` - 认证令牌
    /// - `owner` - 元数据所属的用户
    /// - `key` - 要删除的元数据键名
    ///
    /// # Errors
    /// 如果 `owner` 无效或 `key` 格式不正确，返回 `Error::Validation`。
    pub async fn delete_user_metadata(
        &self,
        token: &str,
        owner: UserRef,
        key: impl Into<String>,
    ) -> Result<UserMetadata> {
        validate_user_ref(&owner, "owner")?;
        let key = key.into();
        validate_user_metadata_key(&key, "key")?;
        let response = self
            .request_json(
                Method::DELETE,
                &format!(
                    "/nodes/{}/users/{}/metadata/{key}",
                    owner.node_id, owner.user_id
                ),
                token,
                None,
                &[200],
            )
            .await?;
        user_metadata_from_http(expect_object(&response)?)
    }

    /// 扫描用户元数据。
    ///
    /// 支持按前缀过滤和分页查询。
    ///
    /// # 参数
    /// - `token` - 认证令牌
    /// - `owner` - 元数据所属的用户
    /// - `request` - 扫描参数（前缀、分页游标、限制数）
    ///
    /// # Errors
    /// 如果参数无效（如 limit 超出范围），返回 `Error::Validation`。
    pub async fn scan_user_metadata(
        &self,
        token: &str,
        owner: UserRef,
        request: ScanUserMetadataRequest,
    ) -> Result<UserMetadataScanResult> {
        validate_user_ref(&owner, "owner")?;
        validate_optional_user_metadata_key_fragment(&request.prefix, "prefix")?;
        validate_optional_user_metadata_key_fragment(&request.after, "after")?;
        validate_user_metadata_scan_limit(request.limit)?;

        let mut params = Vec::new();
        if !request.prefix.is_empty() {
            params.push(format!("prefix={}", request.prefix));
        }
        if !request.after.is_empty() {
            params.push(format!("after={}", request.after));
        }
        if request.limit > 0 {
            params.push(format!("limit={}", request.limit));
        }
        let suffix = if params.is_empty() {
            String::new()
        } else {
            format!("?{}", params.join("&"))
        };
        let response = self
            .request_json(
                Method::GET,
                &format!(
                    "/nodes/{}/users/{}/metadata{suffix}",
                    owner.node_id, owner.user_id
                ),
                token,
                None,
                &[200],
            )
            .await?;
        user_metadata_scan_result_from_http(expect_object(&response)?)
    }

    /// 创建频道订阅关系。
    ///
    /// # 参数
    /// - `token` - 认证令牌
    /// - `user` - 订阅者
    /// - `channel` - 被订阅的频道
    ///
    /// # Errors
    /// 如果用户引用无效，返回 `Error::Validation`。
    pub async fn create_subscription(
        &self,
        token: &str,
        user: UserRef,
        channel: UserRef,
    ) -> Result<Subscription> {
        let attachment = self
            .upsert_attachment(
                token,
                user,
                channel,
                AttachmentType::ChannelSubscription,
                b"{}".to_vec(),
            )
            .await?;
        Ok(subscription_from_attachment(&attachment))
    }

    /// 订阅频道（`create_subscription` 的别名）。
    pub async fn subscribe_channel(
        &self,
        token: &str,
        subscriber: UserRef,
        channel: UserRef,
    ) -> Result<Subscription> {
        self.create_subscription(token, subscriber, channel).await
    }

    /// 取消订阅频道。
    ///
    /// # 参数
    /// - `token` - 认证令牌
    /// - `subscriber` - 订阅者
    /// - `channel` - 要取消订阅的频道
    ///
    /// # Errors
    /// 如果用户引用无效，返回 `Error::Validation`。
    pub async fn unsubscribe_channel(
        &self,
        token: &str,
        subscriber: UserRef,
        channel: UserRef,
    ) -> Result<Subscription> {
        let attachment = self
            .delete_attachment(
                token,
                subscriber,
                channel,
                AttachmentType::ChannelSubscription,
            )
            .await?;
        Ok(subscription_from_attachment(&attachment))
    }

    /// 获取用户的所有频道订阅列表。
    ///
    /// # 参数
    /// - `token` - 认证令牌
    /// - `subscriber` - 订阅者
    ///
    /// # Errors
    /// 如果用户引用无效，返回 `Error::Validation`。
    pub async fn list_subscriptions(
        &self,
        token: &str,
        subscriber: UserRef,
    ) -> Result<Vec<Subscription>> {
        self.list_attachments(token, subscriber, Some(AttachmentType::ChannelSubscription))
            .await?
            .into_iter()
            .map(|attachment| Ok(subscription_from_attachment(&attachment)))
            .collect()
    }

    /// 获取目标用户的消息列表。
    ///
    /// # 参数
    /// - `token` - 认证令牌
    /// - `target` - 目标用户（支持 `node_id=0, user_id=0` 表示当前用户）
    /// - `limit` - 返回消息的最大数量（0 表示使用服务器默认值）
    /// - `peer_node_id` - 可选的会话对方节点 ID，用于 session 查询
    /// - `peer_user_id` - 可选的会话对方用户 ID，用于 session 查询
    ///
    /// # Errors
    /// 如果 `target` 的 node_id 或 user_id 为负数，返回 `Error::Validation`。
    pub async fn list_messages(
        &self,
        token: &str,
        target: UserRef,
        limit: i32,
        peer_node_id: Option<i64>,
        peer_user_id: Option<i64>,
    ) -> Result<Vec<Message>> {
        if target.node_id < 0 {
            return Err(Error::validation("target.node_id must not be negative"));
        }
        if target.user_id < 0 {
            return Err(Error::validation("target.user_id must not be negative"));
        }
        let mut suffix = String::new();
        if limit > 0 {
            suffix.push_str(&format!("?limit={limit}"));
        }
        if let (Some(peer_nid), Some(peer_uid)) = (peer_node_id, peer_user_id) {
            let sep = if suffix.is_empty() { "?" } else { "&" };
            suffix.push_str(&format!(
                "{sep}peer_node_id={peer_nid}&peer_user_id={peer_uid}"
            ));
        }
        let response = self
            .request_json(
                Method::GET,
                &format!(
                    "/nodes/{}/users/{}/messages{suffix}",
                    target.node_id, target.user_id
                ),
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

    /// 发送持久化消息。
    ///
    /// 消息会被服务器持久化存储，并通过 WebSocket 推送给目标用户。
    ///
    /// # 参数
    /// - `token` - 认证令牌
    /// - `target` - 目标用户
    /// - `body` - 消息体内容（字节数组，base64 编码传输）
    ///
    /// # Errors
    /// 如果 `target` 无效或 `body` 为空，返回 `Error::Validation`。
    pub async fn post_message(
        &self,
        token: &str,
        target: UserRef,
        body: Vec<u8>,
    ) -> Result<Message> {
        validate_user_ref(&target, "target")?;
        if body.is_empty() {
            return Err(Error::validation("body is required"));
        }

        let response = self
            .request_json(
                Method::POST,
                &format!(
                    "/nodes/{}/users/{}/messages",
                    target.node_id, target.user_id
                ),
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

    /// 发送瞬时数据包（非持久化消息）。
    ///
    /// 数据包不会被持久化存储，适用于实时通信场景。
    ///
    /// # 参数
    /// - `token` - 认证令牌
    /// - `target_node_id` - 目标节点 ID
    /// - `relay_target` - 中继目标用户
    /// - `body` - 数据包体内容（字节数组）
    /// - `mode` - 投递模式（`BestEffort` 或 `RouteRetry`）
    ///
    /// # Errors
    /// 如果参数无效，返回 `Error::Validation`。
    /// `target_node_id` 必须与 `relay_target.node_id` 一致。
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

    /// 获取集群节点列表。
    ///
    /// # 参数
    /// - `token` - 认证令牌
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

    /// 获取指定节点的已登录用户列表。
    ///
    /// # 参数
    /// - `token` - 认证令牌
    /// - `node_id` - 要查询的节点 ID
    ///
    /// # Errors
    /// 如果 `node_id` 无效（小于等于 0），返回 `Error::Validation`。
    pub async fn list_node_logged_in_users(
        &self,
        token: &str,
        node_id: i64,
    ) -> Result<Vec<LoggedInUser>> {
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

    /// 使用请求体执行登录（内部方法）。
    async fn login_with_body(&self, body: Map<String, Value>) -> Result<String> {
        let response = self
            .request_json(
                Method::POST,
                "/auth/login",
                "",
                Some(Value::Object(body)),
                &[200],
            )
            .await?;
        let object = expect_object(&response)?;
        match object.get("token").and_then(Value::as_str) {
            Some(token) if !token.is_empty() => Ok(token.to_owned()),
            _ => Err(Error::protocol("empty token in login response")),
        }
    }

    /// 屏蔽用户。
    ///
    /// 将指定用户加入黑名单。被屏蔽的用户将无法向屏蔽者发送消息。
    ///
    /// # 参数
    /// - `token` - 认证令牌
    /// - `owner` - 黑名单所有者
    /// - `blocked` - 要被屏蔽的用户
    ///
    /// # Errors
    /// 如果用户引用无效，返回 `Error::Validation`。
    pub async fn block_user(
        &self,
        token: &str,
        owner: UserRef,
        blocked: UserRef,
    ) -> Result<BlacklistEntry> {
        let attachment = self
            .upsert_attachment(
                token,
                owner,
                blocked,
                AttachmentType::UserBlacklist,
                b"{}".to_vec(),
            )
            .await?;
        Ok(blacklist_entry_from_attachment(&attachment))
    }

    /// 解除用户屏蔽。
    ///
    /// # 参数
    /// - `token` - 认证令牌
    /// - `owner` - 黑名单所有者
    /// - `blocked` - 要解除屏蔽的用户
    ///
    /// # Errors
    /// 如果用户引用无效，返回 `Error::Validation`。
    pub async fn unblock_user(
        &self,
        token: &str,
        owner: UserRef,
        blocked: UserRef,
    ) -> Result<BlacklistEntry> {
        let attachment = self
            .delete_attachment(token, owner, blocked, AttachmentType::UserBlacklist)
            .await?;
        Ok(blacklist_entry_from_attachment(&attachment))
    }

    /// 获取用户的黑名单列表。
    ///
    /// # 参数
    /// - `token` - 认证令牌
    /// - `owner` - 黑名单所有者
    ///
    /// # Errors
    /// 如果用户引用无效，返回 `Error::Validation`。
    pub async fn list_blocked_users(
        &self,
        token: &str,
        owner: UserRef,
    ) -> Result<Vec<BlacklistEntry>> {
        self.list_attachments(token, owner, Some(AttachmentType::UserBlacklist))
            .await?
            .into_iter()
            .map(|attachment| Ok(blacklist_entry_from_attachment(&attachment)))
            .collect()
    }

    /// 创建或更新用户附件。
    ///
    /// 附件用于记录用户之间的关联关系，如频道订阅、黑名单等。
    ///
    /// # 参数
    /// - `token` - 认证令牌
    /// - `owner` - 附件所有者
    /// - `subject` - 附件主题用户
    /// - `attachment_type` - 附件类型
    /// - `config_json` - 附件的 JSON 配置（空字节数组表示空对象）
    ///
    /// # Errors
    /// 如果用户引用无效或 `config_json` 不是有效 JSON，返回 `Error::Validation`。
    pub async fn upsert_attachment(
        &self,
        token: &str,
        owner: UserRef,
        subject: UserRef,
        attachment_type: AttachmentType,
        config_json: Vec<u8>,
    ) -> Result<Attachment> {
        validate_user_ref(&owner, "owner")?;
        validate_user_ref(&subject, "subject")?;
        let config_json = if config_json.is_empty() {
            Value::Object(Map::new())
        } else {
            json_bytes_to_value(&config_json, "config_json")?
        };
        let response = self
            .request_json(
                Method::PUT,
                &format!(
                    "/nodes/{}/users/{}/attachments/{}/{}/{}",
                    owner.node_id,
                    owner.user_id,
                    attachment_type.as_str(),
                    subject.node_id,
                    subject.user_id
                ),
                token,
                Some(Value::Object(Map::from_iter([(
                    "config_json".into(),
                    config_json,
                )]))),
                &[200, 201],
            )
            .await?;
        attachment_from_http(expect_object(&response)?)
    }

    /// 删除用户附件。
    ///
    /// # 参数
    /// - `token` - 认证令牌
    /// - `owner` - 附件所有者
    /// - `subject` - 附件主题用户
    /// - `attachment_type` - 附件类型
    ///
    /// # Errors
    /// 如果用户引用无效，返回 `Error::Validation`。
    pub async fn delete_attachment(
        &self,
        token: &str,
        owner: UserRef,
        subject: UserRef,
        attachment_type: AttachmentType,
    ) -> Result<Attachment> {
        validate_user_ref(&owner, "owner")?;
        validate_user_ref(&subject, "subject")?;
        let response = self
            .request_json(
                Method::DELETE,
                &format!(
                    "/nodes/{}/users/{}/attachments/{}/{}/{}",
                    owner.node_id,
                    owner.user_id,
                    attachment_type.as_str(),
                    subject.node_id,
                    subject.user_id
                ),
                token,
                None,
                &[200],
            )
            .await?;
        attachment_from_http(expect_object(&response)?)
    }

    /// 获取用户的附件列表。
    ///
    /// # 参数
    /// - `token` - 认证令牌
    /// - `owner` - 附件所有者
    /// - `attachment_type` - 可选的附件类型过滤（`None` 表示返回所有类型）
    ///
    /// # Errors
    /// 如果用户引用无效，返回 `Error::Validation`。
    pub async fn list_attachments(
        &self,
        token: &str,
        owner: UserRef,
        attachment_type: Option<AttachmentType>,
    ) -> Result<Vec<Attachment>> {
        validate_user_ref(&owner, "owner")?;
        let suffix = attachment_type
            .map(|value| format!("?attachment_type={}", value.as_str()))
            .unwrap_or_default();
        let response = self
            .request_json(
                Method::GET,
                &format!(
                    "/nodes/{}/users/{}/attachments{suffix}",
                    owner.node_id, owner.user_id
                ),
                token,
                None,
                &[200],
            )
            .await?;
        items_from_payload(&response, &["items"])?
            .into_iter()
            .map(expect_object)
            .map(|value| value.and_then(attachment_from_http))
            .collect()
    }

    /// 获取事件日志列表。
    ///
    /// # 参数
    /// - `token` - 认证令牌
    /// - `after` - 起始事件序列号（0 表示从最新开始）
    /// - `limit` - 返回事件的最大数量（0 表示使用服务器默认值）
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
            .request_json(
                Method::GET,
                &format!("/events{suffix}"),
                token,
                None,
                &[200],
            )
            .await?;
        items_from_payload(&response, &["items"])?
            .into_iter()
            .map(expect_object)
            .map(|value| value.and_then(event_from_http))
            .collect()
    }

    /// 获取集群节点的运行状态。
    ///
    /// # 参数
    /// - `token` - 认证令牌
    pub async fn operations_status(&self, token: &str) -> Result<OperationsStatus> {
        let response = self
            .request_json(Method::GET, "/ops/status", token, None, &[200])
            .await?;
        operations_status_from_http(expect_object(&response)?)
    }

    /// 获取节点的监控指标文本。
    ///
    /// # 参数
    /// - `token` - 认证令牌
    ///
    /// # 返回值
    /// 返回 Prometheus 格式的监控指标文本。
    pub async fn metrics(&self, token: &str) -> Result<String> {
        self.request_text(Method::GET, "/metrics", token, None, &[200])
            .await
    }

    /// 发送 HTTP 请求并解析 JSON 响应。
    async fn request_json(
        &self,
        method: Method,
        path: &str,
        token: &str,
        body: Option<Value>,
        statuses: &[u16],
    ) -> Result<Value> {
        let text = self
            .request_text(method, path, token, body, statuses)
            .await?;
        if text.trim().is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_str(&text)
            .map_err(|err| Error::protocol(format!("invalid JSON response: {err}")))
    }

    /// 发送 HTTP 请求并获取原始文本响应。
    ///
    /// 这是所有 HTTP 请求调用的核心方法。处理认证头、请求体序列化、
    /// 响应状态码检查和错误转换。
    async fn request_text(
        &self,
        method: Method,
        path: &str,
        token: &str,
        body: Option<Value>,
        statuses: &[u16],
    ) -> Result<String> {
        let op = format!("{} {}", method.as_str(), path);
        let mut request = self
            .client
            .request(method, format!("{}{}", self.base_url, path));
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

/// 将 HTTP 状态码转换为对应的错误码字符串。
///
/// # 参数
/// - `status` - HTTP 状态码
///
/// # 返回值
/// 对应的错误码字符串，如 `"bad_request"`、`"unauthorized"` 等。
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
