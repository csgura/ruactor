use std::time::Instant;

use crossbeam::queue::SegQueue;
use ruactor::{props_from_clone, Actor, ActorSystem, ReplyTo};

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
        context: &mut ruactor::ActorContext<Self::Message>,
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
struct MainActor {}

impl Actor for MainActor {
    type Message = TestMessage;

    fn on_message(
        &mut self,
        context: &mut ruactor::ActorContext<Self::Message>,
        message: Self::Message,
    ) {
        match message {
            TestMessage::World(str, reply_to) => {
                let child =
                    context.get_or_create_child(str.clone(), props_from_clone(TestActor {}));
                child.tell(TestMessage::World(str, reply_to));
            }
            _ => {}
        }
    }
}

#[tokio::main]
async fn main() {
    let queue = SegQueue::<TestMessage>::new();

    let count = 1000000;

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

    let start = Instant::now();

    while let Some(mess) = queue.pop() {}
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

    let mut bulk = Vec::new();
    let mut wait = Vec::new();

    for _ in 0..count {
        let ch = tokio::sync::oneshot::channel();
        bulk.push(TestMessage::World("hello".into(), ch.0));
        wait.push(ch.1);
    }

    let actor_ref = asys
        .create_actor("main", props_from_clone(MainActor {}))
        .expect("failed");

    let start = Instant::now();

    ruactor::benchmark_actor_loop(actor_ref.clone(), bulk).await;

    for h in wait {
        let _ = h.await;
    }

    let end = Instant::now();

    let elapsed = end - start;

    println!(
        "actor loop main ask : elapsed = {:?}, tps =  {}",
        elapsed,
        count as f64 / elapsed.as_secs_f64()
    );

    let mut wait = Vec::new();

    let start = Instant::now();

    for _ in 0..count {
        let ch = tokio::sync::oneshot::channel();
        wait.push(ch.1);
        actor_ref.tell(TestMessage::World("hello".into(), ch.0));
    }

    for h in wait {
        let _ = h.await;
    }

    let end = Instant::now();

    let elapsed = end - start;

    println!(
        "actor loop main send/recv : elapsed = {:?}, tps =  {}",
        elapsed,
        count as f64 / elapsed.as_secs_f64()
    );
}
