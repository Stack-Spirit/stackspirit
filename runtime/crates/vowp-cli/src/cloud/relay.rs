//! `stackspirit relay proxy`: the client end of the relay's end-to-end SSH channel.
//!
//! Spec: `docs/design/relay-and-packaging.md` §6.5–§6.7. The command mints a short-lived
//! `ssh`-scoped ticket, connects to `wss://relay.stackspirit.io/v1/client`, completes the
//! v6-13 P-256/HKDF/AES-GCM handshake, opens the literal `ssh` TCP stream and copies raw bytes
//! between stdin/stdout and the encrypted channel. It never parses SSH and never sees an SSH
//! private key; the relay sees only ciphertext.
//!
//! Standard output carries tunnel bytes only. Errors are returned to the caller, which prints
//! them on stderr through the normal CLI error path.

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::Path;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use serde_json::Value;
use tungstenite::client::IntoClientRequest;
use tungstenite::http::HeaderValue;
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Error as WsError, Message, WebSocket};
use vowp_agent::relay::{AcceptInput, EphemeralKey, Jwt, TicketClaims, client_finish, jwk_thumbprint, random_nonce};
use vowp_protocol::{
    ClientKind, E2E_SUITE, MAX_TICKET_TTL_SECS, RELAY_FRAME_VERSION, RELAY_SUBPROTOCOL, REMOTE_FRAME_VERSION,
    RelayDataFrame, RelayFrame, RemoteFrame, Scope, Timestamp, codes,
};

use super::{Client, DEFAULT_RELAY_URL, list_machines, mint_ticket, resolve_machine};
use crate::CliError;

/// The fixed stream id this command uses for TCP/SSH.
const SSH_STREAM: u32 = 1;
/// How long to wait for each handshake step.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
/// TCP connect timeout.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// The read timeout that lets the loop drain stdin while the relay is quiet.
const POLL_TIMEOUT: Duration = Duration::from_millis(20);
/// How long to keep reading after stdin reaches EOF before closing.
const EOF_GRACE: Duration = Duration::from_secs(2);
/// Raw bytes read from stdin at a time; 32 KiB encodes well under the 512 KiB frame cap.
const STDIN_CHUNK: usize = 32 * 1024;

/// The WebSocket type returned by the relay connection.
type Socket = WebSocket<MaybeTlsStream<TcpStream>>;

/// One incoming WebSocket message, or an idle poll.
enum Incoming {
    /// The read timed out; the caller may drain stdin and try again.
    Idle,
    /// The peer closed the WebSocket.
    Closed,
    /// One JSON control frame.
    Control(Box<RelayFrame>),
    /// One encrypted binary data frame.
    Data(RelayDataFrame),
}

