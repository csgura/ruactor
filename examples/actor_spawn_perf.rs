use std::time::{Duration, Instant};

use crossbeam::sync::WaitGroup;
use ruactor::{ask, props_from_clone, Actor, ActorSystem, ReplyTo, SystemMessage};

#[allow(dead_code)]
enum TestMessage {
    World(String, ReplyTo<()>),
    Hello(String, ReplyTo<()>),
    Done,
}

#[derive(Clone)]
struct IdleState {}

impl Actor for IdleState {
    type Message = TestMessage;

    fn on_enter(&mut self, context: &mut ruactor::ActorContext<Self::Message>) {
        context.set_receive_timeout(Duration::from_secs(1))
    }
    fn on_message(
        &mut self,
        context: &mut ruactor::ActorContext<Self::Message>,
        message: Self::Message,
    ) {
        match message {
            TestMessage::World(str, reply_to) => {
                //let _ = reply_to.send(());

                // tokio::spawn(async move {
                //     let _ = reply_to.send(());
                // });
                context.transit(WorkingState {
                    str,
                    reply_to: Some(reply_to),
                });
            }
            TestMessage::Hello(_str, reply_to) => {
                //let _ = reply_to.send(());

                tokio::spawn(async move {
                    let _ = reply_to.send(());
                });
            }
            _ => {}
        }
    }

    fn on_exit(&mut self, context: &mut ruactor::ActorContext<Self::Message>) {
        context.cancel_receive_timeout()
    }

    fn on_system_message(
        &mut self,
        context: &mut ruactor::ActorContext<Self::Message>,
        message: ruactor::SystemMessage,
    ) {
        //println!("actor {} receive timeout ", context.self_ref());
        match message {
            SystemMessage::ReceiveTimeout => context.stop_self(),
        }
    }
}

#[allow(dead_code)]
struct WorkingState {
    str: String,
    reply_to: Option<ReplyTo<()>>,
}

impl Actor for WorkingState {
    type Message = TestMessage;

    fn on_enter(&mut self, context: &mut ruactor::ActorContext<Self::Message>) {
        let self_ref = context.self_ref();
        //self_ref.tell(TestMessage::Done);

        tokio::spawn(async move {
            self_ref.tell(TestMessage::Done);
        });
    }

    fn on_exit(&mut self, context: &mut ruactor::ActorContext<Self::Message>) {
        context.unstash_all();
    }
    fn on_message(
        &mut self,
        context: &mut ruactor::ActorContext<Self::Message>,
        message: Self::Message,
    ) {
        match message {
            TestMessage::Done => {
                let sender = self.reply_to.take().expect("must");
                let _ = sender.send(());
                context.transit(IdleState {})
            }
            TestMessage::World(_, _) => {
                context.stash(message);
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
                    context.get_or_create_child(str.clone(), props_from_clone(IdleState {}));
                child.tell(TestMessage::World(str, reply_to));
            }
            TestMessage::Hello(str, reply_to) => {
                let child =
                    context.get_or_create_child(str.clone(), props_from_clone(IdleState {}));
                child.tell(TestMessage::Hello(str, reply_to));
            }
            _ => {}
        }
    }
}

#[tokio::main]
async fn main() {
    let asys = ActorSystem::new("app");

    let actor_ref = asys
        .create_actor("bounded", props_from_clone(MainActor {}))
        .expect("failed");

    let start = Instant::now();

    let total_count = 1000000;
    let num_cli = 40;
    let count = total_count / num_cli;

    let wg = WaitGroup::new();

    for i in 0..num_cli {
        let ac = actor_ref.clone();
        let _client_id = i;
        let wg = wg.clone();
        tokio::spawn(async move {
            for j in 0..count {
                let key = format!("sess-{}", j);

                let res = ask!(ac, TestMessage::World(key, _), Duration::from_secs(5));

                if let Err(err) = res {
                    println!("err = {}", err);
                }
            }
            drop(wg);
        });
    }

    wg.wait();

    let end = Instant::now();

    let elapsed = end - start;

    println!(
        "actor ask : elapsed = {:?}, tps =  {}",
        elapsed,
        num_cli as f64 * count as f64 / elapsed.as_secs_f64()
    );

    //tokio::time::sleep(Duration::from_secs(2)).await;
}
