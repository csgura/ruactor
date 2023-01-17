use std::time::Duration;

use ruactor::*;

#[derive(Clone)]
struct Hello {
    counter: u32,
}

struct Child {
    counter: u32,
}

impl Actor for Child {
    type UserMessageType = TestMessage;

    fn on_message(
        &self,
        context: &mut Context<Self::UserMessageType>,
        message: Self::UserMessageType,
    ) {
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
    type UserMessageType = TestMessage;

    fn on_message(
        &self,
        context: &mut Context<Self::UserMessageType>,
        message: Self::UserMessageType,
    ) {
        println!("receive message {:?}", message);
        match message {
            TestMessage::Hello(key) => {
                let child = context.get_or_create_child(key.clone(), || Child { counter: 0 });
                child.tell(TestMessage::Hello(key));
            }
            _ => println!("ignore"),
        }
    }

    fn on_enter(&self, context: &mut Context<Self::UserMessageType>) {
        println!("start timer");

        context.start_single_timer(
            "alarm".into(),
            Duration::from_secs(1),
            TestMessage::Timer("timer".into()),
        );
    }
}
#[derive(Clone, Debug)]
enum TestMessage {
    Hello(String),
    Timer(String),
}

#[tokio::main]
async fn main() {
    let system = ActorSystem::new("test");

    let actor_ref = system
        .create_actor("test-actor", || Hello { counter: 0 })
        .await
        .unwrap();

    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    let msg_a = TestMessage::Hello("hello world!".to_string());
    actor_ref.tell(msg_a);

    let msg_a = TestMessage::Hello("hello world!".to_string());
    actor_ref.tell(msg_a);

    let msg_a = TestMessage::Hello("hello world!".to_string());
    actor_ref.tell(msg_a);

    tokio::time::sleep(tokio::time::Duration::from_secs(1000)).await;

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
