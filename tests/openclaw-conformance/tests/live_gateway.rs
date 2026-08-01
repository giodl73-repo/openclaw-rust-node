use openclaw_node_host::{
    CommandRuntime, ConnectAuth, HandlerError, NodeClient, NodeClientConfig, NodeConnectOptions,
    NodeIdentity, ReconnectAction, ReconnectPause, ReconnectPolicy,
};
use serde_json::{json, Value};
use std::{
    convert::Infallible,
    process::{Command, Output},
};

const STATUS_COMMAND: &str = "rust-node.conformance.status";
const FAILURE_COMMAND: &str = "rust-node.conformance.fail";

/// Black-box conformance against an isolated real `OpenClaw` Gateway.
///
/// The caller owns Gateway startup and supplies only process-local test
/// credentials. See `tests/cross-repo/README.md` at the repository root.
#[tokio::test]
#[ignore = "requires an isolated real OpenClaw Gateway and CLI"]
async fn real_gateway_pairing_approval_invocation_and_reconnect() {
    let environment = LiveEnvironment::load();
    let identity = NodeIdentity::generate().expect("generate ephemeral node identity");
    let device_id = identity.device_id();
    let runtime = live_runtime();

    let first = connect(
        &environment.gateway_url,
        ConnectAuth::token(environment.gateway_token.clone()),
        identity.clone(),
        &runtime,
    )
    .await
    .expect("connect with isolated Gateway credential");
    assert_hello(&first);
    let issued_device_token = first
        .issued_device_token()
        .expect("Gateway should issue a device-bound reconnect token")
        .to_owned();

    let cli = LiveCli::new(&environment, device_id.clone());
    let approval_path = cli.approve_command_surface_if_pending();
    first.close().await;
    let _ = first.wait_closed().await;

    // Pairing changes the Gateway-owned approved surface. A fresh connection
    // binds invocation to that approved generation; the pre-approval session
    // must not be reused for work.
    let reconnected = connect(
        &environment.gateway_url,
        ConnectAuth::device_token(issued_device_token),
        identity.clone(),
        &runtime,
    )
    .await
    .expect("reconnect with issued device token");
    assert_hello(&reconnected);
    let reconnect_runtime = tokio::spawn({
        let runtime = runtime.clone();
        let session = reconnected.clone();
        async move { runtime.run(session).await }
    });
    cli.assert_success_and_structured_failure().await;
    reconnected.close().await;
    let _ = reconnected.wait_closed().await;
    assert!(
        reconnect_runtime
            .await
            .expect("join reconnect runtime")
            .is_err(),
        "closed reconnect session should terminate the command runtime"
    );

    let rejected = connect(
        &environment.gateway_url,
        ConnectAuth::device_token("invalid-device-token"),
        identity,
        &runtime,
    )
    .await;
    let rejected = match rejected {
        Ok(session) => {
            session.close().await;
            panic!("Gateway should reject an invalid issued-device token");
        }
        Err(error) => error,
    };
    assert!(matches!(
        ReconnectPolicy::default().after_failure(&rejected),
        ReconnectAction::Pause(ReconnectPause::Authentication { detail_code })
            if detail_code == "AUTH_DEVICE_TOKEN_MISMATCH"
    ));

    println!(
        "real-gateway pairing=true capability_approval={} success=true structured_failure=true device_token_reconnect=true invalid_token_paused=true device={}",
        approval_path.as_str(),
        &device_id[..12]
    );
}

async fn connect(
    url: &str,
    auth: ConnectAuth,
    identity: NodeIdentity,
    runtime: &CommandRuntime,
) -> Result<openclaw_node_host::NodeSession, openclaw_node_host::ClientError> {
    let connect_runtime = runtime.clone();
    NodeClient::connect(NodeClientConfig::new(url), move |_nonce| async move {
        Ok::<_, Infallible>(
            connect_runtime.activate(
                NodeConnectOptions::new(env!("CARGO_PKG_VERSION"), std::env::consts::OS)
                    .display_name("Rust node conformance")
                    .auth(auth)
                    .identity(identity),
            ),
        )
    })
    .await
}

fn live_runtime() -> CommandRuntime {
    CommandRuntime::builder()
        .max_concurrency(2)
        .command(STATUS_COMMAND, |_context| async {
            Ok(json!({"ready": true}))
        })
        .command(FAILURE_COMMAND, |_context| async {
            Err(HandlerError::new(
                "CONFORMANCE_NOT_READY",
                "conformance failure path",
            ))
        })
        .build()
        .expect("build bounded conformance runtime")
}

