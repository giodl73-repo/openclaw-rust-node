use futures_util::{SinkExt, StreamExt};
use openclaw_node::{
    ClientError, CommandRuntime, HandlerError, NodeClient, NodeClientConfig, NodeConnectOptions,
    NodeInvocation, NodeSession, RuntimeError,
};
use serde_json::{Value, json};
use std::{convert::Infallible, time::Duration};
use tokio::{net::TcpListener, sync::Notify};
use tokio_tungstenite::{accept_async, tungstenite::Message};

#[tokio::test]
async fn runtime_routes_success_failure_timeout_and_unknown_commands() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let mut socket = accept_async(tcp).await.unwrap();
        complete_handshake(&mut socket).await;

        for (index, (command, expected_ok, expected_code)) in [
            ("example.ok", true, None),
            ("example.fail", false, Some("NOT_READY")),
            ("example.wait", false, Some("HANDLER_TIMEOUT")),
            ("example.missing", false, Some("COMMAND_NOT_FOUND")),
        ]
        .into_iter()
        .enumerate()
        {
            let timeout_ms = if command == "example.wait" { 10 } else { 1_000 };
            send_json(
                &mut socket,
                json!({
                    "type":"event",
                    "event":"node.invoke.request",
                    "payload":{
                        "id":format!("invoke-{index}"),
                        "nodeId":"node-1",
                        "command":command,
                        "paramsJSON":"{\"value\":1}",
                        "timeoutMs":timeout_ms
                    }
                }),
            )
            .await;
            let result = receive_json(&mut socket).await;
            assert_eq!(result["method"], "node.invoke.result");
            assert_eq!(result["params"]["ok"], expected_ok);
            if let Some(code) = expected_code {
                assert_eq!(result["params"]["error"]["code"], code);
            } else {
                assert_eq!(result["params"]["payload"], json!({"ready":true}));
            }
            send_json(
                &mut socket,
                json!({"type":"res","id":result["id"],"ok":true,"payload":{}}),
            )
            .await;
        }
        socket.close(None).await.unwrap();
    });

    let session = connect_runtime_node(address.to_string()).await;
    let runtime = CommandRuntime::builder()
        .default_timeout(Duration::from_millis(20))
        .command("example.ok", |_context| async { Ok(json!({"ready":true})) })
        .command("example.fail", |_context| async {
            Err(HandlerError::new("NOT_READY", "dependency unavailable"))
        })
        .command("example.wait", |_context| async {
            std::future::pending::<Result<Value, HandlerError>>().await
        })
        .build()
        .unwrap();
    assert!(runtime.run(session).await.is_err());
    server.await.unwrap();
}

#[tokio::test]
async fn runtime_limits_original_noncanonical_parameter_bytes() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let mut socket = accept_async(tcp).await.unwrap();
        complete_handshake(&mut socket).await;
        send_json(
            &mut socket,
            json!({
                "type":"event",
                "event":"node.invoke.request",
                "payload":{
                    "id":"noncanonical-input",
                    "nodeId":"node-1",
                    "command":"example.ok",
                    "paramsJSON":format!("[{}]", " ".repeat(64))
                }
            }),
        )
        .await;
        let result = receive_json(&mut socket).await;
        assert_eq!(result["params"]["error"]["code"], "INPUT_TOO_LARGE");
        send_json(
            &mut socket,
            json!({"type":"res","id":result["id"],"ok":true,"payload":{}}),
        )
        .await;
        socket.close(None).await.unwrap();
    });

    let runtime = CommandRuntime::builder()
        .max_input_bytes(8)
        .command("example.ok", |_context| async { Ok(Value::Null) })
        .build()
        .unwrap();
    let session = connect_runtime_node(address.to_string()).await;
    assert!(runtime.run(session).await.is_err());
    server.await.unwrap();
}

#[tokio::test]
async fn buffered_invocation_deadline_expires_before_handler_execution() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let mut socket = accept_async(tcp).await.unwrap();
        complete_handshake(&mut socket).await;
        send_json(
            &mut socket,
            json!({
                "type":"event",
                "event":"node.invoke.request",
                "payload":{
                    "id":"stale-invocation",
                    "nodeId":"node-1",
                    "command":"example.ok",
                    "timeoutMs":50
                }
            }),
        )
        .await;
        let result = receive_json(&mut socket).await;
        assert_eq!(result["params"]["error"]["code"], "HANDLER_TIMEOUT");
        send_json(
            &mut socket,
            json!({"type":"res","id":result["id"],"ok":true,"payload":{}}),
        )
        .await;
        socket.close(None).await.unwrap();
    });

    let executed = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let handler_executed = executed.clone();
    let runtime = CommandRuntime::builder()
        .result_grace(Duration::from_millis(5))
        .command("example.ok", move |_context| {
            handler_executed.store(true, std::sync::atomic::Ordering::SeqCst);
            async { Ok(Value::Null) }
        })
        .build()
        .unwrap();
    let session = connect_runtime_node(address.to_string()).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(runtime.run(session).await.is_err());
    assert!(!executed.load(std::sync::atomic::Ordering::SeqCst));
    server.await.unwrap();
}

