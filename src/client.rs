use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashMap},
    error::Error,
    future::Future,
    net::IpAddr,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use thiserror::Error;
use tokio::sync::{Mutex, Semaphore, broadcast, mpsc, oneshot, watch};
use tokio_tungstenite::{
    connect_async_with_config,
    tungstenite::{Message, protocol::WebSocketConfig},
};
use url::Url;

use crate::identity::{IdentityError, NodeIdentity};

const PROTOCOL_VERSION: u32 = 4;
const MINIMUM_NODE_PROTOCOL_VERSION: u32 = 3;
const DEFAULT_CHALLENGE_TIMEOUT: Duration = Duration::from_secs(15);
const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const DEFAULT_MAX_MESSAGE_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct NodeClientConfig {
    gateway_url: String,
    challenge_timeout: Duration,
    request_timeout: Duration,
    max_message_bytes: usize,
    event_capacity: usize,
    max_in_flight: usize,
}

impl NodeClientConfig {
    #[must_use]
    pub fn new(gateway_url: impl Into<String>) -> Self {
        Self {
            gateway_url: gateway_url.into(),
            challenge_timeout: DEFAULT_CHALLENGE_TIMEOUT,
            request_timeout: DEFAULT_REQUEST_TIMEOUT,
            max_message_bytes: DEFAULT_MAX_MESSAGE_BYTES,
            event_capacity: 256,
            max_in_flight: 64,
        }
    }

    #[must_use]
    pub fn challenge_timeout(mut self, timeout: Duration) -> Self {
        self.challenge_timeout = timeout;
        self
    }

    #[must_use]
    pub fn request_timeout(mut self, timeout: Duration) -> Self {
        self.request_timeout = timeout;
        self
    }

    #[must_use]
    pub fn max_message_bytes(mut self, bytes: usize) -> Self {
        self.max_message_bytes = bytes;
        self
    }

    #[must_use]
    pub fn event_capacity(mut self, capacity: usize) -> Self {
        self.event_capacity = capacity;
        self
    }

