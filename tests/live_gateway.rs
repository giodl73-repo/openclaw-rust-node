use openclaw_node::{
    ConnectAuth, NodeClient, NodeClientConfig, NodeConnectOptions, NodeIdentity, ReconnectAction,
    ReconnectPause, ReconnectPolicy,
};
use std::convert::Infallible;

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
