#![expect(
    dead_code,
    reason = "fixture-only wire structs are decoded for shape validation"
)]

use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
enum Frame {
    #[serde(rename = "event")]
    Event {
        event: String,
        #[serde(default)]
        payload: Value,
        #[serde(default, deserialize_with = "deserialize_optional_non_null")]
        seq: Option<u64>,
        #[serde(
            rename = "stateVersion",
            default,
            deserialize_with = "deserialize_optional_non_null"
        )]
        state_version: Option<StateVersion>,
    },
    #[serde(rename = "req")]
    Request {
        id: String,
        method: String,
        #[serde(default)]
        params: Value,
    },
    #[serde(rename = "res")]
    Response {
        id: String,
        ok: bool,
        #[serde(default)]
        payload: Value,
        #[serde(default, deserialize_with = "deserialize_optional_non_null")]
        error: Option<ErrorShape>,
    },
}

impl Frame {
    fn validate(&self) -> Result<(), String> {
        match self {
            Self::Event { event, .. } => require_non_empty("event", event),
            Self::Request { id, method, .. } => {
                require_non_empty("id", id)?;
                require_non_empty("method", method)
            }
            Self::Response { id, error, .. } => {
                require_non_empty("id", id)?;
                if let Some(error) = error {
                    error.validate()?;
                }
                Ok(())
            }
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StateVersion {
    presence: u64,
    health: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ErrorShape {
    code: String,
    message: String,
    #[serde(default)]
    details: Value,
    #[serde(default, deserialize_with = "deserialize_optional_non_null")]
    retryable: Option<bool>,
    #[serde(default, deserialize_with = "deserialize_optional_non_null")]
    retry_after_ms: Option<u64>,
}

impl ShapeValidation for ErrorShape {
    fn validate(&self) -> Result<(), String> {
        require_non_empty("error.code", &self.code)?;
        require_non_empty("error.message", &self.message)
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DevicePairRequested {
    request_id: String,
    device_id: String,
    public_key: String,
    ts: u64,
    #[serde(default, deserialize_with = "deserialize_optional_non_null")]
    display_name: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_non_null")]
    platform: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_non_null")]
    device_family: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_non_null")]
    client_id: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_non_null")]
    client_mode: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_non_null")]
    browser_origin: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_non_null")]
    role: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_non_null")]
    roles: Option<Vec<String>>,
    #[serde(default, deserialize_with = "deserialize_optional_non_null")]
    scopes: Option<Vec<String>>,
    #[serde(default, deserialize_with = "deserialize_optional_non_null")]
    remote_ip: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_non_null")]
    silent: Option<bool>,
    #[serde(default, deserialize_with = "deserialize_optional_non_null")]
    is_repair: Option<bool>,
}

impl ShapeValidation for DevicePairRequested {
    fn validate(&self) -> Result<(), String> {
        require_non_empty("requestId", &self.request_id)?;
        require_non_empty("deviceId", &self.device_id)?;
        require_non_empty("publicKey", &self.public_key)?;
        for (field, value) in [
            ("displayName", self.display_name.as_deref()),
            ("platform", self.platform.as_deref()),
            ("deviceFamily", self.device_family.as_deref()),
            ("clientId", self.client_id.as_deref()),
            ("clientMode", self.client_mode.as_deref()),
            ("browserOrigin", self.browser_origin.as_deref()),
            ("role", self.role.as_deref()),
            ("remoteIp", self.remote_ip.as_deref()),
        ] {
            if let Some(value) = value {
                require_non_empty(field, value)?;
            }
        }
        validate_non_empty_items("roles", self.roles.as_deref())?;
        validate_non_empty_items("scopes", self.scopes.as_deref())
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct NodeInvokeRequest {
    id: String,
    node_id: String,
    command: String,
    #[serde(
        rename = "paramsJSON",
        default,
        deserialize_with = "deserialize_optional_non_null"
    )]
    params_json: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_non_null")]
    timeout_ms: Option<u64>,
    #[serde(default, deserialize_with = "deserialize_optional_non_null")]
    idempotency_key: Option<String>,
}

impl ShapeValidation for NodeInvokeRequest {
    fn validate(&self) -> Result<(), String> {
        require_non_empty("id", &self.id)?;
        require_non_empty("nodeId", &self.node_id)?;
        require_non_empty("command", &self.command)?;
        if let Some(idempotency_key) = &self.idempotency_key {
            require_non_empty("idempotencyKey", idempotency_key)?;
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct NodeInvokeInput {
    id: String,
    node_id: String,
    seq: u64,
    #[serde(rename = "payloadJSON")]
    payload_json: String,
}

impl ShapeValidation for NodeInvokeInput {
    fn validate(&self) -> Result<(), String> {
        require_non_empty("id", &self.id)?;
        require_non_empty("nodeId", &self.node_id)?;
        if self.payload_json.chars().count() > 16_384 {
            return Err("payloadJSON exceeds maxLength 16384".into());
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct NodeInvokeResult {
    id: String,
    node_id: String,
    ok: bool,
    #[serde(default)]
    payload: Value,
    #[serde(
        rename = "payloadJSON",
        default,
        deserialize_with = "deserialize_optional_non_null"
    )]
    payload_json: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_non_null")]
    error: Option<InvokeError>,
}

impl ShapeValidation for NodeInvokeResult {
    fn validate(&self) -> Result<(), String> {
        require_non_empty("id", &self.id)?;
        require_non_empty("nodeId", &self.node_id)?;
        if let Some(error) = &self.error {
            error.validate()?;
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct InvokeError {
    #[serde(default, deserialize_with = "deserialize_optional_non_null")]
    code: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_non_null")]
    message: Option<String>,
}

impl ShapeValidation for InvokeError {
    fn validate(&self) -> Result<(), String> {
        if let Some(code) = &self.code {
            require_non_empty("error.code", code)?;
        }
        if let Some(message) = &self.message {
            require_non_empty("error.message", message)?;
        }
        Ok(())
    }
}

trait ShapeValidation {
    fn validate(&self) -> Result<(), String>;
}

#[derive(Debug, PartialEq, Eq)]
pub enum FixtureError {
    InvalidFrame(String),
    WrongEvent { expected: String, actual: String },
    InvalidPayload(String),
    UnsupportedContract(String),
}

/// Strictly validate a named conformance fixture against the pinned projection.
///
/// This is a drift harness, not the future runtime decoder. Unknown fields are
/// rejected here because the pinned schema uses `additionalProperties: false`.
/// # Errors
///
/// Returns the frame, event-name, payload, or missing-contract failure.
pub fn validate_fixture(contract_id: &str, json: &str) -> Result<(), FixtureError> {
    let frame: Frame = serde_json::from_str(json)
        .map_err(|error| FixtureError::InvalidFrame(error.to_string()))?;
    frame.validate().map_err(FixtureError::InvalidFrame)?;

    match (contract_id, frame) {
        ("device.pair.requested", Frame::Event { event, payload, .. }) => {
            validate_event_name("device.pair.requested", &event)?;
            decode_payload::<DevicePairRequested>(payload)
        }
        ("node.invoke.request", Frame::Event { event, payload, .. }) => {
            validate_event_name("node.invoke.request", &event)?;
            decode_payload::<NodeInvokeRequest>(payload)
        }
        ("node.invoke.input", Frame::Event { event, payload, .. }) => {
            validate_event_name("node.invoke.input", &event)?;
            decode_payload::<NodeInvokeInput>(payload)
        }
        ("node.invoke.result", Frame::Request { method, params, .. }) => {
            if method != "node.invoke.result" {
                return Err(FixtureError::WrongEvent {
                    expected: "node.invoke.result".into(),
                    actual: method,
                });
            }
            decode_payload::<NodeInvokeResult>(params)
        }
        (
            "connect.challenge"
            | "node.pair.requested"
            | "node.pair.resolved"
            | "node.invoke.cancel"
            | "disconnect-cleanup"
            | "node.pending",
            _,
        ) => Err(FixtureError::UnsupportedContract(contract_id.into())),
        (_, Frame::Event { event, .. }) => Err(FixtureError::WrongEvent {
            expected: contract_id.into(),
            actual: event,
        }),
        (_, Frame::Request { method, .. }) => Err(FixtureError::WrongEvent {
            expected: contract_id.into(),
            actual: method,
        }),
        (_, Frame::Response { .. }) => Err(FixtureError::WrongEvent {
            expected: contract_id.into(),
            actual: "response".into(),
        }),
    }
}

fn validate_event_name(expected: &str, actual: &str) -> Result<(), FixtureError> {
    if actual == expected {
        Ok(())
    } else {
        Err(FixtureError::WrongEvent {
            expected: expected.into(),
            actual: actual.into(),
        })
    }
}

fn decode_payload<T: for<'de> Deserialize<'de> + ShapeValidation>(
    payload: Value,
) -> Result<(), FixtureError> {
    let decoded = serde_json::from_value::<T>(payload)
        .map_err(|error| FixtureError::InvalidPayload(error.to_string()))?;
    decoded.validate().map_err(FixtureError::InvalidPayload)
}

fn require_non_empty(field: &str, value: &str) -> Result<(), String> {
    if value.is_empty() {
        Err(format!("{field} must have minLength 1"))
    } else {
        Ok(())
    }
}

fn validate_non_empty_items(field: &str, values: Option<&[String]>) -> Result<(), String> {
    if values.is_some_and(|values| values.iter().any(String::is_empty)) {
        Err(format!("{field} items must have minLength 1"))
    } else {
        Ok(())
    }
}

fn deserialize_optional_non_null<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}
