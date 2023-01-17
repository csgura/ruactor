use std::{
    any::Any,
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

use crate::{Actor, Message, Prop};

use super::{ActorRef, Mailbox, SystemMessage, Timer};

pub struct Context<T: 'static + Send> {
    pub(crate) self_ref: ActorRef<T>,
    pub(crate) actor: Option<Box<dyn Actor<UserMessageType = T>>>,
    pub(crate) stash: Option<T>,
    pub(crate) timer: Timer<T>,
    pub(crate) childrens: HashMap<String, Box<dyn Any + Send + Sync + 'static>>,
    pub(crate) receive_timeout: Option<Duration>,
}

impl<T: 'static + Send> Context<T> {
    pub fn self_ref(&mut self) -> ActorRef<T> {
        self.self_ref.clone()
    }

    pub fn transit<A: Actor<UserMessageType = T> + 'static>(&mut self, new_actor: A) {
        self.actor = Some(Box::new(new_actor));
    }

    pub fn start_single_timer(&mut self, name: String, d: Duration, t: T) {
        let self_ref = self.self_ref.clone();
        tokio::spawn(async move {
            let s = tokio::time::sleep(d);
            s.await;

            self_ref.send(Message::Timer(t));
        });
    }

    pub fn cancel_timer(&mut self, name: String) {}

    pub fn set_receive_timeout(&mut self, d: Duration) {
        self.receive_timeout = Some(d);
        self.schedule_receive_timeout(d);
    }

    pub(crate) fn schedule_receive_timeout(&mut self, d: Duration) {
        let self_ref = self.self_ref.clone();

        let tmout = Instant::now() + d;

        tokio::spawn(async move {
            let s = tokio::time::sleep(d);
            s.await;

            self_ref.send(Message::ReceiveTimeout(tmout));
        });
    }

    pub fn stash(&mut self, message: Message<T>) {}

    pub fn unstash_all(&mut self) {}

    pub fn get_child<M: 'static + Send>(&self, name: String) -> Option<ActorRef<M>> {
        let ret = self
            .childrens
            .get(&name)
            .and_then(|any| any.downcast_ref::<ActorRef<M>>().cloned());

        ret
    }

    pub fn get_or_create_child<A: Actor, P: Prop<A>>(
        &mut self,
        name: String,
        prop: P,
    ) -> ActorRef<A::UserMessageType> {
        let ret = self.get_child(name.clone());
        match ret {
            Some(actor_ref) => actor_ref,
            None => {
                let cpath = self.self_ref.path.clone() / name.clone();

                let mbox = Mailbox::new(prop);

                let actor_ref = ActorRef::new(cpath, Arc::new(mbox));

                self.childrens.insert(name, Box::new(actor_ref.clone()));
                actor_ref
            }
        }
    }

    pub fn stop_self(&mut self) {
        self.self_ref.send(Message::PoisonPil);
    }
}