    #[must_use]
    pub fn max_in_flight(mut self, maximum: usize) -> Self {
        self.max_in_flight = maximum;
        self
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceProof {
    id: String,
    public_key: String,
    signature: String,
    signed_at: u64,
    nonce: String,
}

impl DeviceProof {
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        public_key: impl Into<String>,
        signature: impl Into<String>,
        signed_at: u64,
        nonce: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            public_key: public_key.into(),
            signature: signature.into(),
            signed_at,
            nonce: nonce.into(),
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectAuth {
    #[serde(skip_serializing_if = "Option::is_none")]
    token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    bootstrap_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    device_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    password: Option<String>,
}

impl ConnectAuth {
    #[must_use]
    pub fn token(value: impl Into<String>) -> Self {
        Self::single(Some(value.into()), None, None, None)
    }

    #[must_use]
    pub fn bootstrap_token(value: impl Into<String>) -> Self {
        Self::single(None, Some(value.into()), None, None)
    }

    #[must_use]
    pub fn device_token(value: impl Into<String>) -> Self {
        Self::single(None, None, Some(value.into()), None)
    }

    #[must_use]
    pub fn password(value: impl Into<String>) -> Self {
        Self::single(None, None, None, Some(value.into()))
    }

    fn single(
        token: Option<String>,
        bootstrap_token: Option<String>,
        device_token: Option<String>,
        password: Option<String>,
    ) -> Self {
        Self {
            token,
            bootstrap_token,
            device_token,
            password,
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ClientInfo {
    id: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    display_name: Option<String>,
    version: String,
    platform: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    device_family: Option<String>,
    mode: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    instance_id: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeConnectOptions {
    min_protocol: u32,
    max_protocol: u32,
    client: ClientInfo,
    #[serde(rename = "caps", skip_serializing_if = "Vec::is_empty")]
    advertised_caps: Vec<String>,
    #[serde(rename = "commands", skip_serializing_if = "Vec::is_empty")]
    advertised_commands: Vec<String>,
    #[serde(rename = "permissions", skip_serializing_if = "BTreeMap::is_empty")]
    advertised_permissions: BTreeMap<String, bool>,
    #[serde(skip)]
    declared_caps: Vec<String>,
    #[serde(skip)]
    declared_commands: Vec<String>,
    #[serde(skip)]
    declared_permissions: BTreeMap<String, bool>,
    #[serde(skip)]
    activated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    path_env: Option<String>,
    role: &'static str,
    scopes: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    device: Option<DeviceProof>,
    #[serde(skip)]
    identity: Option<NodeIdentity>,
    #[serde(skip_serializing_if = "Option::is_none")]
    auth: Option<ConnectAuth>,
    #[serde(skip_serializing_if = "Option::is_none")]
    locale: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    user_agent: Option<String>,
}

impl NodeConnectOptions {
    #[must_use]
    pub fn new(version: impl Into<String>, platform: impl Into<String>) -> Self {
        Self {
            min_protocol: MINIMUM_NODE_PROTOCOL_VERSION,
            max_protocol: PROTOCOL_VERSION,
            client: ClientInfo {
                id: "node-host",
                display_name: None,
                version: version.into(),
                platform: platform.into(),
                device_family: None,
                mode: "node",
                instance_id: None,
            },
            advertised_caps: Vec::new(),
            advertised_commands: Vec::new(),
            advertised_permissions: BTreeMap::new(),
            declared_caps: Vec::new(),
            declared_commands: Vec::new(),
            declared_permissions: BTreeMap::new(),
            activated: false,
            path_env: None,
            role: "node",
            scopes: Vec::new(),
            device: None,
            identity: None,
            auth: None,
            locale: None,
            user_agent: None,
        }
    }

    #[must_use]
    pub fn display_name(mut self, value: impl Into<String>) -> Self {
        self.client.display_name = Some(value.into());
        self
    }

    #[must_use]
    pub fn instance_id(mut self, value: impl Into<String>) -> Self {
        self.client.instance_id = Some(value.into());
        self
    }

    #[must_use]
    pub fn device_family(mut self, value: impl Into<String>) -> Self {
        self.client.device_family = Some(value.into());
        self
    }

    #[must_use]
    pub fn capability(mut self, value: impl Into<String>) -> Self {
        self.declared_caps.push(value.into());
        self
    }

    #[must_use]
    pub fn command(mut self, value: impl Into<String>) -> Self {
        self.declared_commands.push(value.into());
        self
    }

    #[must_use]
    pub fn permission(mut self, name: impl Into<String>, allowed: bool) -> Self {
        self.declared_permissions.insert(name.into(), allowed);
        self
    }

    /// Advertise the declared node surface on this connection.
    ///
    /// Declarations are withheld by default so an embedding can establish its
    /// own readiness before asking the Gateway to approve or expose them.
    /// Gateway approval remains authoritative and is intentionally not inferred
    /// from a successful `hello-ok` response.
    #[must_use]
    pub fn activate(mut self) -> Self {
        self.activated = true;
        self
    }

    #[must_use]
    pub fn path_env(mut self, value: impl Into<String>) -> Self {
        self.path_env = Some(value.into());
        self
    }

    #[must_use]
    pub fn device(mut self, value: DeviceProof) -> Self {
        self.device = Some(value);
        self.identity = None;
        self
    }

    /// Use a library-managed Ed25519 identity for the final connect signature.
    #[must_use]
    pub fn identity(mut self, value: NodeIdentity) -> Self {
        self.identity = Some(value);
        self.device = None;
        self
    }

    #[must_use]
    pub fn auth(mut self, value: ConnectAuth) -> Self {
        self.auth = Some(value);
        self
    }

    #[must_use]
    pub fn locale(mut self, value: impl Into<String>) -> Self {
        self.locale = Some(value.into());
        self
    }

    #[must_use]
    pub fn user_agent(mut self, value: impl Into<String>) -> Self {
        self.user_agent = Some(value.into());
        self
    }

    fn finalize_identity(mut self, nonce: &str) -> Result<Self, IdentityError> {
        if self.activated {
            self.advertised_caps.clone_from(&self.declared_caps);
            self.advertised_commands.clone_from(&self.declared_commands);
            self.advertised_permissions
                .clone_from(&self.declared_permissions);
        }
        let Some(identity) = self.identity.take() else {
            return Ok(self);
        };
        self.device = Some(identity.sign_connect(
            nonce,
            &self.client.platform,
            self.client.device_family.as_deref(),
            self.auth.as_ref().and_then(ConnectAuth::signature_token),
        )?);
        Ok(self)
    }
}

impl ConnectAuth {
    fn signature_token(&self) -> Option<&str> {
        self.token
            .as_deref()
            .or(self.device_token.as_deref())
            .or(self.bootstrap_token.as_deref())
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Event {
    pub event: String,
    #[serde(default)]
    pub payload: Value,
    #[serde(default)]
    pub seq: Option<u64>,
}

/// A Gateway-authorized node command invocation.
#[derive(Clone, Debug, PartialEq)]
pub struct NodeInvocation {
    pub id: String,
    pub node_id: String,
    pub command: String,
    pub params: Value,
    pub timeout_ms: Option<u64>,
    pub idempotency_key: Option<String>,
}

/// A structured final result for a node invocation.
#[derive(Clone, Debug, PartialEq)]
pub enum InvocationResult {
    Success(Value),
    Failure { code: String, message: String },
}

impl InvocationResult {
    #[must_use]
    pub fn success(payload: Value) -> Self {
        Self::Success(payload)
    }

    #[must_use]
    pub fn failure(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Failure {
            code: code.into(),
            message: message.into(),
        }
    }
}

#[derive(Debug, Error)]
pub enum ClientError {
    #[error("invalid Gateway URL: {0}")]
    InvalidUrl(String),
    #[error("plaintext WebSocket is allowed only for loopback Gateways")]
    InsecureRemoteGateway,
    #[error("Gateway connection failed: {0}")]
    Transport(String),
    #[error("Gateway connect challenge timed out")]
    ChallengeTimeout,
    #[error("Gateway connect challenge was invalid: {0}")]
    InvalidChallenge(String),
    #[error("connect parameter callback failed: {0}")]
    ConnectParams(String),
    #[error("node identity failed: {0}")]
    Identity(#[from] IdentityError),
    #[error("Gateway rejected {method}: {code}: {message}")]
    Gateway {
        method: String,
        code: String,
        message: String,
        details: Option<Value>,
        retryable: Option<bool>,
        retry_after_ms: Option<u64>,
    },
    #[error("Gateway request timed out: {0}")]
    RequestTimeout(String),
    #[error("Gateway session is closed: {0}")]
    Closed(String),
    #[error("Gateway frame was invalid: {0}")]
    InvalidFrame(String),
    #[error("event consumer fell behind by {0} events")]
    EventLagged(u64),
    #[error("the embedding did not activate this node connection")]
    NotActivated,
}

pub struct NodeClient;

impl NodeClient {
    /// Connect as a node after the Gateway supplies its challenge nonce.
    ///
    /// The callback owns identity loading and challenge-bound signing. This
    /// keeps key storage platform-neutral while the library owns protocol
    /// ordering and node defaults.
    /// # Errors
    ///
    /// Returns transport, challenge, callback, or Gateway rejection errors.
    pub async fn connect<F, Fut, E>(
        config: NodeClientConfig,
        make_options: F,
    ) -> Result<NodeSession, ClientError>
    where
        F: FnOnce(String) -> Fut,
        Fut: Future<Output = Result<NodeConnectOptions, E>>,
        E: Error + Send + Sync + 'static,
    {
        validate_gateway_url(&config.gateway_url)?;
        let websocket_config = WebSocketConfig::default()
            .max_message_size(Some(config.max_message_bytes))
            .max_frame_size(Some(config.max_message_bytes));
        let (mut socket, _) =
            connect_async_with_config(config.gateway_url.as_str(), Some(websocket_config), false)
                .await
                .map_err(|error| ClientError::Transport(error.to_string()))?;

        let nonce = tokio::time::timeout(config.challenge_timeout, wait_for_challenge(&mut socket))
            .await
            .map_err(|_| ClientError::ChallengeTimeout)??;
        let options = make_options(nonce.clone())
            .await
            .map_err(|error| ClientError::ConnectParams(error.to_string()))?;
        let options = options.finalize_identity(&nonce)?;
        let activated = options.activated;

        let connect_id = "rust-node-connect-1";
        send_request(&mut socket, connect_id, "connect", json!(options)).await?;
        let hello = tokio::time::timeout(
            config.challenge_timeout,
            wait_for_response(&mut socket, connect_id, "connect"),
        )
        .await
        .map_err(|_| ClientError::RequestTimeout("connect".into()))??;

        let (command_tx, command_rx) = mpsc::channel(config.max_in_flight.max(1));
        let (cancel_tx, cancel_rx) = mpsc::unbounded_channel();
        let (event_tx, initial_event_rx) = broadcast::channel(config.event_capacity.max(1));
        let (invocation_tx, initial_invocation_rx) =
            broadcast::channel(config.event_capacity.max(1));
        let (closed_tx, closed_rx) = watch::channel(None);
        tokio::spawn(run_session(
            socket,
            command_rx,
            cancel_rx,
            event_tx.clone(),
            invocation_tx,
            closed_tx,
        ));

        Ok(NodeSession {
            hello,
            activated,
            command_tx,
            cancel_tx,
            event_tx,
            event_rx: Arc::new(Mutex::new(initial_event_rx)),
            invocation_rx: Arc::new(Mutex::new(initial_invocation_rx)),
            closed_rx,
            next_request_id: Arc::new(AtomicU64::new(1)),
            request_timeout: config.request_timeout,
            in_flight: Arc::new(Semaphore::new(config.max_in_flight.max(1))),
        })
    }
}

#[derive(Clone)]
pub struct NodeSession {
    hello: Value,
    activated: bool,
    command_tx: mpsc::Sender<SessionCommand>,
    cancel_tx: mpsc::UnboundedSender<String>,
    event_tx: broadcast::Sender<Event>,
    event_rx: Arc<Mutex<broadcast::Receiver<Event>>>,
    invocation_rx: Arc<Mutex<broadcast::Receiver<Event>>>,
    closed_rx: watch::Receiver<Option<String>>,
    next_request_id: Arc<AtomicU64>,
    request_timeout: Duration,
    in_flight: Arc<Semaphore>,
}

impl NodeSession {
    #[must_use]
    pub fn hello(&self) -> &Value {
        &self.hello
    }

    /// Whether the embedding activated its declared surface for this connection.
    ///
    /// This is local readiness only. `OpenClaw` may still narrow the effective
    /// surface while node capability approval is pending.
    #[must_use]
    pub fn is_activated(&self) -> bool {
        self.activated
    }

    #[must_use]
    /// Subscribe to events published after this call.
    ///
    /// Use [`Self::next_event`] for the retained primary stream.
    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.event_tx.subscribe()
    }

    /// Receive the next event from the session's retained primary stream.
    ///
    /// Unlike [`Self::subscribe`], this stream begins at connect time, so an
    /// event sent immediately after the hello response is not lost.
    /// # Errors
    ///
    /// Returns a lag or closed-session error.
    pub async fn next_event(&self) -> Result<Event, ClientError> {
        match self.event_rx.lock().await.recv().await {
            Ok(event) => Ok(event),
            Err(broadcast::error::RecvError::Lagged(count)) => Err(ClientError::EventLagged(count)),
            Err(broadcast::error::RecvError::Closed) => Err(self.closed_error()),
        }
    }

    /// Receive the next Gateway-authorized invocation for this connection.
    ///
    /// Invocation events remain available from the ordinary event stream too.
    /// The method refuses work when this connection was not locally activated.
    /// # Errors
    ///
    /// Returns an activation, lag, closed-session, or malformed-payload error.
    pub async fn next_invocation(&self) -> Result<NodeInvocation, ClientError> {
        if !self.activated {
            return Err(ClientError::NotActivated);
        }
        let event = match self.invocation_rx.lock().await.recv().await {
            Ok(event) => event,
            Err(broadcast::error::RecvError::Lagged(count)) => {
                return Err(ClientError::EventLagged(count));
            }
            Err(broadcast::error::RecvError::Closed) => return Err(self.closed_error()),
        };
        parse_invocation(event.payload)
    }

    /// Complete one invocation using `OpenClaw`'s published result contract.
    /// # Errors
    ///
    /// Returns validation, Gateway, timeout, or closed-session errors.
    pub async fn complete_invocation(
        &self,
        invocation: &NodeInvocation,
        result: InvocationResult,
    ) -> Result<(), ClientError> {
        if !self.activated {
            return Err(ClientError::NotActivated);
        }
        let params = match result {
            InvocationResult::Success(payload) => json!({
                "id": invocation.id,
                "nodeId": invocation.node_id,
                "ok": true,
                "payload": payload,
            }),
            InvocationResult::Failure { code, message } => {
                let code = require_non_empty_result_field("error code", code)?;
                let message = require_non_empty_result_field("error message", message)?;
                json!({
                    "id": invocation.id,
                    "nodeId": invocation.node_id,
                    "ok": false,
                    "error": { "code": code, "message": message },
                })
            }
        };
        self.request("node.invoke.result", params).await.map(|_| ())
    }

    /// Send one Gateway request and wait for its correlated response.
    /// # Errors
    ///
    /// Returns validation, Gateway, timeout, or closed-session errors.
    pub async fn request(
        &self,
        method: impl Into<String>,
        params: Value,
    ) -> Result<Value, ClientError> {
        let method = method.into();
        if method.is_empty() {
            return Err(ClientError::InvalidFrame(
                "request method must not be empty".into(),
            ));
        }
        let _permit = self
            .in_flight
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| self.closed_error())?;
        let id = format!(
            "rust-node-{}",
            self.next_request_id.fetch_add(1, Ordering::Relaxed)
        );
        let (reply_tx, reply_rx) = oneshot::channel();
        self.command_tx
            .send(SessionCommand::Request {
                id: id.clone(),
                method: method.clone(),
                params,
                reply: reply_tx,
            })
            .await
            .map_err(|_| self.closed_error())?;

        match tokio::time::timeout(self.request_timeout, reply_rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(self.closed_error()),
            Err(_) => {
                let _ = self.cancel_tx.send(id);
                Err(ClientError::RequestTimeout(method))
            }
        }
    }

    pub async fn close(&self) {
        let _ = self.command_tx.send(SessionCommand::Close).await;
    }

    /// Wait for the transport to close.
    /// # Errors
    ///
    /// Returns the transport's close reason.
    pub async fn wait_closed(&self) -> Result<(), ClientError> {
        let mut closed = self.closed_rx.clone();
        loop {
            if let Some(reason) = closed.borrow().clone() {
                return Err(ClientError::Closed(reason));
            }
            if closed.changed().await.is_err() {
                return Err(ClientError::Closed("session task ended".into()));
            }
        }
    }

    fn closed_error(&self) -> ClientError {
        ClientError::Closed(
            self.closed_rx
                .borrow()
                .clone()
                .unwrap_or_else(|| "session task ended".into()),
        )
    }
}

enum SessionCommand {
    Request {
        id: String,
        method: String,
        params: Value,
        reply: oneshot::Sender<Result<Value, ClientError>>,
    },
    Close,
}

#[derive(Deserialize)]
#[serde(tag = "type")]
enum IncomingFrame {
    #[serde(rename = "event")]
    Event {
        event: String,
        #[serde(default)]
        payload: Value,
        #[serde(default)]
        seq: Option<u64>,
    },
    #[serde(rename = "res")]
    Response {
        id: String,
        ok: bool,
        #[serde(default)]
        payload: Value,
        #[serde(default)]
        error: Option<GatewayErrorShape>,
    },
}

#[derive(Deserialize)]
struct GatewayErrorShape {
    #[serde(default)]
    code: String,
    #[serde(default)]
    message: String,
    #[serde(default)]
    details: Option<Value>,
    #[serde(default)]
    retryable: Option<bool>,
    #[serde(default, rename = "retryAfterMs")]
    retry_after_ms: Option<u64>,
}

async fn wait_for_challenge<S>(
    socket: &mut tokio_tungstenite::WebSocketStream<S>,
) -> Result<String, ClientError>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    loop {
        match next_frame(socket).await? {
            IncomingFrame::Event { event, payload, .. } if event == "connect.challenge" => {
                let nonce = payload
                    .get("nonce")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| ClientError::InvalidChallenge("missing nonce".into()))?;
                return Ok(nonce.into());
            }
            IncomingFrame::Event { .. } | IncomingFrame::Response { .. } => {}
        }
    }
}

async fn wait_for_response<S>(
    socket: &mut tokio_tungstenite::WebSocketStream<S>,
    expected_id: &str,
    method: &str,
) -> Result<Value, ClientError>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    loop {
        if let IncomingFrame::Response {
            id,
            ok,
            payload,
            error,
        } = next_frame(socket).await?
        {
            if id != expected_id {
                continue;
            }
            return response_result(method, ok, payload, error);
        }
    }
}

async fn next_frame<S>(
    socket: &mut tokio_tungstenite::WebSocketStream<S>,
) -> Result<IncomingFrame, ClientError>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    loop {
        let message = socket
            .next()
            .await
            .ok_or_else(|| ClientError::Closed("Gateway ended the WebSocket stream".into()))?
            .map_err(|error| ClientError::Transport(error.to_string()))?;
        match message {
            Message::Text(text) => {
                return serde_json::from_str(text.as_str())
                    .map_err(|error| ClientError::InvalidFrame(error.to_string()));
            }
            Message::Ping(payload) => socket
                .send(Message::Pong(payload))
                .await
                .map_err(|error| ClientError::Transport(error.to_string()))?,
            Message::Close(frame) => {
                return Err(ClientError::Closed(format_close(frame.as_ref())));
            }
            Message::Binary(_) | Message::Pong(_) | Message::Frame(_) => {}
        }
    }
}

async fn send_request<S>(
    socket: &mut tokio_tungstenite::WebSocketStream<S>,
    id: &str,
    method: &str,
    params: Value,
) -> Result<(), ClientError>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let frame = json!({ "type": "req", "id": id, "method": method, "params": params });
    socket
        .send(Message::Text(frame.to_string().into()))
        .await
        .map_err(|error| ClientError::Transport(error.to_string()))
}

async fn run_session<S>(
    mut socket: tokio_tungstenite::WebSocketStream<S>,
    mut commands: mpsc::Receiver<SessionCommand>,
    mut cancellations: mpsc::UnboundedReceiver<String>,
    events: broadcast::Sender<Event>,
    invocations: broadcast::Sender<Event>,
    closed: watch::Sender<Option<String>>,
) where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let mut pending = HashMap::new();
    let close_reason = loop {
        tokio::select! {
            Some(id) = cancellations.recv() => {
                pending.remove(&id);
            }
            command = commands.recv() => {
                match command {
                    Some(SessionCommand::Request { id, method, params, reply }) => {
                        match send_request(&mut socket, &id, &method, params).await {
                            Ok(()) => { pending.insert(id, (method, reply)); }
                            Err(error) => { let _ = reply.send(Err(error)); }
                        }
                    }
                    Some(SessionCommand::Close) | None => {
                        let _ = socket.close(None).await;
                        break "closed by client".to_owned();
                    }
                }
            }
            message = socket.next() => {
                match message {
                    Some(Ok(Message::Text(text))) => {
                        match serde_json::from_str::<IncomingFrame>(text.as_str()) {
                            Ok(IncomingFrame::Event { event, payload, seq }) => {
                                let received = Event { event, payload, seq };
                                if received.event == "node.invoke.request" {
                                    let _ = invocations.send(received.clone());
                                }
                                let _ = events.send(received);
                            }
                            Ok(IncomingFrame::Response { id, ok, payload, error }) => {
                                if let Some((method, reply)) = pending.remove(&id) {
                                    let _ = reply.send(response_result(&method, ok, payload, error));
                                }
                            }
                            Err(error) => break format!("invalid Gateway frame: {error}"),
                        }
                    }
                    Some(Ok(Message::Ping(payload))) => {
                        if socket.send(Message::Pong(payload)).await.is_err() {
                            break "failed to answer Gateway ping".into();
                        }
                    }
                    Some(Ok(Message::Close(frame))) => break format_close(frame.as_ref()),
                    Some(Ok(Message::Binary(_) | Message::Pong(_) | Message::Frame(_))) => {}
                    Some(Err(error)) => break error.to_string(),
                    None => break "Gateway ended the WebSocket stream".into(),
                }
            }
        }
    };

    for (_, (method, reply)) in pending {
        let _ = reply.send(Err(ClientError::Closed(format!(
            "{close_reason}; request {method} did not complete"
        ))));
    }
    let _ = closed.send(Some(close_reason));
}

fn parse_invocation(payload: Value) -> Result<NodeInvocation, ClientError> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    #[serde(rename_all = "camelCase")]
    struct Payload {
        id: String,
        node_id: String,
        command: String,
        #[serde(
            default,
            rename = "paramsJSON",
            deserialize_with = "deserialize_optional_string"
        )]
        params_json: Option<String>,
        #[serde(default, deserialize_with = "deserialize_optional_u64")]
        timeout_ms: Option<u64>,
        #[serde(default, deserialize_with = "deserialize_optional_string")]
        idempotency_key: Option<String>,
    }

    let payload: Payload = serde_json::from_value(payload).map_err(|error| {
        ClientError::InvalidFrame(format!("invalid node.invoke.request: {error}"))
    })?;
    let id = require_non_empty_result_field("invocation id", payload.id)?;
    let node_id = require_non_empty_result_field("invocation node id", payload.node_id)?;
    let command = require_non_empty_result_field("invocation command", payload.command)?;
    let idempotency_key = payload
        .idempotency_key
        .map(|value| require_non_empty_result_field("invocation idempotency key", value))
        .transpose()?;
    let params = match payload.params_json {
        Some(value) => serde_json::from_str(&value).map_err(|error| {
            ClientError::InvalidFrame(format!("invalid invocation paramsJSON: {error}"))
        })?,
        None => Value::Null,
    };
    Ok(NodeInvocation {
        id,
        node_id,
        command,
        params,
        timeout_ms: payload.timeout_ms,
        idempotency_key,
    })
}

