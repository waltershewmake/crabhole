static COMMANDS: &[&[u8]] = &[
    b"echo 'Hello, World!'",
    b"ls",
    b"vi demo.txt",
    b"iHello, World!\x1b",
    b"ccRaw mode works!\x1b",
    b"yy12p",
    b":wq",
    b"ls",
    b"echo \"there's our file\"",
    b"bat demo.txt",
];

use std::{env, os::fd::FromRawFd, time::Duration};

use nix::pty::openpty;
use tokio::{
    fs::File,
    io::{AsyncReadExt, AsyncWriteExt, BufReader, BufWriter},
    process::Command,
    select, spawn,
    time::sleep,
};
use tokio_util::sync::CancellationToken;

fn spawn_as_pty(control: CancellationToken) -> i32 {
    let default_shell = env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());

    match openpty(None, None) {
        Ok(pty) => {
            spawn(async move {
                unsafe {
                    let mut shell = Command::new(default_shell)
                        .pre_exec(move || -> std::io::Result<()> {
                            if libc::login_tty(pty.slave) != 0 {
                                return Err(std::io::Error::last_os_error());
                            }
                            Ok(())
                        })
                        .spawn()
                        .expect("failed to spawn shell");

                    // std::thread::sleep(Duration::from_secs(1));
                    select! {
                        _ = shell.wait() => {
                            println!("shell exited");
                        }
                        _ = control.cancelled() => {
                            shell.kill().await.expect("failed to kill shell");
                        }
                    }
                    // shell.kill().expect("failed to kill shell");
                    nix::unistd::close(pty.slave).expect("failed to close slave");
                }
            });
            return pty.master;
        }
        Err(e) => {
            panic!("Errorg: {}", e);
        }
    }
}

async fn read_pty(control: CancellationToken, mut pty: BufReader<File>) {
    let mut stdout = tokio::io::stdout();
    println!("\n");
    let mut buf = [0; 1024];
    loop {
        if control.is_cancelled() {
            break;
        }
        sleep(Duration::from_millis(100)).await;
        pty.flush().await.unwrap();
        let result = pty.read(&mut buf).await;
        match result {
            Ok(n) => {
                if n == 0 {
                    println!("big sads");
                    break;
                }
                stdout.write(&buf[..n]).await.unwrap();
                stdout.flush().await.unwrap();
            }
            Err(_e) => {
                break;
            }
        }
    }
}

#[tokio::main]
async fn main() {
    let control_token = CancellationToken::new();
    let kill_token = CancellationToken::new();
    let fd = spawn_as_pty(kill_token.clone());
    let pty = unsafe { File::from_raw_fd(fd) };

    let mut pty_in = BufWriter::new(pty.try_clone().await.unwrap());
    let pty_out = BufReader::new(pty);

    let moved_token = control_token.clone();
    spawn(read_pty(moved_token, pty_out));

    let mut cmds = COMMANDS.iter();
    loop {
        select! {
            _ = sleep(Duration::from_millis(2000)) => {
                if let Some(cmd) = cmds.next() {
                    for c in cmd.iter() {
                        pty_in.write_all([*c].as_ref()).await.unwrap();
                        pty_in.flush().await.unwrap();
                        sleep(Duration::from_millis(50)).await;
                    }
                    pty_in.write_all(b"\n").await.unwrap();
                    pty_in.flush().await.unwrap();
                } else {
                    sleep(Duration::from_millis(300)).await;
                    pty_in.write_all([4].as_ref()).await.unwrap();
                    pty_in.flush().await.unwrap();
                    kill_token.cancel();
                    // control_token.cancel();
                    break;
                }
            }
            _ = tokio::signal::ctrl_c() => {
                kill_token.cancel();
                pty_in.write_all([4].as_ref()).await.unwrap();
                pty_in.flush().await.unwrap();
                break;
            }
        }
    }
    sleep(Duration::from_secs(1)).await;
}
