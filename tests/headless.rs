use futures_util::{SinkExt, StreamExt};
use openclaw_node::{ConnectAuth, HostConfig, HostCredentials, NodeIdentity, run_host};
use serde_json::{Value, json};
use std::{path::Path, time::Duration};
use tempfile::TempDir;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::oneshot,
};
use tokio_tungstenite::{accept_async, tungstenite::Message};

#[tokio::test]
async fn headless_host_serves_status_readiness_and_graceful_shutdown() {
    let gateway = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let gateway_address = gateway.local_addr().unwrap();
    let health_address = unused_loopback_address().await;
    let temporary = TempDir::new().unwrap();
    let config_path = temporary.path().join("node.json");
    write_config(&config_path, gateway_address, health_address);
    let config = HostConfig::load(&config_path).unwrap();
    let credentials = HostCredentials::new(
        NodeIdentity::from_secret_bytes([7; 32]),
        ConnectAuth::token("test-token"),
    );
    let (result_tx, result_rx) = oneshot::channel();
    let server = tokio::spawn(run_gateway(gateway, result_tx));

    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let host = tokio::spawn(run_host(config, credentials, async move {
        let _ = shutdown_rx.await;
    }));
    tokio::time::timeout(Duration::from_secs(8), result_rx)
        .await
        .expect("status command should complete")
        .unwrap();

    let readiness = health_get(health_address, "/readyz").await;
    assert!(readiness.starts_with("HTTP/1.1 200 OK"), "{readiness}");
    assert!(readiness.ends_with(r#"{"ready":true}"#), "{readiness}");

    shutdown_tx.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(10), host)
        .await
        .expect("host should stop promptly")
        .unwrap()
        .unwrap();
    server.await.unwrap();
}

#[tokio::test]
async fn cancelling_host_releases_health_listener() {
    let gateway_address = unused_loopback_address().await;
    let health_address = unused_loopback_address().await;
    let temporary = TempDir::new().unwrap();
    let config_path = temporary.path().join("node.json");
    write_config(&config_path, gateway_address, health_address);
    let config = HostConfig::load(&config_path).unwrap();
    let credentials = HostCredentials::new(
        NodeIdentity::from_secret_bytes([8; 32]),
        ConnectAuth::token("test-token"),
    );
    let host = tokio::spawn(run_host(config, credentials, std::future::pending()));

    let health = health_get(health_address, "/healthz").await;
    assert!(health.starts_with("HTTP/1.1 200 OK"), "{health}");

    host.abort();
    assert!(host.await.unwrap_err().is_cancelled());
    let rebound = tokio::time::timeout(Duration::from_secs(1), TcpListener::bind(health_address))
        .await
        .expect("cancelled host should promptly release its health listener")
        .expect("cancelled host must not leave the health listener detached");
    drop(rebound);
}

async fn run_gateway(gateway: TcpListener, result_tx: oneshot::Sender<()>) {
    let (tcp, _) = gateway.accept().await.unwrap();
    let mut socket = accept_async(tcp).await.unwrap();
    send_json(
        &mut socket,
        json!({"type":"event","event":"connect.challenge","payload":{"nonce":"headless-nonce"}}),
    )
    .await;
    let connect = receive_json(&mut socket).await;
    assert_eq!(connect["method"], "connect");
    assert_eq!(connect["params"]["commands"], json!(["example.status"]));
    assert_eq!(connect["params"]["auth"]["token"], "test-token");
    send_json(
        &mut socket,
        json!({
            "type":"res",
            "id":connect["id"],
            "ok":true,
            "payload":{
                "type":"hello-ok",
                "protocol":4,
                "server":{"version":"test"},
                "auth":{"deviceToken":"issued-device-token"}
            }
        }),
    )
    .await;
    send_json(
        &mut socket,
        json!({
            "type":"event",
            "event":"node.invoke.request",
            "payload":{
                "id":"status-1",
                "nodeId":"node-1",
                "command":"example.status",
                "paramsJSON":"{}",
                "timeoutMs":5000
            }
        }),
    )
    .await;
    let result = receive_json(&mut socket).await;
    assert_eq!(result["method"], "node.invoke.result");
    assert_eq!(result["params"]["payload"]["ready"], true);
    send_json(
        &mut socket,
        json!({"type":"res","id":result["id"],"ok":true,"payload":{"accepted":true}}),
    )
    .await;
    socket.close(None).await.unwrap();

    let (tcp, _) = gateway.accept().await.unwrap();
    let mut socket = accept_async(tcp).await.unwrap();
    send_json(
        &mut socket,
        json!({"type":"event","event":"connect.challenge","payload":{"nonce":"reconnect-nonce"}}),
    )
    .await;
    let reconnect = receive_json(&mut socket).await;
    assert_eq!(reconnect["method"], "connect");
    assert_eq!(
        reconnect["params"]["auth"]["deviceToken"],
        "issued-device-token"
    );
    assert!(reconnect["params"]["auth"].get("token").is_none());
    send_json(
        &mut socket,
        json!({
            "type":"res",
            "id":reconnect["id"],
            "ok":true,
            "payload":{"type":"hello-ok","protocol":4,"server":{"version":"test"}}
        }),
    )
    .await;
    result_tx.send(()).unwrap();
    let close = tokio::time::timeout(Duration::from_secs(5), socket.next())
        .await
        .expect("host should close its socket on shutdown");
    assert!(matches!(close, Some(Ok(Message::Close(_))) | None));
}

async fn unused_loopback_address() -> std::net::SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    listener.local_addr().unwrap()
}

fn write_config(path: &Path, gateway: std::net::SocketAddr, health: std::net::SocketAddr) {
    std::fs::write(
        path,
        serde_json::to_vec(&json!({
            "gatewayUrl": format!("ws://{gateway}"),
            "healthListen": health.to_string(),
            "statusCommand": "example.status",
            "identitySecretEnv": "UNUSED_IN_INJECTED_TEST",
        }))
        .unwrap(),
    )
    .unwrap();
}

async fn health_get(address: std::net::SocketAddr, path: &str) -> String {
    for _ in 0..50 {
        match TcpStream::connect(address).await {
            Ok(mut stream) => {
                stream
                    .write_all(format!("GET {path} HTTP/1.1\r\nHost: localhost\r\n\r\n").as_bytes())
                    .await
                    .unwrap();
                let mut response = Vec::new();
                stream.read_to_end(&mut response).await.unwrap();
                return String::from_utf8(response).unwrap();
            }
            Err(_) => tokio::time::sleep(Duration::from_millis(10)).await,
        }
    }
    panic!("health listener did not start");
}

async fn send_json<S>(socket: &mut S, value: Value)
where
    S: futures_util::Sink<Message> + Unpin,
    S::Error: std::fmt::Debug,
{
    socket
        .send(Message::Text(value.to_string().into()))
        .await
        .unwrap();
}

async fn receive_json<S>(socket: &mut S) -> Value
where
    S: futures_util::Stream<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    let message = socket.next().await.unwrap().unwrap();
    serde_json::from_slice(&message.into_data()).unwrap()
}
