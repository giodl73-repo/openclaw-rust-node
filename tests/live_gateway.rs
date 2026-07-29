use openclaw_node::{
    CommandRuntime, ConnectAuth, HandlerError, NodeClient, NodeClientConfig, NodeConnectOptions,
    NodeIdentity, ReconnectAction, ReconnectPause, ReconnectPolicy,
};
use serde_json::{Value, json};
use std::process::{Command, Output};
use std::{convert::Infallible, future::pending, sync::Arc, time::Duration};
use tokio::sync::Notify;

#[tokio::test]
#[ignore = "requires an isolated real OpenClaw Gateway"]
async fn connects_to_a_real_local_gateway() {
    let url = std::env::var("OPENCLAW_GATEWAY_URL").expect("OPENCLAW_GATEWAY_URL is required");
    let token =
        std::env::var("OPENCLAW_GATEWAY_TOKEN").expect("OPENCLAW_GATEWAY_TOKEN is required");
    let reconnect_url = url.clone();
    let rejected_url = url.clone();
    let identity = NodeIdentity::generate().unwrap();
    let device_id = identity.device_id();
    let first_identity = identity.clone();

    let session = NodeClient::connect(NodeClientConfig::new(url), move |_nonce| async move {
        Ok::<_, Infallible>(
            NodeConnectOptions::new(env!("CARGO_PKG_VERSION"), std::env::consts::OS)
                .display_name("Rust live proof node")
                .auth(ConnectAuth::token(token))
                .identity(first_identity),
        )
    })
    .await
    .unwrap();

    assert_eq!(session.hello()["type"], "hello-ok");
    assert!(
        session.hello()["protocol"]
            .as_u64()
            .is_some_and(|value| value >= 3)
    );
    let device_token = session.hello()["auth"]["deviceToken"]
        .as_str()
        .filter(|value| !value.is_empty())
        .expect("Gateway should issue a device token")
        .to_owned();
    session.close().await;
    let _ = session.wait_closed().await;

    let rejected_identity = identity.clone();
    let reconnect = NodeClient::connect(
        NodeClientConfig::new(reconnect_url),
        move |_nonce| async move {
            Ok::<_, Infallible>(
                NodeConnectOptions::new(env!("CARGO_PKG_VERSION"), std::env::consts::OS)
                    .display_name("Rust live proof node")
                    .auth(ConnectAuth::device_token(device_token))
                    .identity(identity),
            )
        },
    )
    .await
    .unwrap();
    assert_eq!(reconnect.hello()["type"], "hello-ok");
    reconnect.close().await;
    let _ = reconnect.wait_closed().await;

    let rejected = NodeClient::connect(
        NodeClientConfig::new(rejected_url),
        move |_nonce| async move {
            Ok::<_, Infallible>(
                NodeConnectOptions::new(env!("CARGO_PKG_VERSION"), std::env::consts::OS)
                    .display_name("Rust live proof node")
                    .auth(ConnectAuth::device_token("invalid-device-token"))
                    .identity(rejected_identity),
            )
        },
    )
    .await;
    let Err(rejected) = rejected else {
        panic!("Gateway should reject an invalid issued-device token");
    };
    let mut reconnect_policy = ReconnectPolicy::default();
    assert!(matches!(
        reconnect_policy.after_failure(&rejected),
        ReconnectAction::Pause(ReconnectPause::Authentication { detail_code })
            if detail_code == "AUTH_DEVICE_TOKEN_MISMATCH"
    ));
    println!(
        "real-gateway connected protocol={} server={} device={} device_token_issued=true device_token_reconnect=true rejected_device_token_paused=true",
        reconnect.hello()["protocol"],
        reconnect.hello()["server"]["version"],
        &device_id[..12]
    );
}

#[tokio::test]
#[ignore = "requires an isolated real OpenClaw Gateway and CLI"]
async fn proves_gateway_capability_approval_and_real_invocation() {
    let url = std::env::var("OPENCLAW_GATEWAY_URL").expect("OPENCLAW_GATEWAY_URL is required");
    let token =
        std::env::var("OPENCLAW_GATEWAY_TOKEN").expect("OPENCLAW_GATEWAY_TOKEN is required");
    let cli = std::env::var("OPENCLAW_CLI").expect("OPENCLAW_CLI is required");
    let node_exe = std::env::var("OPENCLAW_NODE_EXE").unwrap_or_else(|_| "node".into());
    let identity = NodeIdentity::generate().unwrap();
    let device_id = identity.device_id();
    let connect_token = token.clone();
    let live_cli = LiveCli {
        node_exe,
        cli,
        url: url.clone(),
        token,
        device_id: device_id.clone(),
    };
    let signals = LiveSignals::default();
    let runtime = live_runtime(&signals);
    let connect_runtime = runtime.clone();

    let session = NodeClient::connect(
        NodeClientConfig::new(url.clone()),
        move |_nonce| async move {
            Ok::<_, Infallible>(
                connect_runtime.activate(
                    NodeConnectOptions::new(env!("CARGO_PKG_VERSION"), std::env::consts::OS)
                        .display_name("Rust capability proof node")
                        .auth(ConnectAuth::token(connect_token))
                        .identity(identity),
                ),
            )
        },
    )
    .await
    .unwrap();

    approve_runtime_commands(&live_cli);
    let running_runtime = runtime.clone();
    let running_session = session.clone();
    let runtime_task = tokio::spawn(async move { running_runtime.run(running_session).await });

    let status = live_cli
        .spawn("example.status", "{\"proof\":true}", 5_000)
        .await
        .unwrap();
    let invoked = cli_json(&status, "invoke approved Rust command");
    assert_eq!(invoked["payload"]["ready"], true);
    let failure = live_cli.spawn("example.fail", "{}", 5_000).await.unwrap();
    assert_cli_failure(&failure, "NOT_READY");
    let timeout = live_cli.spawn("example.wait", "{}", 1_000).await.unwrap();
    assert_cli_failure(&timeout, "HANDLER_TIMEOUT");
    prove_live_saturation(&live_cli, &signals).await;
    let disconnected = live_cli.spawn("example.disconnect", "{}", 10_000);
    tokio::time::timeout(
        Duration::from_secs(20),
        signals.disconnect_started.notified(),
    )
    .await
    .expect("disconnect handler should start");
    session.close().await;
    let _ = session.wait_closed().await;
    assert!(runtime_task.await.unwrap().is_err());
    tokio::time::timeout(
        Duration::from_secs(2),
        signals.disconnect_cancelled.notified(),
    )
    .await
    .expect("disconnect should cancel handler child work");
    assert!(!disconnected.await.unwrap().status.success());
    println!(
        "real-gateway capability_approval=true preapproval_filtered=true runtime_success=true runtime_failure=true runtime_timeout=true runtime_saturation=true runtime_disconnect_cleanup=true device={}",
        &device_id[..12]
    );
}

