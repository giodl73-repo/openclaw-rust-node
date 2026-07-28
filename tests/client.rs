use futures_util::{SinkExt, StreamExt};
use openclaw_node::{
    ClientError, ConnectAuth, Event, InvocationResult, NodeClient, NodeClientConfig,
    NodeConnectOptions, NodeIdentity,
};
use serde_json::{Value, json};
use std::{io, time::Duration};
use tokio::net::TcpListener;
use tokio_tungstenite::{accept_async, tungstenite::Message};

#[tokio::test]
async fn generic_client_connects_publishes_events_and_correlates_requests() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let mut socket = accept_async(tcp).await.unwrap();
        send_json(
            &mut socket,
            json!({"type":"event","event":"connect.challenge","payload":{"nonce":"nonce-1"}}),
        )
        .await;

        let connect = receive_json(&mut socket).await;
        assert_eq!(connect["method"], "connect");
        assert_eq!(connect["params"]["role"], "node");
        assert_eq!(connect["params"]["client"]["mode"], "node");
        assert_eq!(connect["params"]["client"]["id"], "node-host");
        assert_eq!(connect["params"]["commands"], json!(["example.status"]));
        assert_eq!(connect["params"]["device"]["nonce"], "nonce-1");
        assert_eq!(
            connect["params"]["device"]["id"].as_str().unwrap().len(),
            64
        );
        assert!(
            connect["params"]["device"]["signature"]
                .as_str()
                .is_some_and(|value| !value.is_empty())
        );
        send_json(
            &mut socket,
            json!({
                "type":"res",
                "id":connect["id"],
                "ok":true,
                "payload":{"type":"hello-ok","protocol":4,"server":{"version":"test"}}
            }),
        )
        .await;

        send_json(
            &mut socket,
            json!({
                "type":"event",
                "event":"node.test",
                "payload":{"ready":true},
                "seq":7
            }),
        )
        .await;

        let request = receive_json(&mut socket).await;
        assert_eq!(request["method"], "node.echo");
        send_json(
            &mut socket,
            json!({
                "type":"res",
                "id":request["id"],
                "ok":true,
                "payload":{"echo":request["params"]}
            }),
        )
        .await;
    });

    let session = NodeClient::connect(
        NodeClientConfig::new(format!("ws://{address}")),
        |nonce| async move {
            assert_eq!(nonce, "nonce-1");
            Ok::<_, io::Error>(
                NodeConnectOptions::new("0.0.0-test", "test")
                    .display_name("Reusable test node")
                    .command("example.status")
                    .activate()
                    .auth(ConnectAuth::token("test-token"))
                    .identity(NodeIdentity::from_secret_bytes([7; 32])),
            )
        },
    )
    .await
    .unwrap();
    assert_eq!(session.hello()["protocol"], 4);

    let response = session
        .request("node.echo", json!({"value":42}))
        .await
        .unwrap();
    assert_eq!(response, json!({"echo":{"value":42}}));
    assert_eq!(
        session.next_event().await.unwrap(),
        Event {
            event: "node.test".into(),
            payload: json!({"ready":true}),
            seq: Some(7),
        }
    );
    server.await.unwrap();
}

#[tokio::test]
async fn declarations_are_withheld_until_the_embedding_activates_them() {
    for (activate, expected_surface) in [(false, false), (true, true)] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.unwrap();
            let mut socket = accept_async(tcp).await.unwrap();
            send_json(
                &mut socket,
                json!({"type":"event","event":"connect.challenge","payload":{"nonce":"activation-nonce"}}),
            )
            .await;
            let connect = receive_json(&mut socket).await;
            assert_eq!(
                connect["params"].get("commands").is_some(),
                expected_surface
            );
            assert_eq!(connect["params"].get("caps").is_some(), expected_surface);
            assert_eq!(
                connect["params"].get("permissions").is_some(),
                expected_surface
            );
            send_json(
                &mut socket,
                json!({"type":"res","id":connect["id"],"ok":true,"payload":{"type":"hello-ok","protocol":4}}),
            )
            .await;
        });

        let session = NodeClient::connect(
            NodeClientConfig::new(format!("ws://{address}")),
            move |_nonce| async move {
                let options = NodeConnectOptions::new("test", "test")
                    .capability("example")
                    .command("example.status")
                    .permission("example.read", true);
                Ok::<_, io::Error>(if activate {
                    options.activate()
                } else {
                    options
                })
            },
        )
        .await
        .unwrap();
        assert_eq!(session.is_activated(), activate);
        server.await.unwrap();
    }
}

