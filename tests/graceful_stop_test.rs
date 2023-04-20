use std::{thread, time::Duration};

use ruactor::{props_from_clone, Actor, ActorError, ActorRef, ActorSystem, PropClone, Props};

#[derive(Clone)]
struct PPP;

#[allow(dead_code)]
enum RootMessage {
    Sleep,
}

#[derive(Clone)]
struct PP;

impl Actor for PP {
    type Message = RootMessage;

    fn on_enter(&mut self, context: &mut ruactor::ActorContext<Self::Message>) {
        context.create_child(PropClone(P));
        context.create_child(PropClone(P));
    }

    fn on_message(
        &mut self,
        _context: &mut ruactor::ActorContext<Self::Message>,
        _message: Self::Message,
    ) {
    }

    fn on_exit(&mut self, _context: &mut ruactor::ActorContext<Self::Message>) {
        println!("pp exit");
    }
}

#[derive(Clone)]
struct P;

impl Actor for P {
    type Message = RootMessage;

    fn on_enter(&mut self, context: &mut ruactor::ActorContext<Self::Message>) {
        context.create_child(PropClone(C));
        context.create_child(PropClone(C));
    }

    fn on_message(
        &mut self,
        _context: &mut ruactor::ActorContext<Self::Message>,
        _message: Self::Message,
    ) {
    }
    fn on_exit(&mut self, _context: &mut ruactor::ActorContext<Self::Message>) {
        println!("p exit");
    }
}

#[derive(Clone)]
struct C;

impl Actor for C {
    type Message = RootMessage;

    fn on_message(
        &mut self,
        _context: &mut ruactor::ActorContext<Self::Message>,
        _message: Self::Message,
    ) {
    }

    fn on_exit(&mut self, _context: &mut ruactor::ActorContext<Self::Message>) {
        println!("c exit");
    }
}

impl Actor for PPP {
    type Message = RootMessage;

    fn on_enter(&mut self, context: &mut ruactor::ActorContext<Self::Message>) {
        context.create_child(PropClone(PP));
        context.create_child(PropClone(PP));
    }
    fn on_message(
        &mut self,
        _context: &mut ruactor::ActorContext<Self::Message>,
        message: Self::Message,
    ) {
        match message {
            RootMessage::Sleep => {
                println!("sleep 1s");
                thread::sleep(Duration::from_secs(1));
            }
        }
    }

    fn on_exit(&mut self, _context: &mut ruactor::ActorContext<Self::Message>) {
        println!("ppp exit");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
//#[tokio::test]
async fn graceful_stop() -> Result<(), ActorError> {
    let asys = ActorSystem::new("test");
    let actor_ref = asys.create_actor(
        "main",
        props_from_clone(PPP)
            .with_graceful_stop()
            .with_dedicated_thread(4),
    )?;

    actor_ref.tell(RootMessage::Sleep);
    actor_ref.tell(RootMessage::Sleep);
    actor_ref.tell(RootMessage::Sleep);

    let ar: Option<ActorRef<RootMessage>> = asys.get_actor(actor_ref.path());

    assert!(ar.is_some());

    //actor_ref.tell(RootMessage::StopSelf);
    tokio::time::sleep(Duration::from_secs(1)).await;

    drop(asys);

    tokio::time::sleep(Duration::from_millis(10)).await;

    // println!("path = {}", actor_ref.path());
    // let ar: Option<ActorRef<RootMessage>> = asys.get_actor(actor_ref.path());

    // assert!(ar.is_none());

    Ok(())
}
