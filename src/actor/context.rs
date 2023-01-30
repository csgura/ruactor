use std::{
    collections::HashMap,
    marker::PhantomData,
    sync::Arc,
    time::{Duration, Instant},
};

use crate::{Actor, Prop};

use super::{
    dispatcher::TimerMessage, ActorRef, ChildContainer, Mailbox, Message, ParentRef, Timer,
};

pub struct ActorContext<T: 'static + Send> {
    pub(crate) self_ref: ActorRef<T>,
    pub(crate) actor: Option<Box<dyn Actor<Message = T>>>,
    pub(crate) cell: ActorCell<T>,
}

pub struct ActorCell<T: 'static + Send> {
    pub(crate) parent: Option<Box<dyn ParentRef>>,
    pub(crate) stash: Vec<T>,
    pub(crate) timer: Timer<T>,
    pub(crate) childrens: HashMap<String, ChildContainer>,
    pub(crate) receive_timeout: Option<Duration>,
    pub(crate) timer_gen: u32,
}

impl<T: 'static + Send> Default for ActorCell<T> {
    fn default() -> Self {
        Self {
            parent: Default::default(),
            stash: Default::default(),
            timer: Default::default(),
            childrens: Default::default(),
            receive_timeout: Default::default(),
            timer_gen: Default::default(),
        }
    }
}

impl<T: 'static + Send> ActorCell<T> {
    pub(crate) fn new(parent: Option<Box<dyn ParentRef>>) -> Self {
        Self {
            parent,
            ..Default::default()
        }
    }
}

impl<T: 'static + Send> ActorContext<T> {
    pub fn self_ref(&mut self) -> ActorRef<T> {
        self.self_ref.clone()
    }

    pub fn transit<A: Actor<Message = T> + 'static>(&mut self, new_actor: A) {
        self.actor = Some(Box::new(new_actor));
    }

    fn next_timer_gen(&mut self) -> u32 {
        let ret = self.cell.timer_gen;

        self.cell.timer_gen = match self.cell.timer_gen {
            u32::MAX => 0,
            _ => self.cell.timer_gen + 1,
        };

        ret
    }
    pub fn start_single_timer(&mut self, name: String, d: Duration, t: T) {
        let gen = self.next_timer_gen();

        self.cell.timer.list.insert(
            name.clone(),
            TimerMessage {
                gen: gen,
                phantom: PhantomData,
            },
        );

        let self_ref = self.self_ref.clone();
        tokio::spawn(async move {
            let s = tokio::time::sleep(d);
            s.await;

            self_ref.send(Message::Timer(name.clone(), 0, t));
        });
    }

    pub fn cancel_timer(&mut self, name: String) {
        self.cell.timer.list.remove(&name);
    }

    pub fn set_receive_timeout(&mut self, d: Duration) {
        self.cell.receive_timeout = Some(d);
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

    pub fn stash(&mut self, message: T) {
        self.cell.stash.push(message);
    }

    pub fn unstash_all(&mut self) {
        while let Some(msg) = self.cell.stash.pop() {
            self.self_ref.tell(msg);
        }
    }

    pub fn get_child<M: 'static + Send>(&self, name: String) -> Option<ActorRef<M>> {
        let ret = self
            .cell
            .childrens
            .get(&name)
            .and_then(|any| any.actor_ref.downcast_ref::<ActorRef<M>>().cloned());

        ret
    }

    pub fn get_or_create_child<A: Actor, P: Prop<A>>(
        &mut self,
        name: String,
        prop: P,
    ) -> ActorRef<A::Message> {
        let ret = self.get_child(name.clone());
        match ret {
            Some(actor_ref) => actor_ref,
            None => {
                let cpath = self.self_ref.path.clone() / name.clone();

                let mbox = Mailbox::new(prop, Some(Box::new(self.self_ref.clone())));

                let actor_ref = ActorRef::new(cpath, Arc::new(mbox));

                self.cell.childrens.insert(
                    name,
                    ChildContainer {
                        actor_ref: Box::new(actor_ref.clone()),
                        stop_ref: Box::new(actor_ref.clone()),
                    },
                );
                actor_ref
            }
        }
    }

    pub fn stop_self(&mut self) {
        self.self_ref.send(Message::Terminate);
    }
}