#[tokio::test]
async fn activated_sessions_receive_and_complete_typed_invocations() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let mut socket = accept_async(tcp).await.unwrap();
        send_json(
            &mut socket,
            json!({"type":"event","event":"connect.challenge","payload":{"nonce":"invoke-nonce"}}),
        )
        .await;
        let connect = receive_json(&mut socket).await;
        send_json(
            &mut socket,
            json!({"type":"res","id":connect["id"],"ok":true,"payload":{"type":"hello-ok","protocol":4}}),
        )
        .await;
        send_json(
            &mut socket,
            json!({
                "type":"event",
                "event":"node.invoke.request",
                "payload":{
                    "id":"invoke-1",
                    "nodeId":"node-1",
                    "command":"example.status",
                    "paramsJSON":"{\"verbose\":true}",
                    "timeoutMs":5000,
                    "idempotencyKey":"once-1"
                }
            }),
        )
        .await;
        let result = receive_json(&mut socket).await;
        assert_eq!(result["method"], "node.invoke.result");
        assert_eq!(result["params"]["id"], "invoke-1");
        assert_eq!(result["params"]["nodeId"], "node-1");
        assert_eq!(result["params"]["ok"], true);
        assert_eq!(result["params"]["payload"], json!({"ready":true}));
        send_json(
            &mut socket,
            json!({"type":"res","id":result["id"],"ok":true,"payload":{"accepted":true}}),
        )
        .await;
    });

    let session = NodeClient::connect(
        NodeClientConfig::new(format!("ws://{address}")),
        |_nonce| async {
            Ok::<_, io::Error>(
                NodeConnectOptions::new("test", "test")
                    .command("example.status")
                    .activate(),
            )
        },
    )
    .await
    .unwrap();
    let invocation = session.next_invocation().await.unwrap();
    assert_eq!(invocation.id, "invoke-1");
    assert_eq!(invocation.node_id, "node-1");
    assert_eq!(invocation.command, "example.status");
    assert_eq!(invocation.params, json!({"verbose":true}));
    assert_eq!(invocation.timeout_ms, Some(5000));
    assert_eq!(invocation.idempotency_key.as_deref(), Some("once-1"));
    session
        .complete_invocation(
            &invocation,
            InvocationResult::success(json!({"ready":true})),
        )
        .await
        .unwrap();
    server.await.unwrap();
}

#[tokio::test]
async fn inactive_sessions_refuse_invocation_dispatch() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let mut socket = accept_async(tcp).await.unwrap();
        send_json(
            &mut socket,
            json!({"type":"event","event":"connect.challenge","payload":{"nonce":"inactive-nonce"}}),
        )
        .await;
        let connect = receive_json(&mut socket).await;
        send_json(
            &mut socket,
            json!({"type":"res","id":connect["id"],"ok":true,"payload":{"type":"hello-ok","protocol":4}}),
        )
        .await;
    });
    let session = NodeClient::connect(
        NodeClientConfig::new(format!("ws://{address}")),
        |_nonce| async { Ok::<_, io::Error>(NodeConnectOptions::new("test", "test")) },
    )
    .await
    .unwrap();
    assert!(matches!(
        session.next_invocation().await,
        Err(ClientError::NotActivated)
    ));
    server.await.unwrap();
}

