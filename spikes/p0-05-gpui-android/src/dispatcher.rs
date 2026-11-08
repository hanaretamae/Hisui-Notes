//! GPUIの PlatformDispatcher のAndroid向け実装（検証用）。
//!
//! - メインスレッドのタスクはキューに積み、`AndroidAppWaker` でイベントループを起こす。
//! - バックグラウンドのタスクは固定数のスレッドで実行する。
//! - 時間指定のタスクはヒープに積み、イベントループが期限を確認する。

use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::thread::{self, ThreadId};
use std::time::{Duration, Instant};

use android_activity::AndroidAppWaker;
use gpui::{PlatformDispatcher, Priority, RunnableVariant};

struct Timer {
    due: Instant,
    seq: u64,
    runnable: RunnableVariant,
}
impl PartialEq for Timer {
    fn eq(&self, o: &Self) -> bool {
        (self.due, self.seq) == (o.due, o.seq)
    }
}
impl Eq for Timer {}
impl PartialOrd for Timer {
    fn partial_cmp(&self, o: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for Timer {
    fn cmp(&self, o: &Self) -> std::cmp::Ordering {
        (self.due, self.seq).cmp(&(o.due, o.seq))
    }
}

pub struct AndroidDispatcher {
    main_thread: ThreadId,
    main_tx: Mutex<Sender<RunnableVariant>>,
    main_rx: Mutex<Receiver<RunnableVariant>>,
    bg_tx: Mutex<Sender<RunnableVariant>>,
    timers: Mutex<(BinaryHeap<Reverse<Timer>>, u64)>,
    waker: AndroidAppWaker,
}

impl AndroidDispatcher {
    pub fn new(waker: AndroidAppWaker) -> Arc<Self> {
        let (main_tx, main_rx) = channel();
        let (bg_tx, bg_rx) = channel::<RunnableVariant>();
        let bg_rx = Arc::new(Mutex::new(bg_rx));
        let n = thread::available_parallelism().map(|n| n.get()).unwrap_or(4).clamp(2, 8);
        for i in 0..n {
            let rx = bg_rx.clone();
            thread::Builder::new()
                .name(format!("gpui-bg-{i}"))
                .spawn(move || {
                    loop {
                        let r = rx.lock().unwrap().recv();
                        match r {
                            Ok(runnable) => {
                                runnable.run();
                            }
                            Err(_) => break,
                        }
                    }
                })
                .unwrap();
        }
        Arc::new(Self {
            main_thread: thread::current().id(),
            main_tx: Mutex::new(main_tx),
            main_rx: Mutex::new(main_rx),
            bg_tx: Mutex::new(bg_tx),
            timers: Mutex::new((BinaryHeap::new(), 0)),
            waker,
        })
    }

    /// メインスレッドで呼ぶ。期限の来たタイマーと、キューのタスクをすべて実行する。
    /// 戻り値は次のタイマーまでの時間。
    pub fn run_main_tasks(&self) -> Option<Duration> {
        loop {
            let now = Instant::now();
            let due = {
                let mut t = self.timers.lock().unwrap();
                if t.0.peek().is_some_and(|Reverse(x)| x.due <= now) { t.0.pop().map(|Reverse(x)| x.runnable) } else { None }
            };
            match due {
                Some(r) => {
                    r.run();
                }
                None => break,
            }
        }
        loop {
            let r = self.main_rx.lock().unwrap().try_recv();
            match r {
                Ok(runnable) => {
                    runnable.run();
                }
                Err(_) => break,
            }
        }
        let t = self.timers.lock().unwrap();
        t.0.peek().map(|Reverse(x)| x.due.saturating_duration_since(Instant::now()))
    }
}

impl PlatformDispatcher for AndroidDispatcher {
    fn is_main_thread(&self) -> bool {
        thread::current().id() == self.main_thread
    }

    fn dispatch(&self, runnable: RunnableVariant, _priority: Priority) {
        let _ = self.bg_tx.lock().unwrap().send(runnable);
    }

    fn dispatch_on_main_thread(&self, runnable: RunnableVariant, _priority: Priority) {
        let _ = self.main_tx.lock().unwrap().send(runnable);
        self.waker.wake();
    }

    fn dispatch_after(&self, duration: Duration, runnable: RunnableVariant) {
        let mut t = self.timers.lock().unwrap();
        t.1 += 1;
        let seq = t.1;
        t.0.push(Reverse(Timer { due: Instant::now() + duration, seq, runnable }));
        drop(t);
        self.waker.wake();
    }

    fn spawn_realtime(&self, f: Box<dyn FnOnce() + Send>) {
        thread::spawn(f);
    }
}
