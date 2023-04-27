use std::time::{Duration, Instant};

use ruactor::{ask, props_from_clone, Actor, ActorSystem, Props, ReplyTo};
use tokio::task::JoinSet;

enum TestMessage {
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
        }
    }
}

#[tokio::main]
async fn main() {
    let asys = ActorSystem::new("app");

    let actor_ref = asys
        .create_actor("bounded", props_from_clone(MainActor { bounded: false }))
        .expect("failed");

    let start = Instant::now();

    let mut js = JoinSet::new();
    let num_cli = 1000;
    let count = 1000;
    for _ in 0..num_cli {
        let ac = actor_ref.clone();
        js.spawn(async move {
            for _ in 0..count {
                let res = ask!(
                    ac,
                    TestMessage::World("hello".into(), _),
                    Duration::from_secs(5)
                );

                if let Err(err) = res {
                    println!("err = {}", err);
                }
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
