use std::alloc::System;
use std::any::Any;
use std::collections::HashMap;
use std::hash::Hash;
use std::mem::replace;
use std::sync::atomic::Ordering;
use std::time::Duration;
use std::time::Instant;

use super::Actor;
use super::ActorRef;
use super::Context;
use super::Message;
use super::ParentRef;
use super::SystemMessage;
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
    pub(crate) parent: Option<Box<dyn ParentRef>>,
    pub(crate) actor: Option<Box<dyn Actor<UserMessageType = T>>>,
    pub(crate) prop: Box<dyn PropDyn<T>>,
    pub(crate) ch: tokio::sync::mpsc::UnboundedReceiver<Message<T>>,
    pub(crate) stash: Vec<T>,
    pub(crate) timer: Timer<T>,
    pub(crate) receive_timeout: Option<Duration>,
    pub(crate) childrens: HashMap<String, Box<dyn Any + Send + Sync + 'static>>,
    pub(crate) last_message_timestamp: Instant,
}

impl<T: 'static + Send> ActorCell<T> {
    fn create_context(&mut self, self_ref: ActorRef<T>) -> Context<T> {
        let dt = Timer::default();
        let dc = HashMap::new();

        let dp = None;
        Context {
            parent: replace(&mut self.parent, dp),
            self_ref: self_ref,
            actor: None,
            stash: None,
            timer: replace(&mut self.timer, dt),
            childrens: replace(&mut self.childrens, dc),
            receive_timeout: self.receive_timeout.clone(),
        }
    }

    fn drop_context(&mut self, self_ref: ActorRef<T>, context: Context<T>) {
        self.parent = context.parent;
        self.timer = context.timer;
        self.childrens = context.childrens;
        self.receive_timeout = context.receive_timeout;

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
                Message::System(msg) => {
                    self.last_message_timestamp = Instant::now();
                    actor.on_system_message(&mut context, msg)
                }
                Message::User(msg) => {
                    self.last_message_timestamp = Instant::now();

                    actor.on_message(&mut context, msg)
                }
                Message::Timer(msg) => {
                    self.last_message_timestamp = Instant::now();
                    actor.on_message(&mut context, msg)
                }
                Message::ReceiveTimeout(exp) => {
                    if let Some(tmout) = self.receive_timeout {
                        if exp > self.last_message_timestamp {
                            if exp - self.last_message_timestamp >= tmout {
                                actor.on_system_message(
                                    &mut context,
                                    super::SystemMessage::ReceiveTimeout,
                                );
                            } else {
                                let exp = (self.last_message_timestamp + tmout) - Instant::now();

                                context.schedule_receive_timeout(exp);
                            }
                        } else {
                            context.schedule_receive_timeout(tmout);
                        }
                    }
                }
                Message::Internal(super::InternalMessage::ChildTerminate(msg)) => {
                    println!("child terminated : {}", msg);
                    let key = msg.key();

                    context.childrens.remove(&key);
                    // println!("after children size =  {}", context.childrens.len());
                }
                Message::PoisonPil => {
                    // covered by actor loop
                }
            }
        }
        self.drop_context(self_ref, context);
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

            match msg {
                Message::PoisonPil => {
                    //println!("stop actor {}", self_ref);
                    self.ch.close();

                    break;
                }
                _ => {
                    self.on_message(self_ref.clone(), msg);
                }
            }

            if num_msg.load(Ordering::SeqCst) == 0 {
                break;
            }
        }
    }
}