/// Runs the hidden `relay proxy --machine <id> --stdio` command.
///
/// Returns the process exit code: `0` for a normal close. Errors are returned before any byte
/// is written to stdout.
pub fn proxy(home: &Path, machine_query: &str) -> Result<i32, CliError> {
    let mut client = Client::load(home)?;
    let machines = list_machines(&mut client)?;
    let machine = resolve_machine(&machines, machine_query)?;
    if machine.state == "revoked" {
        return Err(CliError::Guard(format!(
            "machine {} was revoked by its owner",
            machine.id
        )));
    }

    let ephemeral = EphemeralKey::generate();
    let client_key = ephemeral.jwk();
    let client_nonce = random_nonce();
    let response = mint_ticket(&mut client, &machine.id, &client_key, MAX_TICKET_TTL_SECS)?;
    let ticket = response
        .get("ticket")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|ticket| !ticket.is_empty())
        .ok_or_else(|| CliError::Auth("the cloud API returned no relay ticket".to_owned()))?;
    let claims: TicketClaims = Jwt::parse(ticket)
        .and_then(|jwt| jwt.claims())
        .map_err(|_| CliError::Auth("the cloud API returned a malformed relay ticket".to_owned()))?;
    validate_ticket(&claims, &machine.id, &client_key)?;
    let relay_url = response
        .get("relay_url")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|url| !url.is_empty())
        .unwrap_or(DEFAULT_RELAY_URL);

    let client_id = format!("cli_{}", random_nonce());
    let mut socket = connect(relay_url)?;
    send_control(
        &mut socket,
        &RelayFrame::ClientHello {
            v: RELAY_FRAME_VERSION,
            machine_id: claims.machine.clone(),
            ticket: ticket.to_owned(),
            client_id: client_id.clone(),
            kind: ClientKind::Cli,
            protocol: vowp_protocol::REMOTE_PROTOCOL.to_owned(),
            capabilities: vec!["ssh".to_owned()],
            client_key: client_key.clone(),
            client_nonce: client_nonce.clone(),
            sent_at: Timestamp::now(),
        },
    )?;

    let welcome = await_welcome(&mut socket)?;
    let accept = await_accept(&mut socket)?;
    if accept.suite != E2E_SUITE {
        return Err(CliError::Auth(format!(
            "the machine selected an unsupported relay crypto suite: {}",
            accept.suite
        )));
    }
    let input = AcceptInput {
        channel_id: &welcome.channel_id,
        client_key: &client_key,
        client_nonce: &client_nonce,
        machine_id: &welcome.machine_id,
        account_id: &welcome.account_id,
        ticket_jti: &claims.jti,
    };
    let mut cipher = client_finish(
        &ephemeral,
        &input,
        &accept.machine_key,
        &accept.machine_nonce,
        &accept.machine_signature,
        &claims.mkey,
    )
    .map_err(|error| {
        CliError::Auth(format!(
            "the relay channel handshake failed; refusing the tunnel: {error}"
        ))
    })?;

    send_remote(
        &mut socket,
        &mut cipher,
        &RemoteFrame::Hello {
            v: REMOTE_FRAME_VERSION,
            client: client_id,
            machine: welcome.machine_id.clone(),
            scopes: vec![Scope::Ssh],
            capabilities: vec!["ssh".to_owned()],
        },
    )?;
    await_hello_ack(&mut socket, &mut cipher)?;
    set_poll_timeout(&mut socket)?;
    send_remote(
        &mut socket,
        &mut cipher,
        &RemoteFrame::TcpOpen {
            stream: SSH_STREAM,
            target: "ssh".to_owned(),
        },
    )?;

    bridge(&mut socket, &mut cipher)
}

/// Checks the ticket the API just issued before any relay connection is opened.
fn validate_ticket(claims: &TicketClaims, machine_id: &str, client_key: &vowp_protocol::Jwk) -> Result<(), CliError> {
    if claims.machine != machine_id {
        return Err(CliError::Auth(
            "the relay ticket is for another machine; refusing the tunnel".to_owned(),
        ));
    }
    if claims.kind != ClientKind::Cli {
        return Err(CliError::Auth(
            "the relay ticket was not issued for the CLI; refusing the tunnel".to_owned(),
        ));
    }
    if !claims.scopes.contains(&Scope::Ssh) {
        return Err(CliError::Auth(
            "the relay ticket does not grant the ssh scope; refusing the tunnel".to_owned(),
        ));
    }
    if claims.cnf.jkt != jwk_thumbprint(client_key) {
        return Err(CliError::Auth(
            "the relay ticket is bound to another client key; refusing the tunnel".to_owned(),
        ));
    }
    if claims.exp <= Timestamp::now().unix_secs() {
        return Err(CliError::Auth("the relay ticket already expired".to_owned()));
    }
    Ok(())
}

struct Welcome {
    channel_id: String,
    machine_id: String,
    account_id: String,
}

struct Accept {
    machine_key: vowp_protocol::Jwk,
    machine_nonce: String,
    machine_signature: String,
    suite: String,
}

