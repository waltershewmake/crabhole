#![feature(async_closure)]
#![feature(let_chains)]

const GLOBAL_TIMEOUT: u64 = 25;

use std::{process::Stdio, sync::Arc, time::Duration};

use futures_util::FutureExt;
use log::{debug, error, info, warn};
use rust_socketio::{
    asynchronous::{Client, ClientBuilder},
    Payload,
};
use serde_json::{json, Value};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter},
    process::{ChildStderr, ChildStdin, ChildStdout},
    spawn,
    sync::Mutex,
};
use tokio::{process::Command, select, signal, sync::RwLock, time::sleep};
use tokio_util::sync::CancellationToken;

macro_rules! client {
    ($address:expr, $state:expr, $($func:ident),+) => {{
        let mut client = ClientBuilder::new($address);
        $(
            let arc = $state.clone();
            client = client.on(stringify!($func), move |payload: Payload, socket: Client| $func(payload, socket, arc.to_owned()).boxed());
        )+
        client.connect()
        .await
        .expect("Connection failed")
    }};
}
macro_rules! wait {
    ($seconds:expr, $control:expr, $future:expr $(,$err:expr)?) => {
        select! {
            x = $future => x,
            _ = $control.cancelled() => {
                debug!("future ({}) aborted due to shutdown", stringify!($future));
                return $($err)?;
            }
            _ = sleep(Duration::from_millis($seconds)) => {
                warn!("future ({}) timed out", stringify!($future));
                return $($err)?;
            }
        }
    };
    ($control:expr, $future:expr $(,$err:expr)?) => {
        select! {
            x = $future => x,
            _ = $control.cancelled() => {
                debug!("future ({}) aborted due to shutdown", stringify!($future));
                return $($err)?;
            }
        }
    }
}

struct IO {
    stdin: Mutex<BufWriter<ChildStdin>>,
    stdout: Mutex<BufReader<ChildStdout>>,
    stderr: Mutex<BufReader<ChildStderr>>,
}

struct State {
    room_name: RwLock<Option<String>>,
    control: CancellationToken,
    stdio: IO,
    child_pid: u32,
}

async fn room(payload: Payload, _socket: Client, state: Arc<State>) {
    info!("room event received");
    match payload {
        Payload::Text(vec) => {
            let len = vec.len();
            if len != 1 {
                warn!("expected vec of 1 item, got {} item(s)", len);
                return;
            }
            let val = match &vec[0] {
                Value::String(s) => s.clone(),
                _ => {
                    warn!("expected String, got {:?}", vec[0]);
                    return;
                }
            };
            let len = val.len();
            if len < 5 || len > 30 {
                warn!("expected string length between 5 and 30, got len {}", len);
                return;
            }
            let mut lock = wait!(GLOBAL_TIMEOUT, state.control, state.room_name.write());
            println!("\x1b[33msetting room name to {}\x1b[0m", val);
            lock.replace(val);
        }
        x => {
            warn!("expected String(s), got {:?}", x)
        }
    }
}

async fn command(payload: Payload, _socket: Client, state: Arc<State>) {
    debug!("command event received");
    match payload {
        Payload::Text(vec) => {
            let len = vec.len();
            if len != 1 {
                warn!("expected vec of 1 item, got {} item(s)", len);
                return;
            }
            let mut val = match &vec[0] {
                Value::String(s) => s.clone(),
                _ => {
                    warn!("expected String, got {:?}", vec[0]);
                    return;
                }
            };
            if val.len() == 1 {
                match val.as_bytes()[0] {
                    3 => {
                        debug!("parsed ctrl-c, chutting down child process");
                        debug!("{}", state.child_pid);
                        let pid = state.child_pid;
                        if let Err(e) = wait!(
                            GLOBAL_TIMEOUT,
                            state.control,
                            Command::new("kill").arg("-2").arg(pid.to_string()).status()
                        ) {
                            error!("failed to kill child process: {}", e);
                            return;
                        }
                        return;
                    }
                    _ => {}
                }
            }
            val.push('\n');

            debug!("obtaining lock on stdin");
            let mut stdin = wait!(GLOBAL_TIMEOUT, state.control, state.stdio.stdin.lock());
            debug!("lock acquired");

            if let Err(e) = wait!(
                GLOBAL_TIMEOUT,
                state.control,
                stdin.write_all(val.as_bytes())
            ) {
                error!("failed to write to stdin: {}", e);
                return;
            }
            if let Err(e) = wait!(GLOBAL_TIMEOUT, state.control, stdin.flush()) {
                error!("failed to flush stdin: {}", e);
                return;
            }
        }
        x => {
            warn!("expected String(s), got {:?}", x)
        }
    }
}

