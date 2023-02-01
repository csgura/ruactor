use std::time::Duration;

use ruactor::{ask, reply_to, Actor, ActorError, ActorSystem, PropClone, ReplyTo};

#[derive(Clone)]
struct IdleState;

#[derive(Debug)]
enum MyMessage {
    Hello(ReplyTo<String>, i32),
    Other,
    Done,
}

struct WorkingState {
    sender: Option<ReplyTo<String>>,
    seq: i32,
}

impl Actor for WorkingState {
    type Message = MyMessage;

    fn on_message(
        &mut self,
        context: &mut ruactor::ActorContext<Self::Message>,
        message: Self::Message,
    ) {
        println!("receive message at working state");
        match message {
            MyMessage::Done => {
                reply_to!(self.sender, "hello".into());
                context.transit(IdleState);
            }
            _ => context.stash(message),
        }
    }

    fn on_enter(&mut self, context: &mut ruactor::ActorContext<Self::Message>) {
        println!("become working state");
        let self_ref = context.self_ref().clone();

        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            self_ref.tell(MyMessage::Done);
        });
    }

    fn on_exit(&mut self, context: &mut ruactor::ActorContext<Self::Message>) {
        context.unstash_all();
    }
}
impl Actor for IdleState {
    type Message = MyMessage;

    fn on_message(
        &mut self,
        context: &mut ruactor::ActorContext<Self::Message>,
        message: Self::Message,
    ) {
        println!("receive message at idle state = {:?}", message);

        match message {
            MyMessage::Hello(sender, seq) => context.transit(WorkingState {
                sender: Some(sender),
                seq: seq,
            }),
            _ => {}
        }
    }

    fn on_enter(&mut self, context: &mut ruactor::ActorContext<Self::Message>) {
        println!("become idle state");
    }
}

#[tokio::test]
async fn unstash_test() -> Result<(), ActorError> {
    let asys = ActorSystem::new("test");
    let actor_ref = asys.create_actor("main", PropClone(IdleState))?;

    println!("hello??");
    let tmout = Duration::from_secs(10);

    //    let res = ask!(target: actor_ref, MyMessage::Hello(reply_to, 1), rx, tmout)?;

    let res = ask!(actor_ref, MyMessage::Hello(_, 1), tmout)?;
    let res = ask!(actor_ref, MyMessage::Hello(_, 2), tmout)?;
    actor_ref.tell(MyMessage::Other);
    let res = ask!(actor_ref, MyMessage::Hello(_, 3), tmout)?;

    Ok(())
}