/// Connects one WebSocket, offering the relay subprotocol.
fn connect(relay_url: &str) -> Result<Socket, CliError> {
    let mut request = relay_url
        .into_client_request()
        .map_err(|_| CliError::Invalid("the relay URL is not a WebSocket URL".to_owned()))?;
    request
        .headers_mut()
        .insert("Sec-WebSocket-Protocol", HeaderValue::from_static(RELAY_SUBPROTOCOL));
    let uri = request.uri().clone();
    let tls = match uri.scheme_str() {
        Some("wss") => true,
        Some("ws") => false,
        _ => {
            return Err(CliError::Invalid(
                "the relay URL must start with wss:// (or ws:// for a local test server)".to_owned(),
            ));
        }
    };
    let host = uri
        .host()
        .ok_or_else(|| CliError::Invalid("the relay URL has no host".to_owned()))?
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_owned();
    let port = uri.port_u16().unwrap_or(if tls { 443 } else { 80 });
    let stream = connect_tcp(&host, port)?;
    stream
        .set_nodelay(true)
        .map_err(|error| CliError::Remote(format!("cannot configure the relay socket: {error}")))?;
    stream
        .set_read_timeout(Some(HANDSHAKE_TIMEOUT))
        .map_err(|error| CliError::Remote(format!("cannot configure the relay socket: {error}")))?;
    stream
        .set_write_timeout(Some(HANDSHAKE_TIMEOUT))
        .map_err(|error| CliError::Remote(format!("cannot configure the relay socket: {error}")))?;
    let (socket, response) = if tls {
        tungstenite::client_tls(request, stream)
            .map_err(|error| CliError::Remote(format!("the relay TLS handshake failed: {error}")))?
    } else {
        tungstenite::client::client(request, MaybeTlsStream::Plain(stream))
            .map_err(|error| CliError::Remote(format!("the relay handshake failed: {error}")))?
    };
    let accepted = response
        .headers()
        .get("sec-websocket-protocol")
        .and_then(|value| value.to_str().ok());
    if accepted != Some(RELAY_SUBPROTOCOL) {
        return Err(CliError::Remote(format!(
            "the relay did not accept the {RELAY_SUBPROTOCOL} subprotocol"
        )));
    }
    Ok(socket)
}

/// Resolves and connects the first reachable address.
fn connect_tcp(host: &str, port: u16) -> Result<TcpStream, CliError> {
    let addresses = (host, port)
        .to_socket_addrs()
        .map_err(|error| CliError::Remote(format!("cannot resolve the relay host: {error}")))?;
    let mut last_error = None;
    for address in addresses {
        match TcpStream::connect_timeout(&address, CONNECT_TIMEOUT) {
            Ok(stream) => return Ok(stream),
            Err(error) => last_error = Some(error),
        }
    }
    Err(CliError::Remote(format!(
        "cannot connect to the relay: {}",
        last_error
            .map(|error| error.to_string())
            .unwrap_or_else(|| "no address".to_owned())
    )))
}

/// Sets the short read timeout used to poll stdin between relay messages.
fn set_poll_timeout(socket: &mut Socket) -> Result<(), CliError> {
    match socket.get_mut() {
        MaybeTlsStream::Plain(stream) => stream
            .set_read_timeout(Some(POLL_TIMEOUT))
            .map_err(|error| CliError::Remote(format!("cannot configure the relay socket: {error}"))),
        MaybeTlsStream::Rustls(stream) => stream
            .sock
            .set_read_timeout(Some(POLL_TIMEOUT))
            .map_err(|error| CliError::Remote(format!("cannot configure the relay socket: {error}"))),
        _ => Err(CliError::Remote(
            "the relay connection uses an unsupported transport".to_owned(),
        )),
    }
}

fn send_control(socket: &mut Socket, frame: &RelayFrame) -> Result<(), CliError> {
    let text = frame
        .to_json()
        .map_err(|error| CliError::Remote(format!("cannot encode a relay frame: {error}")))?;
    socket
        .send(Message::text(text))
        .map_err(|error| CliError::Remote(format!("cannot send to the relay: {error}")))?;
    socket
        .flush()
        .map_err(|error| CliError::Remote(format!("cannot flush the relay connection: {error}")))
}

