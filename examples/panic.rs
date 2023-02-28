use std::time::Duration;

use ruactor::{props_from_clone, Actor, ActorError, ActorSystem};

#[derive(Clone)]
struct Hello {}

enum HelloMessage {
    Echo,
    Panic,
}

impl Actor for Hello {
    type Message = HelloMessage;

    fn on_enter(&mut self, _context: &mut ruactor::ActorContext<Self::Message>) {
        println!("enter hello");
    }
    fn on_message(
        &mut self,
        _context: &mut ruactor::ActorContext<Self::Message>,
        message: Self::Message,
    ) {
        match message {
            HelloMessage::Echo => println!("echo!!"),
            HelloMessage::Panic => panic!("panic"),
        }
    }

    fn on_exit(&mut self, _context: &mut ruactor::ActorContext<Self::Message>) {
        println!("exit hello");
    }
}

#[tokio::main]
async fn main() -> Result<(), ActorError> {
    let system = ActorSystem::new("test");

    let actor_ref = system.create_actor("test_actor", props_from_clone(Hello {}))?;

    actor_ref.tell(HelloMessage::Echo);
    actor_ref.tell(HelloMessage::Panic);
    actor_ref.tell(HelloMessage::Echo);
    actor_ref.tell(HelloMessage::Panic);

    loop {
        tokio::time::sleep(Duration::from_secs(1)).await;
        println!("send echo");
        actor_ref.tell(HelloMessage::Echo);
    }
}