#[tokio::test]
async fn direct_dispatch_rechecks_mutated_parameter_size() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let mut socket = accept_async(tcp).await.unwrap();
        complete_handshake(&mut socket).await;
        send_json(
            &mut socket,
            json!({
                "type":"event",
                "event":"node.invoke.request",
                "payload":{
                    "id":"mutated-input",
                    "nodeId":"node-1",
                    "command":"example.ok",
                    "paramsJSON":"{}"
                }
            }),
        )
        .await;
        let result = receive_json(&mut socket).await;
        assert_eq!(result["params"]["error"]["code"], "INPUT_TOO_LARGE");
        send_json(
            &mut socket,
            json!({"type":"res","id":result["id"],"ok":true,"payload":{}}),
        )
        .await;
        socket.close(None).await.unwrap();
    });

    let runtime = CommandRuntime::builder()
        .max_input_bytes(8)
        .command("example.ok", |_context| async { Ok(Value::Null) })
        .build()
        .unwrap();
    let session = connect_runtime_node(address.to_string()).await;
    let mut invocation = session.next_invocation().await.unwrap();
    invocation.params = json!({"expanded":"well beyond the configured limit"});
    runtime.dispatch(&session, invocation).await.unwrap();
    server.await.unwrap();
}

#[tokio::test]
async fn runtime_rejects_saturation_without_queueing() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let entered = std::sync::Arc::new(Notify::new());
    let release = std::sync::Arc::new(Notify::new());
    let server_entered = entered.clone();
    let server_release = release.clone();
    let server = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let mut socket = accept_async(tcp).await.unwrap();
        complete_handshake(&mut socket).await;
        send_invoke(&mut socket, "invoke-1", "example.block").await;
        server_entered.notified().await;
        send_invoke(&mut socket, "invoke-2", "example.block").await;

        let overloaded = receive_json(&mut socket).await;
        assert_eq!(overloaded["params"]["id"], "invoke-2");
        assert_eq!(overloaded["params"]["error"]["code"], "OVERLOADED");
        send_json(
            &mut socket,
            json!({"type":"res","id":overloaded["id"],"ok":true,"payload":{}}),
        )
        .await;

        server_release.notify_one();
        let completed = receive_json(&mut socket).await;
        assert_eq!(completed["params"]["id"], "invoke-1");
        assert_eq!(completed["params"]["ok"], true);
        send_json(
            &mut socket,
            json!({"type":"res","id":completed["id"],"ok":true,"payload":{}}),
        )
        .await;
        socket.close(None).await.unwrap();
    });

    let handler_entered = entered.clone();
    let handler_release = release.clone();
    let runtime = CommandRuntime::builder()
        .max_concurrency(1)
        .command("example.block", move |_context| {
            let entered = handler_entered.clone();
            let release = handler_release.clone();
            async move {
                entered.notify_one();
                release.notified().await;
                Ok(json!({"done":true}))
            }
        })
        .build()
        .unwrap();
    let session = connect_runtime_node(address.to_string()).await;
    assert!(runtime.run(session).await.is_err());
    server.await.unwrap();
}

#[tokio::test]
async fn result_delivery_remains_inside_the_concurrency_bound() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let mut socket = accept_async(tcp).await.unwrap();
        complete_handshake(&mut socket).await;
        send_invoke(&mut socket, "invoke-1", "example.ok").await;
        let first = receive_json(&mut socket).await;
        assert_eq!(first["params"]["id"], "invoke-1");
        assert_eq!(first["params"]["ok"], true);

        send_invoke(&mut socket, "invoke-2", "example.ok").await;
        let overloaded = receive_json(&mut socket).await;
        assert_eq!(overloaded["params"]["id"], "invoke-2");
        assert_eq!(overloaded["params"]["error"]["code"], "OVERLOADED");
        send_json(
            &mut socket,
            json!({"type":"res","id":overloaded["id"],"ok":true,"payload":{}}),
        )
        .await;
        send_json(
            &mut socket,
            json!({"type":"res","id":first["id"],"ok":true,"payload":{}}),
        )
        .await;
        socket.close(None).await.unwrap();
    });

    let runtime = CommandRuntime::builder()
        .max_concurrency(1)
        .command("example.ok", |_context| async { Ok(json!({"done":true})) })
        .build()
        .unwrap();
    let session = connect_runtime_node(address.to_string()).await;
    assert!(runtime.run(session).await.is_err());
    server.await.unwrap();
}