fn send_remote(
    socket: &mut Socket,
    cipher: &mut vowp_agent::relay::ChannelCipher,
    frame: &RemoteFrame,
) -> Result<(), CliError> {
    let plain = frame
        .to_json()
        .map_err(|error| CliError::Remote(format!("cannot encode a remote frame: {error}")))?;
    let data = cipher
        .seal(&plain)
        .map_err(|error| CliError::Remote(format!("cannot encrypt a remote frame: {error}")))?;
    let bytes = data
        .to_bytes()
        .map_err(|error| CliError::Remote(format!("cannot encode a relay data frame: {error}")))?;
    socket
        .send(Message::binary(bytes))
        .map_err(|error| CliError::Remote(format!("cannot send to the relay: {error}")))?;
    socket
        .flush()
        .map_err(|error| CliError::Remote(format!("cannot flush the relay connection: {error}")))
}

/// Reads one WebSocket message, treating a read timeout as an idle poll.
fn read_incoming(socket: &mut Socket) -> Result<Incoming, CliError> {
    match socket.read() {
        Ok(Message::Text(text)) => RelayFrame::from_json(text.as_str())
            .map(|frame| Incoming::Control(Box::new(frame)))
            .map_err(|error| CliError::Remote(format!("the relay sent a bad control frame: {error}"))),
        Ok(Message::Binary(bytes)) => RelayDataFrame::from_bytes(&bytes)
            .map(Incoming::Data)
            .map_err(|error| CliError::Remote(format!("the relay sent a bad data frame: {error}"))),
        Ok(Message::Ping(payload)) => {
            let _ = socket.send(Message::Pong(payload));
            let _ = socket.flush();
            Ok(Incoming::Idle)
        }
        Ok(Message::Pong(_)) | Ok(Message::Frame(_)) => Ok(Incoming::Idle),
        Ok(Message::Close(_)) => Ok(Incoming::Closed),
        Err(WsError::Io(error))
            if matches!(
                error.kind(),
                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
            ) =>
        {
            Ok(Incoming::Idle)
        }
        Err(WsError::ConnectionClosed | WsError::AlreadyClosed) => Ok(Incoming::Closed),
        Err(error) => Err(CliError::Remote(format!("the relay connection failed: {error}"))),
    }
}

fn await_welcome(socket: &mut Socket) -> Result<Welcome, CliError> {
    let deadline = Instant::now() + HANDSHAKE_TIMEOUT;
    loop {
        match next_control(socket, deadline)? {
            RelayFrame::ClientWelcome {
                channel_id,
                machine_id,
                account_id,
                ..
            } => {
                return Ok(Welcome {
                    channel_id,
                    machine_id,
                    account_id,
                });
            }
            RelayFrame::Ping { nonce, .. } => send_pong(socket, nonce)?,
            RelayFrame::Error { code, message, .. } => return Err(control_error(&code, &message)),
            other => {
                if other.type_name() != "pong" {
                    return Err(CliError::Remote(format!(
                        "the relay sent {} before client.welcome",
                        other.type_name()
                    )));
                }
            }
        }
    }
}

fn await_accept(socket: &mut Socket) -> Result<Accept, CliError> {
    let deadline = Instant::now() + HANDSHAKE_TIMEOUT;
    loop {
        match next_control(socket, deadline)? {
            RelayFrame::ChannelAccept {
                machine_key,
                machine_nonce,
                machine_signature,
                suite,
                ..
            } => {
                return Ok(Accept {
                    machine_key,
                    machine_nonce,
                    machine_signature,
                    suite,
                });
            }
            RelayFrame::ChannelReject { code, message, .. } | RelayFrame::ChannelClose { code, message, .. } => {
                return Err(control_error(&code, &message));
            }
            RelayFrame::Ping { nonce, .. } => send_pong(socket, nonce)?,
            RelayFrame::Error { code, message, .. } => return Err(control_error(&code, &message)),
            other => {
                if other.type_name() != "pong" {
                    return Err(CliError::Remote(format!(
                        "the relay sent {} before channel.accept",
                        other.type_name()
                    )));
                }
            }
        }
    }
}