#[derive(Clone, Default)]
struct LiveSignals {
    block_started: Arc<Notify>,
    block_release: Arc<Notify>,
    disconnect_started: Arc<Notify>,
    disconnect_cancelled: Arc<Notify>,
}

fn live_runtime(signals: &LiveSignals) -> CommandRuntime {
    let block = signals.clone();
    let disconnect = signals.clone();
    CommandRuntime::builder()
        .max_concurrency(1)
        .command("example.status", |_context| async { Ok(json!({"ready": true})) })
        .command("example.fail", |_context| async {
            Err(HandlerError::new("NOT_READY", "example is not ready"))
        })
        .command("example.wait", |_context| pending())
        .command("example.block", move |context| {
            let signals = block.clone();
            async move {
                signals.block_started.notify_one();
                tokio::select! {
                    () = signals.block_release.notified() => Ok(json!({"released": true})),
                    () = context.cancellation.cancelled() => Err(HandlerError::new("CANCELLED", "block cancelled")),
                }
            }
        })
        .command("example.disconnect", move |context| {
            let signals = disconnect.clone();
            async move {
                signals.disconnect_started.notify_one();
                let cancelled = signals.disconnect_cancelled.clone();
                tokio::spawn(async move {
                    context.cancellation.cancelled().await;
                    cancelled.notify_one();
                });
                pending().await
            }
        })
        .build()
        .unwrap()
}

async fn prove_live_saturation(live_cli: &LiveCli, signals: &LiveSignals) {
    let first = live_cli.spawn("example.block", "{}", 20_000);
    tokio::time::timeout(Duration::from_secs(20), signals.block_started.notified())
        .await
        .expect("first blocking handler should start");
    let overloaded = live_cli.spawn("example.block", "{}", 5_000).await.unwrap();
    assert_cli_failure(&overloaded, "OVERLOADED");
    signals.block_release.notify_one();
    let first = cli_json(&first.await.unwrap(), "release first saturated invocation");
    assert_eq!(first["payload"]["released"], true);
}

fn approve_runtime_commands(live_cli: &LiveCli) {
    let rejected = run_cli(
        &live_cli.node_exe,
        &live_cli.cli,
        &live_cli.url,
        &live_cli.token,
        &[
            "nodes",
            "invoke",
            "--node",
            &live_cli.device_id,
            "--command",
            "example.status",
            "--json",
        ],
    );
    assert!(
        !rejected.status.success(),
        "unapproved command unexpectedly ran"
    );

    let pending_output = run_cli(
        &live_cli.node_exe,
        &live_cli.cli,
        &live_cli.url,
        &live_cli.token,
        &["nodes", "pending", "--json"],
    );
    let pending = cli_json(&pending_output, "list pending node approvals");
    let request_id = pending
        .as_array()
        .and_then(|requests| {
            requests.iter().find(|request| {
                request["nodeId"] == live_cli.device_id
                    && request["commands"].as_array().is_some_and(|commands| {
                        commands.iter().any(|command| command == "example.status")
                    })
            })
        })
        .and_then(|request| request["requestId"].as_str())
        .expect("activated command should create a distinct pending node approval");
    let approved = run_cli(
        &live_cli.node_exe,
        &live_cli.cli,
        &live_cli.url,
        &live_cli.token,
        &["nodes", "approve", request_id, "--json"],
    );
    cli_json(&approved, "approve node capability surface");
}

#[derive(Clone)]
struct LiveCli {
    node_exe: String,
    cli: String,
    url: String,
    token: String,
    device_id: String,
}

impl LiveCli {
    fn invoke_sync(&self, command: &str, params: &str, timeout_ms: u64) -> Output {
        run_cli(
            &self.node_exe,
            &self.cli,
            &self.url,
            &self.token,
            &[
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
            ],
        )
    }

    fn spawn(
        &self,
        command: &str,
        params: &str,
        timeout_ms: u64,
    ) -> tokio::task::JoinHandle<Output> {
        let live_cli = self.clone();
        let command = command.to_owned();
        let params = params.to_owned();
        tokio::task::spawn_blocking(move || live_cli.invoke_sync(&command, &params, timeout_ms))
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

fn run_cli(node_exe: &str, cli: &str, url: &str, token: &str, args: &[&str]) -> Output {
    Command::new(node_exe)
        .arg(cli)
        .args(args)
        .args(["--url", url, "--token", token])
        .output()
        .expect("OpenClaw CLI should start")
}

fn cli_json(output: &Output, action: &str) -> Value {
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
