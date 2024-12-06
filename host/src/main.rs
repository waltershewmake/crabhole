#![feature(async_closure)]
#![feature(let_chains)]

#[macro_use]
mod term;
use term::iobuf;

const GLOBAL_TIMEOUT: u64 = 25;

use std::{process::Stdio, sync::Arc, time::Duration};

use futures_util::FutureExt;
// use log::{debug, error, info, warn};
use lazy_static::lazy_static;
use regex::Regex;
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

lazy_static! {
    static ref RE: Regex =
        Regex::new(r"\x1b([\]\^_XP].+(\x1b\\|\x07)|\[[\d;]+[a-ln-zA-Z])").unwrap();
}

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
                // debug!("future ({}) aborted due to shutdown", stringify!($future));
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
                // debug!("future ({}) aborted due to shutdown", stringify!($future));
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
    // info!("room event received");
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
    // debug!("command event received");
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
                        // debug!("parsed ctrl-c, chutting down child process");
                        // debug!("{}", state.child_pid);
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
            // println!("\x1b[32m> {}\x1b[0m", val);
            val.push('\n');

            // debug!("obtaining lock on stdin");
            let mut stdin = wait!(GLOBAL_TIMEOUT, state.control, state.stdio.stdin.lock());
            // debug!("lock acquired");

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
    // debug!("output stream starting");
    loop {
        // println!("{:?}", stdout);
        // sleep(Duration::from_secs(5)).await;
        select! {
            _ = state.control.cancelled() => {
                // debug!("output stream loop ending due to shutdown");
                return
            }
            status = stdout.read_line(&mut outbuf) => match status {
                Ok(0) => {
                    // info!("stdout stream ended, shutting down");
                    state.control.cancel();
                    return
                }
                Ok(_) => {
                    let room_name = wait!(GLOBAL_TIMEOUT, state.control, state.room_name.read());
                    if outbuf.ends_with('\n') {
                        outbuf.pop();
                    }
                    println!("{}", outbuf.clone());
                    let parsed = RE.replace_all(&outbuf, "");
                    let parsed = ansi_to_html::convert(&parsed).unwrap_or(outbuf.clone());
                    if let Err(e) = wait!(GLOBAL_TIMEOUT, state.control, socket.emit("response", json!({ "room": room_name.clone(), "response": parsed }))) {
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
                    // info!("stderr stream ended, shutting down");
                    state.control.cancel();
                    return
                }
                Ok(_) => {
                    if errbuf.ends_with('\n') {
                        errbuf.pop();
                    }
                    // println!("\x1b[31m{}\x1b[0m", errbuf.clone());
                    println!("{}", errbuf.clone());
                    let parsed = RE.replace_all(&errbuf, "");
                    let parsed = ansi_to_html::convert(&parsed).unwrap_or(errbuf.clone());
                    let room_name = wait!(GLOBAL_TIMEOUT, state.control, state.room_name.read());
                    if let Err(e) = wait!(GLOBAL_TIMEOUT, state.control, socket.emit("error", json!({ "room": room_name.clone(), "response": parsed }))) {
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
    spawn(iobuf());
    env_logger::init();

    let control = CancellationToken::new();

    // let user_shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
    let user_shell = "/bin/bash";
    // let process = Command::new("sh")
    let process = Command::new("script")
        .arg("-q")
        .args(&["-c", &user_shell])
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
        // "https://crabhole-production.up.railway.app/",
        "http://localhost:3000",
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
        let mut reader = readln!();
        loop {
            let socket = socket.clone();
            let state = state.clone();
            select! {
                input = &mut reader => {
                    reader = readln!();
                    if input.is_err() {
                        continue;
                    }
                    spawn(async move {
                        // send 'buffer' to server as a 'command' event
                        // debug!("sending command event");
                        let x = wait!(GLOBAL_TIMEOUT, state.control, state.room_name.read());
                        if x.is_none() {
                            warn!("room name not set, ignoring command");
                            return;
                        }
                        let room_name = x.as_ref().unwrap();
                        if let Err(e) = wait!(
                            GLOBAL_TIMEOUT,
                            state.control,
                            socket.emit("command", json!({ "room": room_name, "command": input.unwrap() }))
                        ) {
                            error!("failed to emit command event: {}", e);
                            return;
                        }
                        // debug!("sent command event");
                    });
                },

                res = signal::ctrl_c() => {
                    match res {
                        Ok(()) => {
                            // debug!("received ctrl-c");
                            control.cancel();
                            break;
                        }
                        Err(e) => {
                            error!("error listenening to ctrl-c events: {}", e);
                            control.cancel();
                            break;
                        }
                    }
                }
                _ = control.cancelled() => {
                    // debug!("main function received shutdown request");
                    break;
                }
            }
        }
        // debug!("main function shutting down");
    }
    // debug!("main function shutting down");
    // if let Err(e) = wait!(GLOBAL_TIMEOUT, state.control, socket.emit("error", json!({"room_name": state.room_name.try_read()..clone(), "response": "host disconnected"}))) {
    //     error!("failed to disconnect: {}", e);
    // }
    let x = select! {
        _ = sleep(Duration::from_millis(GLOBAL_TIMEOUT)) => {
            error!("unable to notify server of shutdown");
            None
        }
        result = socket.emit(
            "error",
            json!({ "room": state.room_name.read().await.clone(), "response": "host disconnected" })
        ) => match result {
            Ok(_) => {
                Some(state.room_name.read().await.clone())
            }
            Err(_) => {
                None
            }
        }
    };

    // if x.is_some() {
    //     let room_name = x.as_ref().unwrap();
    //     select! {
    //         _ = sleep(Duration::from_millis(GLOBAL_TIMEOUT)) => {
    //             error!("unable to notify server of shutdown");
    //         }
    //         result = socket.emit(
    //             "error",
    //             json!({ "room": room_name, "response": "host disconnected" })
    //         ) => match result {
    //             Ok(_) => {
    //                 // debug!("notified server of shutdown");
    //             }
    //             Err(e) => {
    //                 warn!("failed to notify server of shutdown: {}", e);
    //             }
    //         }
    //     }
    // }

    select! {
        _ = sleep(Duration::from_millis(GLOBAL_TIMEOUT)) => {
            // debug!("unable to close socket");
        }
        _ = socket.disconnect() => {
            // debug!("socket closed");
        }
    }

    // debug!("killing child process");

    select! {
        _ = sleep(Duration::from_millis(5_000)) => {
            error!("unable to kill child process");
        }
        _ = Command::new("kill").arg("-2").arg(pid.to_string()).status() => {
            // debug!("child process killed");
        }
    }

    std::process::exit(0);
}
