use openclaw_node::{
    ConnectAuth, InvocationResult, NodeClient, NodeClientConfig, NodeConnectOptions, NodeIdentity,
    ReconnectAction, ReconnectPause, ReconnectPolicy,
};
use serde_json::{Value, json};
use std::convert::Infallible;
use std::process::{Command, Output};

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

    let session = NodeClient::connect(
        NodeClientConfig::new(url.clone()),
        move |_nonce| async move {
            Ok::<_, Infallible>(
                NodeConnectOptions::new(env!("CARGO_PKG_VERSION"), std::env::consts::OS)
                    .display_name("Rust capability proof node")
                    .command("example.status")
                    .activate()
                    .auth(ConnectAuth::token(connect_token))
                    .identity(identity),
            )
        },
    )
    .await
    .unwrap();

    approve_example_status(&node_exe, &cli, &url, &token, &device_id);

    let invoke_node = node_exe.clone();
    let invoke_cli = cli.clone();
    let invoke_url = url.clone();
    let invoke_token = token.clone();
    let invoke_device = device_id.clone();
    let invoke = std::thread::spawn(move || {
        run_cli(
            &invoke_node,
            &invoke_cli,
            &invoke_url,
            &invoke_token,
            &[
                "nodes",
                "invoke",
                "--node",
                &invoke_device,
                "--command",
                "example.status",
                "--params",
                "{\"proof\":true}",
                "--json",
            ],
        )
    });

    let invocation = session.next_invocation().await.unwrap();
    assert_eq!(invocation.command, "example.status");
    assert_eq!(invocation.params, json!({"proof":true}));
    session
        .complete_invocation(
            &invocation,
            InvocationResult::success(json!({"ready":true})),
        )
        .await
        .unwrap();
    let invoked_output = invoke.join().unwrap();
    let invoked = cli_json(&invoked_output, "invoke approved Rust command");
    assert_eq!(invoked["payload"]["ready"], true);
    session.close().await;
    let _ = session.wait_closed().await;
    println!(
        "real-gateway capability_approval=true preapproval_filtered=true activated=true invocation_completed=true device={}",
        &device_id[..12]
    );
}

fn approve_example_status(node_exe: &str, cli: &str, url: &str, token: &str, device_id: &str) {
    let rejected = run_cli(
        node_exe,
        cli,
        url,
        token,
        &[
            "nodes",
            "invoke",
            "--node",
            device_id,
            "--command",
            "example.status",
            "--json",
        ],
    );
    assert!(
        !rejected.status.success(),
        "unapproved command unexpectedly ran"
    );

    let pending_output = run_cli(node_exe, cli, url, token, &["nodes", "pending", "--json"]);
    let pending = cli_json(&pending_output, "list pending node approvals");
    let request_id = pending
        .as_array()
        .and_then(|requests| {
            requests.iter().find(|request| {
                request["nodeId"] == device_id
                    && request["commands"].as_array().is_some_and(|commands| {
                        commands.iter().any(|command| command == "example.status")
                    })
            })
        })
        .and_then(|request| request["requestId"].as_str())
        .expect("activated command should create a distinct pending node approval");
    let approved = run_cli(
        node_exe,
        cli,
        url,
        token,
        &["nodes", "approve", request_id, "--json"],
    );
    cli_json(&approved, "approve node capability surface");
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
