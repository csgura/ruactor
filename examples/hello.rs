use std::time::Duration;

use ruactor::*;

#[derive(Clone)]
struct Hello {
    counter: u32,
}

impl Actor for Hello {
    type UserMessageType = TestMessage;

    fn on_message(
        &self,
        context: &mut Context<Self::UserMessageType>,
        message: Self::UserMessageType,
    ) {
        println!("receive message {:?}", message);
    }

    fn on_enter(&self, context: &mut Context<Self::UserMessageType>) {
        println!("start timer");

        context.start_single_timer(
            "alarm".into(),
            Duration::from_secs(1),
            TestMessage("timer".into()),
        );
    }
}
#[derive(Clone, Debug)]
struct TestMessage(String);

#[tokio::main]
async fn main() {
    let system = ActorSystem::new("test");

    let actor_ref = system
        .create_actor("test-actor", || Hello { counter: 0 })
        .await
        .unwrap();

    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    let msg_a = TestMessage("hello world!".to_string());
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
