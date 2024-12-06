use lazy_static::lazy_static;
use ringbuf::{HeapRb, Rb};
use rustyline_async::{Readline, ReadlineEvent, SharedWriter};
use std::{sync::Arc, time::Duration};
use tokio::{
    sync::{oneshot, Mutex},
    time::{sleep, timeout},
};

pub struct Term {
    pub rl: Mutex<Readline>,
    pub out: Mutex<SharedWriter>,
}
impl Term {
    fn new() -> Self {
        let (mut rl, out) = Readline::new("\x1b[1m> \x1b[0m".to_owned()).unwrap();
        rl.should_print_line_on(false, false);
        Term {
            rl: Mutex::new(rl),
            out: Mutex::new(out),
        }
    }
}

lazy_static! {
    pub static ref TERM: Arc<Term> = Arc::new(Term::new());
    pub static ref TX_QUEUE: Arc<Mutex<HeapRb<oneshot::Sender<String>>>> =
        Arc::new(Mutex::new(HeapRb::new(128)));
    pub static ref IN_QUEUE: Arc<Mutex<HeapRb<String>>> = Arc::new(Mutex::new(HeapRb::new(128)));
}

macro_rules! println {
    ($fmt:expr $(, $arg:expr)*) => {{
        use std::io::Write;
        let mut lock = crate::term::TERM.out.lock().await;
        writeln!(&mut lock, $fmt, $($arg)*).unwrap();
        lock.flush().unwrap();
        drop(lock);
    }};
}

macro_rules! warn {
    ($fmt:expr $(, $arg:expr)*) => {{
        use std::io::Write;
        let mut lock = crate::term::TERM.out.lock().await;
        writeln!(&mut lock, "\x1b[92m{}\x1b[0m", format!($fmt $(,$arg)*)).unwrap();
        lock.flush().unwrap();
        drop(lock);
    }};
}

macro_rules! error {
    ($fmt:expr $(, $arg:expr)*) => {{
        use std::io::Write;
        let mut lock = crate::term::TERM.out.lock().await;
        writeln!(&mut lock, "\x1b[91m{}\x1b[0m", format!($fmt $(,$arg)*)).unwrap();
        lock.flush().unwrap();
        drop(lock);
    }};
}

macro_rules! readln {
    () => {{
        use ringbuf::Rb;
        use tokio::sync::oneshot;
        let mut lock = crate::term::IN_QUEUE.lock().await;
        if let Some(line) = lock.pop() {
            drop(lock);

            let (tx, rx) = oneshot::channel();
            tx.send(line).unwrap();
            rx
        } else {
            drop(lock);
            let mut lock = crate::term::TX_QUEUE.lock().await;
            let (tx, rx) = oneshot::channel();
            lock.push(tx).unwrap();
            drop(lock);
            rx
        }
    }};
}

pub async fn iobuf() -> ! {
    loop {
        let line = {
            let mut lock = crate::term::TERM.rl.lock().await;
            if let Ok(line) = timeout(Duration::from_millis(1), lock.readline()).await {
                if let Ok(line) = line {
                    if let ReadlineEvent::Line(line) = &line {
                        lock.add_history_entry(line.clone());
                    }
                    drop(lock);
                    line
                } else {
                    continue;
                }
            } else {
                drop(lock);
                sleep(Duration::from_millis(10)).await;
                continue;
            }
        };
        match line {
            ReadlineEvent::Line(line) => {
                let mut lock = crate::term::TX_QUEUE.lock().await;
                if let Some(tx) = lock.pop() {
                    drop(lock);
                    tx.send(line).unwrap();
                } else {
                    drop(lock);
                    let mut lock = crate::term::IN_QUEUE.lock().await;
                    lock.push(line).unwrap();
                    drop(lock);
                }
            }
            _ => std::process::exit(0),
        };
    }
}