fn await_hello_ack(socket: &mut Socket, cipher: &mut vowp_agent::relay::ChannelCipher) -> Result<(), CliError> {
    let deadline = Instant::now() + HANDSHAKE_TIMEOUT;
    loop {
        match next_remote(socket, cipher, deadline)? {
            RemoteFrame::HelloAck { scopes, .. } => {
                if !scopes.contains(&Scope::Ssh) {
                    return Err(CliError::Auth(
                        "the machine did not grant the ssh scope; the tunnel stays closed".to_owned(),
                    ));
                }
                return Ok(());
            }
            RemoteFrame::Ping { nonce } => send_remote(socket, cipher, &RemoteFrame::Pong { nonce: nonce.clone() })?,
            RemoteFrame::StreamError {
                code,
                message,
                retryable,
                ..
            } => return Err(stream_error(&code, &message, retryable)),
            RemoteFrame::Close { code, message } => return Err(control_error(&code, &message)),
            _ => {}
        }
    }
}

fn next_control(socket: &mut Socket, deadline: Instant) -> Result<RelayFrame, CliError> {
    loop {
        if Instant::now() > deadline {
            return Err(CliError::Remote("timed out waiting for the relay to answer".to_owned()));
        }
        match read_incoming(socket)? {
            Incoming::Idle => continue,
            Incoming::Closed => {
                return Err(CliError::Remote(
                    "the relay closed the connection during the handshake".to_owned(),
                ));
            }
            Incoming::Control(frame) => return Ok(*frame),
            Incoming::Data(_) => {
                return Err(CliError::Remote(
                    "the relay sent encrypted data before the channel was open".to_owned(),
                ));
            }
        }
    }
}

fn next_remote(
    socket: &mut Socket,
    cipher: &mut vowp_agent::relay::ChannelCipher,
    deadline: Instant,
) -> Result<RemoteFrame, CliError> {
    loop {
        if Instant::now() > deadline {
            return Err(CliError::Remote(
                "timed out waiting for the machine to answer".to_owned(),
            ));
        }
        match read_incoming(socket)? {
            Incoming::Idle => continue,
            Incoming::Closed => {
                return Err(CliError::Remote(
                    "the relay closed the connection during the handshake".to_owned(),
                ));
            }
            Incoming::Control(frame) => match *frame {
                RelayFrame::Ping { nonce, .. } => send_pong(socket, nonce)?,
                RelayFrame::Error { code, message, .. } | RelayFrame::ChannelClose { code, message, .. } => {
                    return Err(control_error(&code, &message));
                }
                RelayFrame::ChannelReject { code, message, .. } => return Err(control_error(&code, &message)),
                _ => {}
            },
            Incoming::Data(frame) => {
                return decrypt(cipher, &frame);
            }
        }
    }
}

fn decrypt(cipher: &mut vowp_agent::relay::ChannelCipher, frame: &RelayDataFrame) -> Result<RemoteFrame, CliError> {
    let plain = cipher
        .open(frame)
        .map_err(|error| CliError::Auth(format!("the relay channel failed authentication: {error}")))?;
    RemoteFrame::from_json(&plain)
        .map_err(|error| CliError::Remote(format!("the machine sent a bad remote frame: {error}")))
}

fn send_pong(socket: &mut Socket, nonce: String) -> Result<(), CliError> {
    send_control(
        socket,
        &RelayFrame::Pong {
            v: RELAY_FRAME_VERSION,
            nonce,
        },
    )
}

/// True for the codes that mean "the channel ended normally".
fn is_normal_close(code: &str) -> bool {
    matches!(code, "" | "normal" | "ok" | "closed")
}

fn control_error(code: &str, message: &str) -> CliError {
    match code {
        codes::UNAUTHENTICATED
        | codes::TICKET_REPLAYED
        | codes::TICKET_EXPIRED
        | codes::TICKET_INVALID
        | codes::SCOPE_DENIED
        | codes::REVOKED => CliError::Auth(format!("the relay refused the channel ({code}): {message}")),
        codes::SUPERSEDED => CliError::Conflict(format!("the relay channel was superseded ({code}): {message}")),
        codes::OVERLOADED => CliError::Limited(format!("the relay is overloaded ({code}): {message}")),
        _ => CliError::Remote(format!("the relay failed ({code}): {message}")),
    }
}