fn require_non_empty_result_field(name: &str, value: String) -> Result<String, ClientError> {
    if value.is_empty() {
        return Err(ClientError::InvalidFrame(format!(
            "{name} must not be empty"
        )));
    }
    Ok(value)
}

fn deserialize_optional_string<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    String::deserialize(deserializer).map(Some)
}

fn deserialize_optional_u64<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
where
    D: Deserializer<'de>,
{
    u64::deserialize(deserializer).map(Some)
}

fn response_result(
    method: &str,
    ok: bool,
    payload: Value,
    error: Option<GatewayErrorShape>,
) -> Result<Value, ClientError> {
    if ok {
        return Ok(payload);
    }
    let mut error = error.unwrap_or(GatewayErrorShape {
        code: "UNKNOWN".into(),
        message: "Gateway rejected the request".into(),
        details: None,
        retryable: None,
        retry_after_ms: None,
    });
    if error.code.is_empty() {
        error.code = "UNKNOWN".into();
    }
    if error.message.is_empty() {
        error.message = "Gateway rejected the request".into();
    }
    Err(ClientError::Gateway {
        method: method.into(),
        code: error.code,
        message: error.message,
        details: error.details,
        retryable: error.retryable,
        retry_after_ms: error.retry_after_ms,
    })
}

