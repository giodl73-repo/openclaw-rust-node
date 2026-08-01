use std::{env, error::Error, io, time::Duration};

use base64::{engine::general_purpose::STANDARD, Engine as _};
use openclaw_node_host::{
    read_sidecar_frame, write_sidecar_frame, AuthenticatedSidecarChannel, SidecarAdmissionDecision,
    SidecarConfigurationExchange, SidecarHandshake, SidecarInvocation, SidecarInvocationResult,
    SidecarLimits, SidecarPeerIdentity, SidecarPeerRole, SidecarProtocolOffer,
    SidecarRuntimeMessage, SidecarSessionKey, SIDECAR_PROTOCOL_MAJOR, SIDECAR_PROTOCOL_MINOR,
};
use serde_json::json;

const BOOTSTRAP_FRAME_LIMIT: u32 = 4096;
const IO_DEADLINE: Duration = Duration::from_secs(10);

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let session_id = required_env("OPENCLAW_SIDECAR_SESSION_ID")?;
    let generation = required_env("OPENCLAW_SIDECAR_GENERATION")?.parse::<u64>()?;
    let key_bytes = STANDARD.decode(required_env("OPENCLAW_SIDECAR_KEY_BASE64")?)?;
    let key: [u8; 32] = key_bytes.try_into().map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "sidecar key must contain 32 bytes",
        )
    })?;

    let mut input = tokio::io::stdin();
    let mut output = tokio::io::stdout();
    let mut channel = AuthenticatedSidecarChannel::new(
        SidecarPeerRole::Runtime,
        session_id,
        generation,
        SidecarSessionKey::from_bytes(key),
        BOOTSTRAP_FRAME_LIMIT,
    )?;

    let frame_limit = establish_session(&mut channel, &mut input, &mut output).await?;
    exercise_dispatch(&mut channel, &mut input, &mut output, frame_limit).await?;
    eprintln!("windows sidecar process probe passed");
    Ok(())
}

async fn establish_session<R, W>(
    channel: &mut AuthenticatedSidecarChannel,
    input: &mut R,
    output: &mut W,
) -> Result<u32, Box<dyn Error>>
where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    let mut handshake = SidecarHandshake::new(SidecarProtocolOffer {
        protocol_major: SIDECAR_PROTOCOL_MAJOR,
        protocol_minor: SIDECAR_PROTOCOL_MINOR,
        peer: SidecarPeerIdentity {
            role: SidecarPeerRole::Runtime,
            name: "openclaw-node".to_owned(),
            version: "1.0.0".to_owned(),
            artifact_identity: "sha256:test-only-process-probe".to_owned(),
        },
        feature_bits: 11,
        limits: SidecarLimits {
            max_frame_bytes: 2048,
            max_in_flight: 8,
            bootstrap_timeout_ms: 1000,
        },
    })?;

    let offer = read_sidecar_frame(input, BOOTSTRAP_FRAME_LIMIT, IO_DEADLINE).await?;
    let acceptance = handshake
        .receive(channel, &offer)?
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "offer had no acceptance"))?;
    write_sidecar_frame(output, &acceptance, BOOTSTRAP_FRAME_LIMIT, IO_DEADLINE).await?;
    handshake.complete_acceptance(channel)?;

    let frame_limit = channel.max_frame_bytes();
    let mut configuration = SidecarConfigurationExchange::new(handshake)?;
    let configure = read_sidecar_frame(input, frame_limit, IO_DEADLINE).await?;
    let received = configuration
        .receive(channel, &configure)?
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "configuration was absent"))?;
    let manifest = configuration
        .validated_manifest()
        .cloned()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "manifest was absent"))?;
    if !received
        .commands
        .iter()
        .any(|command| command.name == "product.status")
    {
        return Err(
            io::Error::new(io::ErrorKind::InvalidData, "product.status not offered").into(),
        );
    }
    let configured = configuration.acknowledge(channel, &manifest)?;
    write_sidecar_frame(output, &configured, frame_limit, IO_DEADLINE).await?;
    configuration.complete_acknowledgement(channel)?;

    Ok(frame_limit)
}

async fn exercise_dispatch<R, W>(
    channel: &mut AuthenticatedSidecarChannel,
    input: &mut R,
    output: &mut W,
    frame_limit: u32,
) -> Result<(), Box<dyn Error>>
where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    let invocation = SidecarInvocation {
        id: "process-invoke-1".to_owned(),
        node_id: "node-1".to_owned(),
        command: "product.status".to_owned(),
        params: json!({ "verbose": true }),
        timeout_ms: Some(1000),
        idempotency_key: None,
        session_key: None,
    };
    send_message(
        channel,
        output,
        frame_limit,
        &SidecarRuntimeMessage::AdmissionRequest {
            invocation: invocation.clone(),
        },
    )
    .await?;
    let decision = receive_message(channel, input, frame_limit).await?;
    if !matches!(
        decision,
        SidecarRuntimeMessage::AdmissionDecision {
            ref invocation_id,
            decision: SidecarAdmissionDecision::Allow,
        } if invocation_id == &invocation.id
    ) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "invocation was not allowed",
        )
        .into());
    }

    send_message(
        channel,
        output,
        frame_limit,
        &SidecarRuntimeMessage::Invoke {
            invocation: invocation.clone(),
        },
    )
    .await?;
    let result = receive_message(channel, input, frame_limit).await?;
    if !matches!(
        result,
        SidecarRuntimeMessage::Result {
            ref invocation_id,
            result: SidecarInvocationResult::Success { ref payload },
        } if invocation_id == &invocation.id && payload == &json!({ "ready": true })
    ) {
        return Err(
            io::Error::new(io::ErrorKind::InvalidData, "unexpected invocation result").into(),
        );
    }

    Ok(())
}

async fn send_message<W: tokio::io::AsyncWrite + Unpin>(
    channel: &mut AuthenticatedSidecarChannel,
    output: &mut W,
    frame_limit: u32,
    message: &SidecarRuntimeMessage,
) -> Result<(), Box<dyn Error>> {
    let frame = channel.seal(message)?;
    write_sidecar_frame(output, &frame, frame_limit, IO_DEADLINE).await?;
    Ok(())
}

async fn receive_message<R: tokio::io::AsyncRead + Unpin>(
    channel: &mut AuthenticatedSidecarChannel,
    input: &mut R,
    frame_limit: u32,
) -> Result<SidecarRuntimeMessage, Box<dyn Error>> {
    let frame = read_sidecar_frame(input, frame_limit, IO_DEADLINE).await?;
    Ok(channel.open(&frame)?)
}

fn required_env(name: &str) -> Result<String, io::Error> {
    env::var(name).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("required environment variable {name} is missing"),
        )
    })
}
