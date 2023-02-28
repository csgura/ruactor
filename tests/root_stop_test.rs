use std::time::Duration;

use ruactor::{props_from_clone, Actor, ActorError, ActorRef, ActorSystem};

#[derive(Clone)]
struct IdleState;

enum RootMessage {
    StopSelf,
}

impl Actor for IdleState {
    type Message = RootMessage;

    fn on_message(
        &mut self,
        context: &mut ruactor::ActorContext<Self::Message>,
        message: Self::Message,
    ) {
        match message {
            RootMessage::StopSelf => context.stop_self(),
        }
    }

    fn on_exit(&mut self, _context: &mut ruactor::ActorContext<Self::Message>) {
        println!("idlestate exit");
    }
}

impl Drop for IdleState {
    fn drop(&mut self) {
        println!("drop idlestate");
    }
}

#[tokio::test]
async fn root_actor_stop() -> Result<(), ActorError> {
    let asys = ActorSystem::new("test");
    let actor_ref = asys.create_actor("main", props_from_clone(IdleState))?;

    let ar: Option<ActorRef<RootMessage>> = asys.get_actor(actor_ref.path());

    assert!(ar.is_some());

    actor_ref.tell(RootMessage::StopSelf);

    tokio::time::sleep(Duration::from_millis(10)).await;

    println!("path = {}", actor_ref.path());
    let ar: Option<ActorRef<RootMessage>> = asys.get_actor(actor_ref.path());

    assert!(ar.is_none());

    Ok(())
}