fn validate_gateway_url(value: &str) -> Result<(), ClientError> {
    let url = Url::parse(value).map_err(|error| ClientError::InvalidUrl(error.to_string()))?;
    match url.scheme() {
        "wss" => Ok(()),
        "ws" if is_loopback_host(&url) => Ok(()),
        "ws" => Err(ClientError::InsecureRemoteGateway),
        scheme => Err(ClientError::InvalidUrl(format!(
            "unsupported scheme {scheme}; expected ws or wss"
        ))),
    }
}

fn is_loopback_host(url: &Url) -> bool {
    match url.host_str() {
        Some("localhost") => true,
        Some(host) => host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback()),
        None => false,
    }
}

fn format_close(frame: Option<&tokio_tungstenite::tungstenite::protocol::CloseFrame>) -> String {
    frame.map_or_else(
        || "Gateway closed the WebSocket".into(),
        |frame| {
            format!(
                "Gateway closed the WebSocket ({}): {}",
                frame.code, frame.reason
            )
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invocation_payload_rejects_null_optional_fields_and_unknown_fields() {
        let required = json!({
            "id": "invoke-1",
            "nodeId": "node-1",
            "command": "example.status"
        });
        assert!(parse_invocation(required.clone()).is_ok());

        for field in ["paramsJSON", "timeoutMs", "idempotencyKey"] {
            let mut invalid = required.clone();
            invalid[field] = Value::Null;
            assert!(matches!(
                parse_invocation(invalid),
                Err(ClientError::InvalidFrame(_))
            ));
        }

        let mut unknown = required;
        unknown["extra"] = json!(true);
        assert!(matches!(
            parse_invocation(unknown),
            Err(ClientError::InvalidFrame(_))
        ));

        let empty_idempotency_key = json!({
            "id": "invoke-1",
            "nodeId": "node-1",
            "command": "example.status",
            "idempotencyKey": ""
        });
        assert!(matches!(
            parse_invocation(empty_idempotency_key),
            Err(ClientError::InvalidFrame(_))
        ));
    }
}
