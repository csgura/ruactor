use ruactor::*;

#[derive(Clone, Debug)]
struct TestEvent(String);

struct Hello {
    counter: u32,
}

#[async_trait]
impl Actor for Hello {}

#[derive(Clone, Debug)]
struct TestMessage(String);

impl Message for TestMessage {
    type Response = String;
}

#[async_trait]
impl Handler<TestMessage> for Hello {
    async fn handle(&mut self, msg: TestMessage, ctx: &mut ActorContext) -> String {
        self.counter += 1;
        "Ping!".to_string()
    }
}

#[tokio::main]
async fn main() {
    let system = ActorSystem::new("test");

    let actor = Hello { counter: 0 };

    let actor_ref = system.create_actor("test-actor", actor).await.unwrap();

    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    let msg_a = TestMessage("hello world!".to_string());
    let response_a = actor_ref.ask(msg_a).await.unwrap();
    assert_eq!(response_a, "Ping!".to_string());
}
