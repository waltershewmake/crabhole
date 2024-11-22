use nix::sys::signal::{self, Signal};
use nix::unistd::{setpgid, Pid};
use pty::{fork, Grantpt};
use std::os::unix::io::{FromRawFd, RawFd};
use std::process::{Command, Stdio};

fn main() -> std::io::Result<()> {
    // Create a PTY using the `pty` crate
    let (master, slave) = pty::fork().unwrap();

    match master {
        // Parent process (master)
        Pid::Parent => {
            // Set the process group ID for the parent (it'll be the leader of the group)
            setpgid(Pid::this(), Pid::this()).unwrap();

            // Set up a new command to spawn `bash`
            let mut child = Command::new("bash")
                .stdin(slave) // Connect to PTY slave
                .stdout(slave) // Connect to PTY slave
                .stderr(slave) // Connect to PTY slave
                .spawn()?;

            // Now you can interact with the child (bash) through the PTY
            // To send SIGINT to the child without terminating it
            signal::kill(child.id().into(), Signal::SIGINT).unwrap();

            // Optionally wait for the child process to exit
            let _ = child.wait()?;
        }

        // Child process (slave)
        Pid::Child => {
            // Execute `bash` in the child process
            let _ = Command::new("bash").spawn().expect("Failed to start bash");
        }
    }

    Ok(())
}