#[tokio::test]
async fn gateway_connect_rejection_is_structured() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let mut socket = accept_async(tcp).await.unwrap();
        send_json(
            &mut socket,
            json!({"type":"event","event":"connect.challenge","payload":{"nonce":"nonce-2"}}),
        )
        .await;
        let connect = receive_json(&mut socket).await;
        send_json(
            &mut socket,
            json!({
                "type":"res",
                "id":connect["id"],
                "ok":false,
                "error":{
                    "code":"NOT_PAIRED",
                    "message":"pairing required: device approval is required",
                    "details":{"code":"PAIRING_REQUIRED","reason":"not-paired","requestId":"pair-1"},
                    "retryable":false,
                    "retryAfterMs":1250
                }
            }),
        )
        .await;
    });

    let result = NodeClient::connect(
        NodeClientConfig::new(format!("ws://{address}")),
        |_nonce| async { Ok::<_, io::Error>(NodeConnectOptions::new("test", "test")) },
    )
    .await;
    let Err(error) = result else {
        panic!("Gateway rejection should fail connect");
    };
    assert!(matches!(
        error,
        ClientError::Gateway {
            code,
            details: Some(_),
            retryable: Some(false),
            retry_after_ms: Some(1250),
            ..
        } if code == "NOT_PAIRED"
    ));
    server.await.unwrap();
}

#[tokio::test]
async fn request_timeout_does_not_poison_the_session() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let mut socket = accept_async(tcp).await.unwrap();
        send_json(
            &mut socket,
            json!({"type":"event","event":"connect.challenge","payload":{"nonce":"nonce-3"}}),
        )
        .await;
        let connect = receive_json(&mut socket).await;
        send_json(
            &mut socket,
            json!({"type":"res","id":connect["id"],"ok":true,"payload":{"type":"hello-ok","protocol":4}}),
        )
        .await;
        let ignored = receive_json(&mut socket).await;
        assert_eq!(ignored["method"], "slow.method");
        let next = receive_json(&mut socket).await;
        assert_eq!(next["method"], "fast.method");
        send_json(
            &mut socket,
            json!({"type":"res","id":next["id"],"ok":true,"payload":{"ready":true}}),
        )
        .await;
    });

    let session = NodeClient::connect(
        NodeClientConfig::new(format!("ws://{address}")).request_timeout(Duration::from_millis(25)),
        |_nonce| async { Ok::<_, io::Error>(NodeConnectOptions::new("test", "test")) },
    )
    .await
    .unwrap();
    assert!(matches!(
        session.request("slow.method", json!({})).await,
        Err(ClientError::RequestTimeout(method)) if method == "slow.method"
    ));
    assert_eq!(
        session.request("fast.method", json!({})).await.unwrap(),
        json!({"ready":true})
    );
    server.await.unwrap();
}

#[tokio::test]
async fn plaintext_remote_gateways_are_rejected_before_dialing() {
    let result = NodeClient::connect(
        NodeClientConfig::new("ws://example.com:18789"),
        |_nonce| async { Ok::<_, io::Error>(NodeConnectOptions::new("test", "test")) },
    )
    .await;
    let Err(error) = result else {
        panic!("remote plaintext Gateway should be rejected");
    };
    assert!(matches!(error, ClientError::InsecureRemoteGateway));
}

async fn send_json<S>(socket: &mut tokio_tungstenite::WebSocketStream<S>, value: Value)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    socket
        .send(Message::Text(value.to_string().into()))
        .await
        .unwrap();
}

async fn receive_json<S>(socket: &mut tokio_tungstenite::WebSocketStream<S>) -> Value
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    loop {
        match socket.next().await.unwrap().unwrap() {
            Message::Text(text) => return serde_json::from_str(text.as_str()).unwrap(),
            Message::Ping(payload) => socket.send(Message::Pong(payload)).await.unwrap(),
            _ => {}
        }
    }
}
