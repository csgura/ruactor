use std::time::Duration;

use crate::{Actor, Message};

use super::{ActorRef, Timer};

pub struct Context<T: 'static + Send> {
    pub(crate) self_ref: ActorRef<T>,
    pub(crate) actor: Option<Box<dyn Actor<UserMessageType = T>>>,
    pub(crate) stash: Option<T>,
    pub(crate) timer: Timer<T>,
}

impl<T: 'static + Send> Context<T> {
    pub fn transit<A: Actor<UserMessageType = T> + 'static>(&mut self, new_actor: A) {
        self.actor = Some(Box::new(new_actor));
    }

    pub fn start_single_timer(&mut self, d: Duration, t: T) {
        let self_ref = self.self_ref.clone();
        tokio::spawn(async move {
            let s = tokio::time::sleep(d);
            s.await;

            self_ref.send(Message::User(t));
        });
    }
}
