use std::time::Duration;

use ruactor::{ask, Actor, ActorError, ActorSystem, PropClone, ReplyTo};

enum TestMessage {
    Last {
        hello: String,
        other: i32,
        value: String,
        reply_to: ReplyTo<String>,
    },
}

#[derive(Clone)]
struct SomeActor;

impl Actor for SomeActor {
    type Message = TestMessage;

    fn on_message(
        &mut self,
        _context: &mut ruactor::ActorContext<Self::Message>,
        message: Self::Message,
    ) {
        match message {
            TestMessage::Last {
                hello,
                reply_to,
                other,
                value,
            } => {
                println!("{}, {}, {}", hello, other, value);
                let _ = reply_to.send("OK".into());
            }
        }
    }
}

#[tokio::test]
async fn macro_test() -> Result<(), ActorError> {
    let asys = ActorSystem::new("hello");
    let actor_ref = asys.create_actor("hello", PropClone(SomeActor)).unwrap();
    ask!(
        actor_ref,
        TestMessage::Last {
            reply_to: _,
            hello: "world".into(),
            other: 12,
            value: "kk".into(),
        },
        Duration::from_millis(100)
    )?;
    Ok(())
}