fn stream_error(code: &str, message: &str, retryable: bool) -> CliError {
    let suffix = if retryable { " (retryable)" } else { "" };
    match code {
        codes::SCOPE_DENIED => CliError::Auth(format!("the machine denied the ssh stream ({code}): {message}{suffix}")),
        "ssh_unavailable" => CliError::Remote(format!("the machine has no SSH server available: {message}{suffix}")),
        _ => CliError::Remote(format!(
            "the machine refused the ssh stream ({code}): {message}{suffix}"
        )),
    }
}

/// Copies bytes between stdin/stdout and the encrypted TCP stream.
fn bridge(socket: &mut Socket, cipher: &mut vowp_agent::relay::ChannelCipher) -> Result<i32, CliError> {
    let (sender, receiver) = mpsc::sync_channel::<Option<Vec<u8>>>(8);
    std::thread::spawn(move || {
        let mut stdin = std::io::stdin();
        let mut buffer = vec![0u8; STDIN_CHUNK];
        loop {
            match stdin.read(&mut buffer) {
                Ok(0) => break,
                Ok(read) => {
                    if sender.send(Some(buffer[..read].to_vec())).is_err() {
                        return;
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => break,
            }
        }
        let _ = sender.send(None);
    });

    let mut stdout = std::io::stdout();
    let mut stdin_closed = false;
    let mut eof_at: Option<Instant> = None;
    loop {
        loop {
            match receiver.try_recv() {
                Ok(Some(chunk)) => {
                    send_remote(
                        socket,
                        cipher,
                        &RemoteFrame::TcpData {
                            stream: SSH_STREAM,
                            data: STANDARD.encode(chunk),
                        },
                    )?;
                }
                Ok(None) => {
                    send_remote(socket, cipher, &RemoteFrame::TcpClose { stream: SSH_STREAM })?;
                    stdin_closed = true;
                    eof_at = Some(Instant::now());
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    if !stdin_closed {
                        send_remote(socket, cipher, &RemoteFrame::TcpClose { stream: SSH_STREAM })?;
                        stdin_closed = true;
                        eof_at = Some(Instant::now());
                    }
                    break;
                }
            }
        }
        if let Some(at) = eof_at
            && at.elapsed() > EOF_GRACE
        {
            return Ok(0);
        }

        match read_incoming(socket)? {
            Incoming::Idle => continue,
            Incoming::Closed => return Ok(0),
            Incoming::Control(frame) => match *frame {
                RelayFrame::Ping { nonce, .. } => send_pong(socket, nonce)?,
                RelayFrame::ChannelClose { code, message, .. } | RelayFrame::Error { code, message, .. } => {
                    return if is_normal_close(&code) {
                        Ok(0)
                    } else {
                        Err(control_error(&code, &message))
                    };
                }
                _ => {}
            },
            Incoming::Data(frame) => match decrypt(cipher, &frame)? {
                RemoteFrame::TcpData { data, .. } => {
                    let bytes = STANDARD
                        .decode(data.as_bytes())
                        .map_err(|_| CliError::Remote("the machine sent malformed SSH bytes".to_owned()))?;
                    match stdout.write_all(&bytes).and_then(|()| stdout.flush()) {
                        Ok(()) => {}
                        Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => return Ok(0),
                        Err(error) => return Err(CliError::Io(error)),
                    }
                }
                RemoteFrame::TcpClose { .. } => return Ok(0),
                RemoteFrame::Ping { nonce } => {
                    send_remote(socket, cipher, &RemoteFrame::Pong { nonce: nonce.clone() })?
                }
                RemoteFrame::StreamError {
                    code,
                    message,
                    retryable,
                    ..
                } => return Err(stream_error(&code, &message, retryable)),
                RemoteFrame::Close { code, message } => {
                    return if is_normal_close(&code) {
                        Ok(0)
                    } else {
                        Err(control_error(&code, &message))
                    };
                }
                _ => {}
            },
        }
        if stdin_closed && eof_at.is_none() {
            eof_at = Some(Instant::now());
        }
    }
}
