use ruactor::*;

#[derive(Clone, Debug)]
struct TestEvent(String);

#[derive(Clone)]
struct Hello {
    counter: u32,
}

// enum State {
//     Idle,
//     Running,
// }
// #[async_trait]
// impl Handler<TestMessage> for State {
//     async fn handle(&mut self, msg: TestMessage, ctx: &mut ActorContext) -> String {
//         match self {
//             State::Idle => {
//                 println!("current state = idle");
//                 *self = State::Running
//             }
//             State::Running => {
//                 println!("current state = running");
//                 *self = State::Idle
//             }
//         }

//         "Ping!".to_string()
//     }
// }

use ruactor::Receive;

struct IdleState {}

#[async_trait]
impl Actor for Hello {
    type UserMessageType = TestMessage;

    fn create_receive(&mut self) -> Box<dyn Receive<TestMessage>> {
        ruactor::create_receive(|ctx, msg| println!("receive message in handle "))
    }
}

#[async_trait]
impl Receive<TestMessage> for IdleState {
    async fn receive(&self, ctx: &mut ActorContext<TestMessage>, msg: Message<TestMessage>) {}
}

#[derive(Clone, Debug)]
struct TestMessage(String);

// #[async_trait]
// impl<S: Sync + Send + 'static + Handler<TestMessage>> Handler<TestMessage> for Hello<S> {
//     async fn handle(&mut self, msg: TestMessage, ctx: &mut ActorContext) -> String {
//         self.state.handle(msg, ctx);

//         self.counter += 1;
//         "Ping!".to_string()
//     }
// }

#[tokio::main]
async fn main() {
    let system = ActorSystem::new("test");

    let actor_ref = system
        .create_actor("test-actor", || Hello { counter: 0 })
        .await
        .unwrap();

    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    let msg_a = TestMessage("hello world!".to_string());
    actor_ref.tell(Message::UserMessage(msg_a));

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
