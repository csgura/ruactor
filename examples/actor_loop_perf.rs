use std::time::{Duration, Instant};

use crossbeam::queue::SegQueue;
use ruactor::{ask, props_from_clone, Actor, ActorSystem, Props, ReplyTo};
use tokio::task::JoinSet;

enum TestMessage {
    Hello(u32),
    World(String, ReplyTo<()>),
}

#[derive(Clone)]
struct TestActor {}

impl Actor for TestActor {
    type Message = TestMessage;

    fn on_message(
        &mut self,
        _context: &mut ruactor::ActorContext<Self::Message>,
        message: Self::Message,
    ) {
        match message {
            TestMessage::World(_str, reply_to) => {
                let _ = reply_to.send(());
            }
            _ => {}
        }
    }
}

#[derive(Clone)]
struct MainActor {
    bounded: bool,
}

impl Actor for MainActor {
    type Message = TestMessage;

    fn on_message(
        &mut self,
        context: &mut ruactor::ActorContext<Self::Message>,
        message: Self::Message,
    ) {
        match message {
            TestMessage::World(str, reply_to) => {
                if self.bounded {
                    let child = context.get_or_create_child(
                        str.clone(),
                        props_from_clone(TestActor {})
                            .with_throughput(2000000)
                            .with_bounded_queue(1000000),
                    );
                    child.tell(TestMessage::World(str, reply_to));
                } else {
                    let child = context.get_or_create_child(
                        str.clone(),
                        props_from_clone(TestActor {}).with_throughput(2000000),
                    );
                    child.tell(TestMessage::World(str, reply_to));
                }
            }
            _ => {}
        }
    }
}

#[tokio::main]
async fn main() {
    let queue = SegQueue::<TestMessage>::new();

    let count = 1000000;

    // push
    let start = Instant::now();
    for _ in 0..count {
        queue.push(TestMessage::Hello(10))
    }
    let end = Instant::now();

    let elapsed = end - start;
    println!(
        "seg queue push : elapsed = {:?}, tps =  {}",
        elapsed,
        count as f64 / elapsed.as_secs_f64()
    );

    // pop
    let start = Instant::now();

    while let Some(_mess) = queue.pop() {}
    let end = Instant::now();

    let elapsed = end - start;
    println!(
        "seg queue pop : elapsed = {:?}, tps =  {}",
        elapsed,
        count as f64 / elapsed.as_secs_f64()
    );

    let asys = ActorSystem::new("app");

    let actor_ref = asys
        .create_actor("test", props_from_clone(TestActor {}))
        .expect("failed");

    // bulk actor loop
    let mut bulk = Vec::new();

    for _ in 0..count {
        bulk.push(TestMessage::Hello(10))
    }

    let start = Instant::now();

    ruactor::benchmark_actor_loop(actor_ref.clone(), bulk).await;
    let end = Instant::now();

    let elapsed = end - start;

    println!(
        "actor loop : elapsed = {:?}, tps =  {}",
        elapsed,
        count as f64 / elapsed.as_secs_f64()
    );

    // bulk actor loop wait
    let mut bulk = Vec::new();
    let mut wait = Vec::new();

    for _ in 0..count {
        let ch = tokio::sync::oneshot::channel();
        bulk.push(TestMessage::World("hello".into(), ch.0));
        wait.push(ch.1);
    }

    let start = Instant::now();

    ruactor::benchmark_actor_loop(actor_ref, bulk).await;

    for h in wait {
        let _ = h.await;
    }

    let end = Instant::now();

    let elapsed = end - start;

    println!(
        "actor loop ask : elapsed = {:?}, tps =  {}",
        elapsed,
        count as f64 / elapsed.as_secs_f64()
    );

    let actor_ref = asys
        .create_actor("main", props_from_clone(MainActor { bounded: false }))
        .expect("failed");

    // main actor wait
    let mut bulk = Vec::new();
    let mut wait = Vec::new();

    for _ in 0..count {
        let ch = tokio::sync::oneshot::channel();
        bulk.push(TestMessage::World("hello".into(), ch.0));
        wait.push(ch.1);
    }

    let start = Instant::now();

    ruactor::benchmark_actor_loop(actor_ref.clone(), bulk).await;

    for h in wait {
        let _ = h.await.expect("no answer");
    }

    let end = Instant::now();

    let elapsed = end - start;

    println!(
        "actor loop main ask : elapsed = {:?}, tps =  {}",
        elapsed,
        count as f64 / elapsed.as_secs_f64()
    );

    // main actor send/recv
    let mut wait = Vec::new();

    let start = Instant::now();

    for _ in 0..count {
        let ch = tokio::sync::oneshot::channel();
        wait.push(ch.1);
        actor_ref.tell(TestMessage::World("hello".into(), ch.0));
    }

    for h in wait {
        let _ = h.await.expect("no answer");
    }

    let end = Instant::now();

    let elapsed = end - start;

    println!(
        "actor loop main send/recv : elapsed = {:?}, tps =  {}",
        elapsed,
        count as f64 / elapsed.as_secs_f64()
    );

    // dedicated send/recv
    let actor_ref = asys
        .create_actor(
            "dedi",
            props_from_clone(MainActor { bounded: false }).with_dedicated_thread(3),
        )
        .expect("failed");

    let mut wait = Vec::new();

    let start = Instant::now();

    for _ in 0..count {
        let ch = tokio::sync::oneshot::channel();
        wait.push(ch.1);
        actor_ref.tell(TestMessage::World("hello".into(), ch.0));
    }

    for h in wait {
        let _ = h.await.expect("no answer");
    }

    let end = Instant::now();

    let elapsed = end - start;

    println!(
        "actor loop dedi send/recv : elapsed = {:?}, tps =  {}",
        elapsed,
        count as f64 / elapsed.as_secs_f64()
    );

    // dedicated send/recv
    let actor_ref = asys
        .create_actor(
            "bounded",
            props_from_clone(MainActor { bounded: true })
                .with_dedicated_thread(3)
                .with_bounded_queue(1000000),
        )
        .expect("failed");

    let mut wait = Vec::new();

    let start = Instant::now();

    for _ in 0..count {
        let ch = tokio::sync::oneshot::channel();
        wait.push(ch.1);
        actor_ref.tell(TestMessage::World("hello".into(), ch.0));
    }

    for h in wait {
        let _ = h.await.expect("no answer");
    }

    let end = Instant::now();

    let elapsed = end - start;

    println!(
        "actor loop dedi bounded send/recv : elapsed = {:?}, tps =  {}",
        elapsed,
        count as f64 / elapsed.as_secs_f64()
    );

    let start = Instant::now();

    let mut js = JoinSet::new();
    let num_cli = 100;
    let count = 10000;
    for _ in 0..num_cli {
        let ac = actor_ref.clone();
        js.spawn(async move {
            for _ in 0..count {
                let _ = ask!(
                    ac,
                    TestMessage::World("hello".into(), _),
                    Duration::from_secs(5)
                );
            }
        });
    }

    while let Some(_) = js.join_next().await {}

    let end = Instant::now();

    let elapsed = end - start;

    println!(
        "actor ask dedi bounded send/recv : elapsed = {:?}, tps =  {}",
        elapsed,
        num_cli as f64 * count as f64 / elapsed.as_secs_f64()
    );
}
