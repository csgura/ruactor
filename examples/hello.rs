use std::time::Duration;

use ruactor::*;

#[derive(Clone)]
struct Hello {
    _counter: u32,
}

#[derive(Clone)]
struct Child {
    counter: u32,
}

#[derive(Clone)]
struct Grandson {}

impl Actor for Grandson {
    type Message = TestMessage;

    fn on_message(&mut self, _context: &mut ActorContext<Self::Message>, _message: Self::Message) {
        todo!()
    }

    fn on_exit(&mut self, _context: &mut ActorContext<Self::Message>) {
        println!("Grandson exit");
    }
}

impl Actor for Child {
    type Message = TestMessage;

    fn on_enter(&mut self, context: &mut ActorContext<Self::Message>) {
        context.get_or_create_child("cc".into(), props_from_clone(Grandson {}));

        context.set_receive_timeout(Duration::from_secs(1));
    }

    fn on_system_message(
        &mut self,
        context: &mut ActorContext<Self::Message>,
        message: SystemMessage,
    ) {
        match message {
            SystemMessage::ReceiveTimeout => {
                println!("{} receive tmout", context.self_ref());
                context.stop_self();
            }
        }
    }

    fn on_message(&mut self, context: &mut ActorContext<Self::Message>, message: Self::Message) {
        println!(
            "actor = {}, receive message at Child {:?} : {}",
            context.self_ref(),
            message,
            self.counter
        );
        context.transit(Child {
            counter: self.counter + 1,
        })
    }
}

impl Actor for Hello {
    type Message = TestMessage;

    fn on_message(&mut self, context: &mut ActorContext<Self::Message>, message: Self::Message) {
        println!("receive message {:?}", message);
        match message {
            TestMessage::Hello(key) => {
                let child = context
                    .get_or_create_child(key.clone(), props_from_clone(Child { counter: 0 }));
                child.tell(TestMessage::Hello(key));
            }
            TestMessage::Timer(tmr) => {
                println!("receive timer {}", tmr);
            }
            _ => {}
        }
    }

    fn on_enter(&mut self, context: &mut ActorContext<Self::Message>) {
        println!("start timer");

        context.start_single_timer(
            "alarm",
            Duration::from_secs(1),
            TestMessage::Timer("timer".into()),
        );
    }

    fn on_exit(&mut self, _context: &mut ActorContext<Self::Message>) {
        println!("hello on exit")
    }
}
#[derive(Debug)]
enum TestMessage {
    Hello(String),
    Timer(String),
    _Request(ReplyTo<String>, String),
}

#[tokio::main]
async fn main() {
    let system = ActorSystem::new("test");

    let actor_ref = system
        .create_actor(
            "test-actor",
            props_from_clone(Hello { _counter: 0 }).with_dedicated_thread(5),
        )
        .unwrap();

    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    let msg_a = TestMessage::Hello("hello world!".to_string());
    actor_ref.tell(msg_a);

    let msg_a = TestMessage::Hello("hello world!".to_string());
    actor_ref.tell(msg_a);

    let msg_a = TestMessage::Hello("hello world!".to_string());
    actor_ref.tell(msg_a);

    let msg_a = TestMessage::Hello("hi".to_string());
    actor_ref.tell(msg_a);

    // let ret = ask!(
    //     actor_ref,
    //     TestMessage::Request(_, "hello".into()),
    //     Duration::from_secs(10)
    // );

    tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;

    // let response_a = actor_ref.ask(msg_a).await.unwrap();
    // assert_eq!(response_a, "Ping!".to_string());

    // let msg_a = TestMessage("hello world!".to_string());
    // let response_a = actor_ref.ask(msg_a).await.unwrap();
    // assert_eq!(response_a, "Ping!".to_string());

    // let msg_a = TestMessage("hello world!".to_string());
    // let response_a = actor_ref.ask(msg_a).await.unwrap();
    // assert_eq!(response_a, "Ping!".to_string());

    // let msg_a = TestMessage("hello world!".to_string());
    // let response_a = actor_ref.ask(msg_a).await.unwrap();
    // assert_eq!(response_a, "Ping!".to_string());
}
