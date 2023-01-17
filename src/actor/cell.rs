use std::alloc::System;
use std::collections::HashMap;
use std::mem::replace;
use std::sync::atomic::Ordering;
use std::time::Duration;

use super::Actor;
use super::ActorRef;
use super::Context;
use super::Message;
use crate::system::PropDyn;

pub struct TimerMessage<T: 'static> {
    pub(crate) msg: T,
    pub(crate) repeat: bool,
}

pub struct Timer<T: 'static> {
    pub(crate) list: HashMap<String, TimerMessage<T>>,
}

impl<T: 'static> Default for Timer<T> {
    fn default() -> Self {
        Self {
            list: Default::default(),
        }
    }
}

pub struct ActorCell<T: 'static + Send> {
    pub(crate) actor: Option<Box<dyn Actor<UserMessageType = T>>>,
    pub(crate) prop: Box<dyn PropDyn<T>>,
    pub(crate) ch: tokio::sync::mpsc::UnboundedReceiver<Message<T>>,
    pub(crate) stash: Vec<T>,
    pub(crate) timer: Timer<T>,
    pub(crate) receive_timeout: Option<Duration>,
}

impl<T: 'static + Send> ActorCell<T> {
    fn create_context(&mut self, self_ref: ActorRef<T>) -> Context<T> {
        let dt = Timer::default();

        Context {
            self_ref: self_ref,
            actor: None,
            stash: None,
            timer: replace(&mut self.timer, dt),
        }
    }

    fn drop_context(&mut self, self_ref: ActorRef<T>, context: Context<T>) {
        self.timer = context.timer;

        if let Some(mess) = context.stash {
            self.stash.push(mess);
        }

        if context.actor.is_some() {
            let old_actor = replace(&mut self.actor, context.actor);

            self.on_exit(old_actor, self_ref.clone());

            self.on_enter(self_ref.clone());
        }
    }

    fn on_exit(
        &mut self,
        old_actor: Option<Box<dyn Actor<UserMessageType = T>>>,
        self_ref: ActorRef<T>,
    ) {
        let mut context = self.create_context(self_ref.clone());

        if let Some(actor) = old_actor {
            actor.on_exit(&mut context);

            self.drop_context(self_ref, context);
        }
    }

    fn on_enter(&mut self, self_ref: ActorRef<T>) {
        let mut context = self.create_context(self_ref.clone());

        if let Some(actor) = &self.actor {
            actor.on_enter(&mut context);

            self.drop_context(self_ref, context);
        }
    }

    fn on_message(&mut self, self_ref: ActorRef<T>, message: Message<T>) {
        let mut context = self.create_context(self_ref.clone());

        if let Some(actor) = &self.actor {
            match message {
                Message::System(msg) => actor.on_system_message(&mut context, msg),
                Message::User(msg) => actor.on_message(&mut context, msg),
                Message::Timer(msg) => actor.on_message(&mut context, msg),
            }

            self.drop_context(self_ref, context);
        }
    }

    pub async fn actor_loop(&mut self, self_ref: ActorRef<T>) {
        if self.actor.is_none() {
            self.actor = Some(self.prop.create());

            self.on_enter(self_ref.clone());
        }

        let num_msg = self_ref.mbox.num_msg.clone();

        // let mut actor = cell.actor;
        // let mut ch = cell.ch;

        // let mut stash = cell.stash;
        if num_msg.load(Ordering::SeqCst) == 0 {
            return;
        }

        while let Some(msg) = self.ch.recv().await {
            num_msg.fetch_sub(1, Ordering::SeqCst);

            self.on_message(self_ref.clone(), msg);

            if num_msg.load(Ordering::SeqCst) == 0 {
                break;
            }
        }
    }
}