fn assert_hello(session: &openclaw_node_host::NodeSession) {
    assert_eq!(session.hello()["type"], "hello-ok");
    assert!(
        session.hello()["protocol"]
            .as_u64()
            .is_some_and(|protocol| protocol >= 3),
        "Gateway should negotiate a supported node protocol"
    );
    assert!(session.is_activated());
}

struct LiveEnvironment {
    gateway_url: String,
    gateway_token: String,
    cli: String,
    node_executable: String,
}

impl LiveEnvironment {
    fn load() -> Self {
        Self {
            gateway_url: required("OPENCLAW_GATEWAY_URL"),
            gateway_token: required("OPENCLAW_GATEWAY_TOKEN"),
            cli: required("OPENCLAW_CLI"),
            node_executable: std::env::var("OPENCLAW_NODE_EXE").unwrap_or_else(|_| "node".into()),
        }
    }
}

#[derive(Clone)]
struct LiveCli {
    node_executable: String,
    cli: String,
    gateway_url: String,
    gateway_token: String,
    device_id: String,
}

enum ApprovalPath {
    Explicit,
    TrustedAutomatic,
}

impl ApprovalPath {
    const fn as_str(&self) -> &'static str {
        match self {
            Self::Explicit => "explicit",
            Self::TrustedAutomatic => "trusted-automatic",
        }
    }
}

impl LiveCli {
    fn new(environment: &LiveEnvironment, device_id: String) -> Self {
        Self {
            node_executable: environment.node_executable.clone(),
            cli: environment.cli.clone(),
            gateway_url: environment.gateway_url.clone(),
            gateway_token: environment.gateway_token.clone(),
            device_id,
        }
    }

    fn approve_command_surface_if_pending(&self) -> ApprovalPath {
        let pending = self.run(&["nodes", "pending", "--json"]);
        let pending = output_json(&pending, "list pending node command approvals");
        let request_id = pending
            .as_array()
            .and_then(|requests| {
                requests.iter().find(|request| {
                    request["nodeId"] == self.device_id
                        && request["commands"].as_array().is_some_and(|commands| {
                            commands.iter().any(|command| command == STATUS_COMMAND)
                                && commands.iter().any(|command| command == FAILURE_COMMAND)
                        })
                })
            })
            .and_then(|request| request["requestId"].as_str());
        let Some(request_id) = request_id else {
            // Current OpenClaw may approve the first capability surface in the
            // same trusted node-pairing step. The subsequent invocation is the
            // required proof that this was approval rather than missing state.
            return ApprovalPath::TrustedAutomatic;
        };
        output_json(
            &self.run(&["nodes", "approve", request_id, "--json"]),
            "approve node command surface",
        );
        ApprovalPath::Explicit
    }

    async fn assert_success_and_structured_failure(&self) {
        self.assert_status().await;
        let failure = self.invoke(FAILURE_COMMAND, "{}", 5_000).await;
        assert_cli_failure(&failure, "CONFORMANCE_NOT_READY");
    }

    async fn assert_status(&self) {
        let status = self
            .invoke(STATUS_COMMAND, r#"{"proof":true}"#, 5_000)
            .await;
        let status = output_json(&status, "invoke approved Rust status command");
        assert_eq!(status["payload"]["ready"], true);
    }

    async fn invoke(&self, command: &str, params: &str, timeout_ms: u64) -> Output {
        let client = self.clone();
        let command = command.to_owned();
        let params = params.to_owned();
        tokio::task::spawn_blocking(move || client.invoke_sync(&command, &params, timeout_ms))
            .await
            .expect("join OpenClaw CLI invocation")
    }

    fn invoke_sync(&self, command: &str, params: &str, timeout_ms: u64) -> Output {
        self.run(&[
            "nodes",
            "invoke",
            "--node",
            &self.device_id,
            "--command",
            command,
            "--params",
            params,
            "--invoke-timeout",
            &timeout_ms.to_string(),
            "--json",
        ])
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(&self.node_executable)
            .arg(&self.cli)
            .args(args)
            .args(["--url", &self.gateway_url, "--token", &self.gateway_token])
            .output()
            .expect("OpenClaw CLI should start")
    }
}

fn assert_cli_failure(output: &Output, expected_code: &str) {
    assert!(
        !output.status.success(),
        "{expected_code} unexpectedly succeeded"
    );
    let combined = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        combined.contains(expected_code),
        "missing {expected_code}: {combined}"
    );
}

fn output_json(output: &Output, action: &str) -> Value {
    assert!(
        output.status.success(),
        "failed to {action}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "failed to parse {action} output as JSON: {error}: {}",
            String::from_utf8_lossy(&output.stdout)
        )
    })
}

fn required(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} is required"))
}