#[tokio::test]
async fn invocation_id_remains_active_until_result_acknowledgement() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let mut socket = accept_async(tcp).await.unwrap();
        complete_handshake(&mut socket).await;
        send_invoke(&mut socket, "same-id", "example.ok").await;
        let first = receive_json(&mut socket).await;
        assert_eq!(first["params"]["ok"], true);

        send_invoke(&mut socket, "same-id", "example.missing").await;
        let duplicate = receive_json(&mut socket).await;
        assert_eq!(duplicate["params"]["error"]["code"], "DUPLICATE_INVOCATION");
        send_json(
            &mut socket,
            json!({"type":"res","id":duplicate["id"],"ok":true,"payload":{}}),
        )
        .await;
        send_json(
            &mut socket,
            json!({"type":"res","id":first["id"],"ok":true,"payload":{}}),
        )
        .await;
        socket.close(None).await.unwrap();
    });

    let runtime = CommandRuntime::builder()
        .max_concurrency(2)
        .command("example.ok", |_context| async { Ok(Value::Null) })
        .build()
        .unwrap();
    let session = connect_runtime_node(address.to_string()).await;
    assert!(runtime.run(session).await.is_err());
    server.await.unwrap();
}

#[tokio::test]
async fn result_delivery_errors_stop_the_runtime() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let mut socket = accept_async(tcp).await.unwrap();
        complete_handshake(&mut socket).await;
        send_invoke(&mut socket, "invoke-rejected", "example.ok").await;
        let result = receive_json(&mut socket).await;
        send_json(
            &mut socket,
            json!({
                "type":"res",
                "id":result["id"],
                "ok":false,
                "error":{"code":"RESULT_REJECTED","message":"result rejected"}
            }),
        )
        .await;
        tokio::time::sleep(Duration::from_secs(1)).await;
    });

    let runtime = CommandRuntime::builder()
        .command("example.ok", |_context| async { Ok(Value::Null) })
        .build()
        .unwrap();
    let session = connect_runtime_node(address.to_string()).await;
    let error = runtime.run(session).await.unwrap_err();
    assert!(matches!(
        error,
        RuntimeError::Client(ClientError::Gateway { ref code, .. })
            if code == "RESULT_REJECTED"
    ));
    server.abort();
}

#[tokio::test]
async fn fatal_invocation_stream_error_closes_retained_session_clones() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let mut socket = accept_async(tcp).await.unwrap();
        complete_handshake(&mut socket).await;
        send_json(
            &mut socket,
            json!({
                "type":"event",
                "event":"node.invoke.request",
                "payload":{
                    "id":"malformed-invocation",
                    "nodeId":"node-1",
                    "command":"example.ok",
                    "paramsJSON":"{"
                }
            }),
        )
        .await;
        while let Some(message) = socket.next().await {
            if matches!(message.unwrap(), Message::Close(_)) {
                break;
            }
        }
    });

    let runtime = CommandRuntime::builder()
        .command("example.ok", |_context| async { Ok(Value::Null) })
        .build()
        .unwrap();
    let session = connect_runtime_node(address.to_string()).await;
    let observer = session.clone();
    let error = runtime.run(session).await.unwrap_err();
    assert!(matches!(
        error,
        RuntimeError::Client(ClientError::InvalidFrame(_))
    ));
    assert!(
        tokio::time::timeout(Duration::from_secs(1), observer.wait_closed())
            .await
            .expect("fatal invocation error should close retained session clones")
            .is_err()
    );
    server.await.unwrap();
}

