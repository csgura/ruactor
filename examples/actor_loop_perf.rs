use std::time::Instant;

use crossbeam::queue::SegQueue;
use ruactor::{props_from_clone, Actor, ActorSystem};

enum TestMessage {
    Hello(u32),
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

    ruactor::benchmark_actor_loop(actor_ref, bulk).await;
    let end = Instant::now();

    let elapsed = end - start;

    println!(
        "actor loop : elapsed = {:?}, tps =  {}",
        elapsed,
        count as f64 / elapsed.as_secs_f64()
    );
}