async fn stream_output(state: Arc<State>, socket: Arc<Client>) {
    let mut stdout = wait!(GLOBAL_TIMEOUT, state.control, state.stdio.stdout.lock());
    let mut stderr = wait!(GLOBAL_TIMEOUT, state.control, state.stdio.stderr.lock());
    let mut outbuf = String::new();
    let mut errbuf = String::new();
    debug!("output stream starting");
    loop {
        // println!("{:?}", stdout);
        // sleep(Duration::from_secs(5)).await;
        select! {
            _ = state.control.cancelled() => {
                debug!("output stream loop ending due to shutdown");
                return
            }
            status = stdout.read_line(&mut outbuf) => match status {
                Ok(0) => {
                    info!("stdout stream ended, shutting down");
                    state.control.cancel();
                    return
                }
                Ok(_) => {
                    let room_name = wait!(GLOBAL_TIMEOUT, state.control, state.room_name.read());
                    if outbuf.ends_with('\n') {
                        outbuf.pop();
                    }
                    if let Err(e) = wait!(GLOBAL_TIMEOUT, state.control, socket.emit("response", json!({ "room": room_name.clone(), "response": outbuf }))) {
                        error!("failed to emit output event: {}", e);
                        return
                    }
                    outbuf.clear();
                }
                Err(e) => {
                    error!("failed to read from stdout: {}", e);
                    return
                }
            },
            status = stderr.read_line(&mut errbuf) => match status {
                Ok(0) => {
                    info!("stderr stream ended, shutting down");
                    state.control.cancel();
                    return
                }
                Ok(_) => {
                    if errbuf.ends_with('\n') {
                        errbuf.pop();
                    }
                    let room_name = wait!(GLOBAL_TIMEOUT, state.control, state.room_name.read());
                    if let Err(e) = wait!(GLOBAL_TIMEOUT, state.control, socket.emit("error", json!({ "room": room_name.clone(), "response": errbuf }))) {
                        error!("failed to emit output event: {}", e);
                        return
                    }
                    errbuf.clear();
                }
                _ => {
                    error!("failed to read from stderr");
                    return
                }
            }
        }
    }
}

#[tokio::main]
async fn main() {
    env_logger::init();

    let control = CancellationToken::new();

    let process = Command::new("sh")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::piped())
        .spawn()
        .expect("failed to spawn process");

    let pid = process.id().expect("failed to get process id");
    let stdin = process.stdin.expect("failed to open stdin");
    let stdout = process.stdout.expect("failed to open stdout");
    let stderr = process.stderr.expect("failed to open stderr");
    let stdin = BufWriter::new(stdin);
    let stdout = BufReader::new(stdout);
    let stderr = BufReader::new(stderr);

    let state = Arc::new(State {
        control: control.clone(),
        room_name: RwLock::new(None),
        child_pid: pid,
        stdio: IO {
            stdin: Mutex::new(stdin),
            stdout: Mutex::new(stdout),
            stderr: Mutex::new(stderr),
        },
    });

    let socket = Arc::new(client!(
        "https://crabhole-production.up.railway.app/",
        // "http://localhost:3000",
        state,
        room,
        command
    ));

    spawn(stream_output(state.clone(), socket.clone()));

    let result = wait!(
        GLOBAL_TIMEOUT,
        state.control,
        socket.emit("host", "quidquid latine dictum sit; altum sonatur")
    );

    if let Err(e) = result {
        error!("error connecting to server: {}", e);
    } else {
        select! {
            res = signal::ctrl_c() => {
                match res {
                    Ok(()) => {
                        debug!("received ctrl-c");
                        control.cancel()
                    }
                    Err(e) => {
                        error!("error listenening to ctrl-c events: {}", e);
                        control.cancel()
                    }
                }
            }
            _ = control.cancelled() => {
                debug!("main function received shutdown request");
            }
        }
    }

    #[allow(unreachable_code)]
    #[allow(unused_variables)]
    if let Err(e) = wait!(GLOBAL_TIMEOUT, state.control, socket.emit("error", json!({"room_name": state.room_name.try_read().unwrap_or(return).clone(), "response": "host disconnected"}))) {
        error!("failed to disconnect: {}", e);
    }

    wait!(GLOBAL_TIMEOUT, state.control, socket.disconnect()).expect("Failed to disconnect");
}
