use std::time::Duration;

use ruactor::{props_from_clone, Actor, ActorError, ActorSystem};

#[derive(Clone)]
struct A1 {}

impl Drop for A1 {
    fn drop(&mut self) {
        println!("drop A1");
    }
}

impl Actor for A1 {
    type Message = String;

    fn on_message(
        &mut self,
        _context: &mut ruactor::ActorContext<Self::Message>,
        _message: Self::Message,
    ) {
        todo!()
    }
}

#[tokio::test]
async fn drop_test() -> Result<(), ActorError> {
    let asys = ActorSystem::new("test");
    let _actor_ref = asys.create_actor("main", props_from_clone(A1 {}))?;

    drop(asys);

    tokio::time::sleep(Duration::from_secs(1)).await;

    // drop(actor_ref);
    // tokio::time::sleep(Duration::from_secs(1)).await;
    println!("after drop actor_ref");

    Ok(())
}