#[tokio::test]
async fn direct_dispatch_cancels_child_work_on_disconnect() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let started = std::sync::Arc::new(Notify::new());
    let cancelled = std::sync::Arc::new(Notify::new());
    let server_started = started.clone();
    let server = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let mut socket = accept_async(tcp).await.unwrap();
        complete_handshake(&mut socket).await;
        server_started.notified().await;
        socket.close(None).await.unwrap();
    });

    let handler_started = started.clone();
    let handler_cancelled = cancelled.clone();
    let runtime = CommandRuntime::builder()
        .command("example.wait", move |context| {
            let started = handler_started.clone();
            let cancelled = handler_cancelled.clone();
            async move {
                let token = context.cancellation.clone();
                tokio::spawn(async move {
                    started.notify_one();
                    token.cancelled().await;
                    cancelled.notify_one();
                });
                std::future::pending::<Result<Value, HandlerError>>().await
            }
        })
        .build()
        .unwrap();
    let session = connect_runtime_node(address.to_string()).await;
    let invocation =
        NodeInvocation::new("direct-disconnect", "node-1", "example.wait", Value::Null);
    assert!(runtime.dispatch(&session, invocation).await.is_err());
    tokio::time::timeout(Duration::from_secs(1), cancelled.notified())
        .await
        .expect("direct dispatch should cancel child work");
    server.await.unwrap();
}

#[tokio::test]
async fn concurrent_direct_dispatch_rejects_duplicate_ids_per_session() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let entered = std::sync::Arc::new(Notify::new());
    let release = std::sync::Arc::new(Notify::new());
    let server_release = release.clone();
    let server = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let mut socket = accept_async(tcp).await.unwrap();
        complete_handshake(&mut socket).await;
        let duplicate = receive_json(&mut socket).await;
        assert_eq!(duplicate["params"]["error"]["code"], "DUPLICATE_INVOCATION");
        send_json(
            &mut socket,
            json!({"type":"res","id":duplicate["id"],"ok":true,"payload":{}}),
        )
        .await;
        server_release.notify_one();
        let success = receive_json(&mut socket).await;
        assert_eq!(success["params"]["ok"], true);
        send_json(
            &mut socket,
            json!({"type":"res","id":success["id"],"ok":true,"payload":{}}),
        )
        .await;
        socket.close(None).await.unwrap();
    });

    let handler_entered = entered.clone();
    let handler_release = release.clone();
    let runtime = CommandRuntime::builder()
        .max_concurrency(2)
        .command("example.block", move |_context| {
            let entered = handler_entered.clone();
            let release = handler_release.clone();
            async move {
                entered.notify_one();
                release.notified().await;
                Ok(Value::Null)
            }
        })
        .build()
        .unwrap();
    let session = connect_runtime_node(address.to_string()).await;
    let invocation = NodeInvocation::new("same-direct-id", "node-1", "example.block", Value::Null);
    let first_runtime = runtime.clone();
    let first_session = session.clone();
    let first_invocation = invocation.clone();
    let first = tokio::spawn(async move {
        first_runtime
            .dispatch(&first_session, first_invocation)
            .await
    });
    entered.notified().await;
    runtime.dispatch(&session, invocation).await.unwrap();
    first.await.unwrap().unwrap();
    server.await.unwrap();
}

#[tokio::test]
async fn direct_dispatch_delivery_saturation_is_bounded_and_closes_session() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let entered = std::sync::Arc::new(Notify::new());
    let release = std::sync::Arc::new(Notify::new());
    let overload_received = std::sync::Arc::new(Notify::new());
    let server_overload_received = overload_received.clone();
    let server = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let mut socket = accept_async(tcp).await.unwrap();
        complete_handshake(&mut socket).await;
        let overloaded = receive_json(&mut socket).await;
        assert_eq!(overloaded["params"]["error"]["code"], "OVERLOADED");
        server_overload_received.notify_one();
        while let Some(message) = socket.next().await {
            if matches!(message.unwrap(), Message::Close(_)) {
                break;
            }
        }
    });

    let handler_entered = entered.clone();
    let handler_release = release.clone();
    let runtime = CommandRuntime::builder()
        .max_concurrency(1)
        .command("example.block", move |_context| {
            let entered = handler_entered.clone();
            let release = handler_release.clone();
            async move {
                entered.notify_one();
                release.notified().await;
                Ok(Value::Null)
            }
        })
        .build()
        .unwrap();
    let session = connect_runtime_node(address.to_string()).await;
    let observer = session.clone();
    let first_runtime = runtime.clone();
    let first_session = session.clone();
    let first = tokio::spawn(async move {
        first_runtime
            .dispatch(
                &first_session,
                NodeInvocation::new("direct-1", "node-1", "example.block", Value::Null),
            )
            .await
    });
    entered.notified().await;
    let second_runtime = runtime.clone();
    let second_session = session.clone();
    let second = tokio::spawn(async move {
        second_runtime
            .dispatch(
                &second_session,
                NodeInvocation::new("direct-2", "node-1", "example.block", Value::Null),
            )
            .await
    });
    overload_received.notified().await;
    let error = runtime
        .dispatch(
            &session,
            NodeInvocation::new("direct-3", "node-1", "example.block", Value::Null),
        )
        .await
        .unwrap_err();
    assert!(matches!(error, RuntimeError::DeliverySaturated));
    assert!(
        tokio::time::timeout(Duration::from_secs(1), observer.wait_closed())
            .await
            .expect("direct delivery saturation should close the session")
            .is_err()
    );
    release.notify_one();
    assert!(first.await.unwrap().is_err());
    assert!(second.await.unwrap().is_err());
    server.await.unwrap();
}

