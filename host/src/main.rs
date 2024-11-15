use std::{process::Stdio, sync::Arc, time::Duration};

use futures_util::FutureExt;
use log::{debug, error, info, warn};
use rust_socketio::{
    asynchronous::{Client, ClientBuilder},
    Payload,
};
use serde_json::{json, Value};
use tokio::io::{AsyncWriteExt, Stdin, Stdout};
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
    ($seconds:literal, $control:expr, $future:expr $(,$err:expr)?) => {
        select! {
            x = $future => x,
            _ = $control.cancelled() => {
                debug!("future ({}) aborted due to shutdown", stringify!($future));
                return $($err)?;
            }
            _ = sleep(Duration::from_secs($seconds)) => {
                warn!("future ({}) timed out", stringify!($future));
                return $($err)?;
            }
        }
    };
}

struct State {
    room_name: RwLock<Option<String>>,
    control: CancellationToken,
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
            let mut lock = wait!(5, state.control, state.room_name.write());
            info!("setting room name to {}", val);
            lock.replace(val);
        }
        x => {
            warn!("expected String(s), got {:?}", x)
        }
    }
}

async fn command(payload: Payload, socket: Client, state: Arc<State>) {
    debug!("command event received");
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
            info!("running command: {}", val);
            let val = val.trim().split(' ').collect::<Vec<&str>>();
            if val.len() < 1 {
                warn!("unable to run empty command");
                return;
            }
            let mut command = Command::new(val[0]);
            for arg in val.iter().skip(1) {
                command.arg(arg);
            }

            let output = wait!(10, state.control, command.output());
            let output = match output {
                Ok(output) => output,
                Err(e) => {
                    warn!("error running command: {}", e);
                    return;
                }
            };
            let lock = wait!(5, state.control, state.room_name.read());
            let room_name = lock.to_owned().unwrap();
            drop(lock);
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            if !stdout.is_empty() {
                let result = socket
                    .emit("response", json!({"response": stdout, "room": room_name}))
                    .await;
                if let Err(e) = result {
                    warn!("failed to send response: {}", e);
                    return;
                }
            }
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            if !stderr.is_empty() {
                let result = socket
                    .emit("error", json!({"error": stderr, "room": room_name}))
                    .await;
                if let Err(e) = result {
                    warn!("failed to send error: {}", e);
                    return;
                }
            }
        }
        x => {
            warn!("expected String(s), got {:?}", x)
        }
    }
}

#[tokio::main]
async fn main() {
    env_logger::init();

    let control = CancellationToken::new();

    let state = Arc::new(State {
        control: control.clone(),
        room_name: RwLock::new(None),
    });

    let socket = client!(
        // "https://crabhole-production.up.railway.app/",
        "http://localhost:3000",
        state,
        room,
        command
    );

    let result = socket
        .emit("host", "quidquid latine dictum sit; altum sonatur")
        .await;

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

    socket.disconnect().await.expect("Failed to disconnect");
}
