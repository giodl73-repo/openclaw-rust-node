use std::{error::Error, fs::File, io, io::Read as _, time::Duration};

use openclaw_node_host::{
    read_sidecar_frame, write_sidecar_frame, AuthenticatedSidecarChannel, SidecarAdmissionDecision,
    SidecarConfigurationExchange, SidecarHandshake, SidecarInvocation, SidecarInvocationResult,
    SidecarLimits, SidecarPeerIdentity, SidecarPeerRole, SidecarProtocolOffer,
    SidecarRuntimeMessage, SidecarSessionKey, SIDECAR_PROTOCOL_MAJOR, SIDECAR_PROTOCOL_MINOR,
};
use serde_json::json;
use sha2::{Digest, Sha256};

const BOOTSTRAP_FRAME_LIMIT: u32 = 4096;
const BOOTSTRAP_PAYLOAD_LIMIT: u32 = 1024;
const BOOTSTRAP_MAGIC: &[u8; 4] = b"OCSB";
const BOOTSTRAP_VERSION: u16 = 1;
const BOOTSTRAP_FIXED_BYTES: usize = 52;
const MAX_ARTIFACT_BYTES: u64 = 256 * 1024 * 1024;
const HEX: &[u8; 16] = b"0123456789abcdef";
const IO_DEADLINE: Duration = Duration::from_secs(10);

struct Bootstrap {
    session_id: String,
    generation: u64,
    max_frame_bytes: u32,
    key: [u8; 32],
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let mut input = tokio::io::stdin();
    let mut output = tokio::io::stdout();
    let mut bootstrap_frame =
        read_sidecar_frame(&mut input, BOOTSTRAP_PAYLOAD_LIMIT, IO_DEADLINE).await?;
    let bootstrap_result = decode_bootstrap(&bootstrap_frame);
    bootstrap_frame.fill(0);
    let mut bootstrap = bootstrap_result?;
    let session_key = SidecarSessionKey::from_bytes(bootstrap.key);
    bootstrap.key.fill(0);
    let mut channel = AuthenticatedSidecarChannel::new(
        SidecarPeerRole::Runtime,
        bootstrap.session_id,
        bootstrap.generation,
        session_key,
        bootstrap.max_frame_bytes,
    )?;

    let artifact_identity = current_artifact_identity()?;
    let frame_limit =
        establish_session(&mut channel, &mut input, &mut output, artifact_identity).await?;
    exercise_dispatch(&mut channel, &mut input, &mut output, frame_limit).await?;
    eprintln!("windows sidecar process probe passed");
    Ok(())
}

async fn establish_session<R, W>(
    channel: &mut AuthenticatedSidecarChannel,
    input: &mut R,
    output: &mut W,
    artifact_identity: String,
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
            artifact_identity,
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

fn decode_bootstrap(payload: &[u8]) -> Result<Bootstrap, io::Error> {
    if payload.len() < BOOTSTRAP_FIXED_BYTES || payload.get(..4) != Some(BOOTSTRAP_MAGIC) {
        return Err(invalid_bootstrap());
    }
    let version = u16::from_be_bytes(copy_array(&payload[4..6]));
    let generation = u64::from_be_bytes(copy_array(&payload[6..14]));
    let max_frame_bytes = u32::from_be_bytes(copy_array(&payload[14..18]));
    let session_bytes = usize::from(u16::from_be_bytes(copy_array(&payload[18..20])));
    if version != BOOTSTRAP_VERSION
        || max_frame_bytes > BOOTSTRAP_FRAME_LIMIT
        || payload.len() != BOOTSTRAP_FIXED_BYTES + session_bytes
    {
        return Err(invalid_bootstrap());
    }
    let mut key = [0_u8; 32];
    key.copy_from_slice(&payload[20..52]);
    let session_id = std::str::from_utf8(&payload[52..])
        .map_err(|_| invalid_bootstrap())?
        .to_owned();
    Ok(Bootstrap {
        session_id,
        generation,
        max_frame_bytes,
        key,
    })
}

fn current_artifact_identity() -> Result<String, io::Error> {
    let path = std::env::current_exe()?;
    let mut artifact = File::open(path)?;
    if artifact.metadata()?.len() > MAX_ARTIFACT_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "sidecar artifact exceeds the verification limit",
        ));
    }
    let mut digest = Sha256::new();
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        let count = artifact.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    buffer.fill(0);
    let mut identity = String::with_capacity(71);
    identity.push_str("sha256:");
    for byte in digest.finalize() {
        identity.push(char::from(HEX[usize::from(byte >> 4)]));
        identity.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    Ok(identity)
}

fn copy_array<const N: usize>(bytes: &[u8]) -> [u8; N] {
    let mut value = [0_u8; N];
    value.copy_from_slice(bytes);
    value
}

fn invalid_bootstrap() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "invalid sidecar bootstrap")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bootstrap_decodes_exact_bounded_record() {
        let payload = bootstrap_payload(BOOTSTRAP_VERSION, 7, 2048, "session-7");

        let decoded = decode_bootstrap(&payload).expect("valid bootstrap");

        assert_eq!(decoded.session_id, "session-7");
        assert_eq!(decoded.generation, 7);
        assert_eq!(decoded.max_frame_bytes, 2048);
        assert_eq!(decoded.key, [0x5a; 32]);
    }

    #[test]
    fn bootstrap_rejects_unknown_version_and_oversized_limit() {
        assert!(decode_bootstrap(&bootstrap_payload(2, 7, 2048, "session-7")).is_err());
        assert!(decode_bootstrap(&bootstrap_payload(
            BOOTSTRAP_VERSION,
            7,
            BOOTSTRAP_FRAME_LIMIT + 1,
            "session-7",
        ))
        .is_err());
    }

    #[test]
    fn bootstrap_rejects_trailing_or_invalid_utf8_bytes() {
        let mut trailing = bootstrap_payload(BOOTSTRAP_VERSION, 7, 2048, "session-7");
        trailing.push(0);
        assert!(decode_bootstrap(&trailing).is_err());

        let mut invalid_utf8 = bootstrap_payload(BOOTSTRAP_VERSION, 7, 2048, "session-7");
        *invalid_utf8.last_mut().expect("session byte") = 0xff;
        assert!(decode_bootstrap(&invalid_utf8).is_err());
    }

    fn bootstrap_payload(version: u16, generation: u64, limit: u32, session: &str) -> Vec<u8> {
        let mut payload = vec![0_u8; BOOTSTRAP_FIXED_BYTES + session.len()];
        payload[..4].copy_from_slice(BOOTSTRAP_MAGIC);
        payload[4..6].copy_from_slice(&version.to_be_bytes());
        payload[6..14].copy_from_slice(&generation.to_be_bytes());
        payload[14..18].copy_from_slice(&limit.to_be_bytes());
        payload[18..20].copy_from_slice(
            &u16::try_from(session.len())
                .expect("test session fits")
                .to_be_bytes(),
        );
        payload[20..52].fill(0x5a);
        payload[52..].copy_from_slice(session.as_bytes());
        payload
    }
}