#[tokio::test]
async fn overload_delivery_saturation_fails_closed() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let mut socket = accept_async(tcp).await.unwrap();
        complete_handshake(&mut socket).await;
        send_invoke(&mut socket, "invoke-1", "example.ok").await;
        let first = receive_json(&mut socket).await;
        assert_eq!(first["params"]["ok"], true);
        send_invoke(&mut socket, "invoke-2", "example.ok").await;
        let overloaded = receive_json(&mut socket).await;
        assert_eq!(overloaded["params"]["error"]["code"], "OVERLOADED");
        send_invoke(&mut socket, "invoke-3", "example.ok").await;
        send_invoke(&mut socket, "invoke-4", "example.ok").await;
        tokio::time::sleep(Duration::from_secs(1)).await;
    });

    let runtime = CommandRuntime::builder()
        .max_concurrency(1)
        .command("example.ok", |_context| async { Ok(Value::Null) })
        .build()
        .unwrap();
    let session = connect_runtime_node(address.to_string()).await;
    let observer = session.clone();
    let error = tokio::time::timeout(Duration::from_secs(1), runtime.run(session))
        .await
        .expect("delivery saturation should fail closed")
        .unwrap_err();
    assert!(matches!(error, RuntimeError::DeliverySaturated));
    assert!(
        tokio::time::timeout(Duration::from_secs(1), observer.wait_closed())
            .await
            .expect("delivery saturation should close retained session clones")
            .is_err()
    );
    server.abort();
}

#[tokio::test]
async fn disconnect_cancels_child_work_and_stops_handlers() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let child_started = std::sync::Arc::new(Notify::new());
    let child_cancelled = std::sync::Arc::new(Notify::new());
    let server_started = child_started.clone();
    let server = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let mut socket = accept_async(tcp).await.unwrap();
        complete_handshake(&mut socket).await;
        send_invoke(&mut socket, "invoke-disconnect", "example.wait").await;
        server_started.notified().await;
        socket.close(None).await.unwrap();
    });

    let handler_started = child_started.clone();
    let handler_cancelled = child_cancelled.clone();
    let runtime = CommandRuntime::builder()
        .command("example.wait", move |context| {
            let started = handler_started.clone();
            let cancelled = handler_cancelled.clone();
            async move {
                let token = context.cancellation.clone();
                tokio::spawn(async move {
                    started.notify_one();
                    token.cancelled().await;
                    cancelled.notify_one();
                });
                std::future::pending::<Result<Value, HandlerError>>().await
            }
        })
        .build()
        .unwrap();
    let session = connect_runtime_node(address.to_string()).await;
    assert!(runtime.run(session).await.is_err());
    tokio::time::timeout(Duration::from_secs(1), child_cancelled.notified())
        .await
        .expect("disconnect should cancel child work");
    server.await.unwrap();
}

async fn connect_runtime_node(address: String) -> NodeSession {
    NodeClient::connect(
        NodeClientConfig::new(format!("ws://{address}")),
        |_nonce| async {
            Ok::<_, Infallible>(
                NodeConnectOptions::new("test", "test")
                    .command("example.ok")
                    .command("example.fail")
                    .command("example.wait")
                    .command("example.block")
                    .activate(),
            )
        },
    )
    .await
    .unwrap()
}

async fn complete_handshake<S>(socket: &mut tokio_tungstenite::WebSocketStream<S>)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    send_json(
        socket,
        json!({"type":"event","event":"connect.challenge","payload":{"nonce":"runtime-nonce"}}),
    )
    .await;
    let connect = receive_json(socket).await;
    send_json(
        socket,
        json!({"type":"res","id":connect["id"],"ok":true,"payload":{"type":"hello-ok","protocol":4}}),
    )
    .await;
}

async fn send_invoke<S>(socket: &mut tokio_tungstenite::WebSocketStream<S>, id: &str, command: &str)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    send_json(
        socket,
        json!({
            "type":"event",
            "event":"node.invoke.request",
            "payload":{"id":id,"nodeId":"node-1","command":command}
        }),
    )
    .await;
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
