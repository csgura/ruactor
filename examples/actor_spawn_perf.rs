use std::time::{Duration, Instant};

use crossbeam::sync::WaitGroup;
use ruactor::{ask, props_from_clone, Actor, ActorSystem, Props, ReplyTo, SystemMessage};

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
        context.set_receive_timeout(Duration::from_secs(30))
    }
    fn on_message(
        &mut self,
        context: &mut ruactor::ActorContext<Self::Message>,
        message: Self::Message,
    ) {
        match message {
            TestMessage::World(str, reply_to) => {
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
            //tokio::time::sleep(Duration::from_millis(1)).await;
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

    fn on_enter(&mut self, _context: &mut ruactor::ActorContext<Self::Message>) {
        println!("main actor on {:?}", std::thread::current().name());
    }
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

#[allow(unused_variables)]
async fn bench() {
    // let handle = tokio::runtime::Handle::current();
    //let runtime_monitor = tokio_metrics::RuntimeMonitor::new(&handle);

    // let frequency = std::time::Duration::from_millis(500);
    // tokio::spawn(async move {
    //     for metrics in runtime_monitor.intervals() {
    //         println!("Metrics = {:?}", metrics);
    //         tokio::time::sleep(frequency).await;
    //     }
    // });

    let asys = ActorSystem::new("app");

    let actor_ref = asys
        .create_actor(
            "bounded",
            props_from_clone(MainActor {}).with_dedicated_thread(8), //.with_throughput(2000000),
        )
        .expect("failed");

    let start = Instant::now();

    let target = 1000000;
    let num_cli = 1000;

    // let ref_arr = (0..num_cli)
    //     .map(|i| {
    //         let name = format!("bounded-{}", i);
    //         asys.create_actor(&name, props_from_clone(MainActor {}))
    //             .expect("failed")
    //     })
    //     .collect::<Vec<_>>();

    let total_actor = 100;
    let num_message_per_actor = target / num_cli / total_actor;
    println!("num_message_per_actor = {}", num_message_per_actor);
    let wg = WaitGroup::new();
    for i in 0..num_cli {
        let ac = actor_ref.clone();
        //let ac = ref_arr[i].clone();

        let _client_id = i;
        let wg = wg.clone();
        tokio::spawn(async move {
            for _ in 0..num_message_per_actor {
                for j in 0..total_actor {
                    let key = format!("sess-{}", j);

                    let res = ask!(ac, TestMessage::World(key, _), Duration::from_secs(5));

                    if let Err(err) = res {
                        println!("err = {}", err);
                    }
                }
            }
            drop(wg);
        });
    }

    wg.wait();

    let end = Instant::now();

    let elapsed = end - start;

    let total_message = num_cli * total_actor * num_message_per_actor;
    println!(
        "actor ask : total = {}, elapsed = {:?}, tps =  {}",
        total_message,
        elapsed,
        total_message as f64 / elapsed.as_secs_f64()
    );

    // let metrics = runtime_monitor.intervals();
    // println!("Metrics = {:?}", metrics);

    //tokio::time::sleep(Duration::from_secs(1)).await;
}

#[tokio::main]
async fn main() {
    bench().await;
    println!("end bench");
}
