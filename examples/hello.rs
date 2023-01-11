use ruactor::*;

#[derive(Clone, Debug)]
struct TestEvent(String);

#[derive(Clone)]
struct Hello<S> {
    counter: u32,
    state: S,
}

#[derive(Clone)]
struct Idle {}

#[derive(Clone)]
struct Running {}

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

#[async_trait]
impl<S: Sync + Send + 'static> Actor for Hello<S> {}

#[derive(Clone, Debug)]
struct TestMessage(String);

impl Message for TestMessage {
    type Response = String;
}

#[async_trait]
impl<S: Sync + Send + 'static + Handler<TestMessage>> Handler<TestMessage> for Hello<S> {
    async fn handle(&mut self, msg: TestMessage, ctx: &mut ActorContext) -> String {
        self.state.handle(msg, ctx);

        self.counter += 1;
        "Ping!".to_string()
    }
}

#[tokio::main]
async fn main() {
    let system = ActorSystem::new("test");

    let actor_ref = system
        .create_actor("test-actor", || Hello {
            counter: 0,
            state: Idle {},
        })
        .await
        .unwrap();

    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    // let msg_a = TestMessage("hello world!".to_string());
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
