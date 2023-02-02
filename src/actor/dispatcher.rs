use std::collections::HashMap;

use std::marker::PhantomData;
use std::mem::replace;
use std::time::Instant;

use super::context::ActorCell;
use super::context::ActorContext;
use super::Actor;
use super::ActorRef;

use super::InternalMessage;
use super::Message;
use crate::system::PropDyn;

pub struct TimerMessage<T: 'static> {
    pub(crate) gen: u32,
    pub(crate) phantom: PhantomData<T>,
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

pub struct Dispatcher<T: 'static + Send> {
    pub(crate) actor: Option<Box<dyn Actor<Message = T>>>,
    pub(crate) prop: Box<dyn PropDyn<T>>,
    pub(crate) last_message_timestamp: Instant,
    pub(crate) cell: ActorCell<T>,
}

impl<T: 'static + Send> Dispatcher<T> {
    fn create_context(&mut self, self_ref: &ActorRef<T>) -> ActorContext<T> {
        let dc = ActorCell::default();

        ActorContext {
            self_ref: self_ref.clone(),
            actor: None,
            cell: replace(&mut self.cell, dc),
        }
    }

    fn drop_context(&mut self, self_ref: &ActorRef<T>, context: ActorContext<T>) {
        self.cell = context.cell;

        if context.actor.is_some() {
            let old_actor = replace(&mut self.actor, context.actor);

            self.on_exit(old_actor, self_ref);

            self.on_enter(self_ref);
        }
    }

    fn on_exit(&mut self, old_actor: Option<Box<dyn Actor<Message = T>>>, self_ref: &ActorRef<T>) {
        let mut context = self.create_context(self_ref);

        if let Some(mut actor) = old_actor {
            actor.on_exit(&mut context);

            self.drop_context(self_ref, context);
        }
    }

    fn on_enter(&mut self, self_ref: &ActorRef<T>) {
        let mut context = self.create_context(self_ref);

        if let Some(actor) = &mut self.actor {
            actor.on_enter(&mut context);

            self.drop_context(self_ref, context);
        }
    }

    fn on_internal_message(&mut self, _self_ref: &ActorRef<T>, message: InternalMessage) {
        match message {
            InternalMessage::ChildTerminate(msg) => {
                //println!("child terminated : {}", msg);
                let key = msg.key();

                self.cell.childrens.remove(&key);
                // println!("after children size =  {}", context.childrens.len());
            }
        }
    }

    fn on_message(&mut self, self_ref: &ActorRef<T>, message: Message<T>) {
        let mut context = self.create_context(self_ref);

        if let Some(actor) = &mut self.actor {
            match message {
                Message::User(msg) => {
                    self.last_message_timestamp = Instant::now();

                    actor.on_message(&mut context, msg)
                }
                Message::Timer(key, gen, msg) => {
                    self.last_message_timestamp = Instant::now();

                    match context.cell.timer.list.get(&key) {
                        Some(info) if info.gen == gen => actor.on_message(&mut context, msg),
                        _ => {}
                    };
                }
                Message::ReceiveTimeout(exp) => {
                    let num_msg = self_ref.mbox.num_user_message();

                    if let Some(tmout) = context.cell.receive_timeout {
                        if num_msg == 0 {
                            if exp > self.last_message_timestamp {
                                if exp - self.last_message_timestamp >= tmout {
                                    actor.on_system_message(
                                        &mut context,
                                        super::SystemMessage::ReceiveTimeout,
                                    );
                                } else {
                                    let exp =
                                        (self.last_message_timestamp + tmout) - Instant::now();

                                    context.schedule_receive_timeout(exp);
                                }
                            } else {
                                context.schedule_receive_timeout(tmout);
                            }
                        } else {
                            context.schedule_receive_timeout(tmout);
                        }
                    }
                }

                Message::Terminate => {
                    // covered by actor loop
                }
            }
        }
        self.drop_context(self_ref, context);
    }

    fn process_message(&mut self, self_ref: &ActorRef<T>, msg: Message<T>) -> bool {
        match msg {
            Message::Terminate => {
                //println!("stop actor {}", self_ref);

                let da = None;
                let old_actor = replace(&mut self.actor, da);

                self.on_exit(old_actor, self_ref);

                if self.cell.parent.is_some() {
                    self.cell.parent.as_ref().unwrap().send_internal_message(
                        InternalMessage::ChildTerminate(self_ref.path.clone()),
                    )
                }

                self.cell.childrens.iter().for_each(|x| {
                    let child_ref = x.1.stop_ref.as_ref();
                    child_ref.stop();
                });

                self.cell.childrens.clear();

                self_ref.mbox.close();
                true
            }
            _ => {
                self.on_message(self_ref, msg);
                false
            }
        }
    }

    fn num_internal_message(&self, self_ref: &ActorRef<T>) -> usize {
        self_ref.mbox.internal_queue.len()
    }

    fn num_user_message(&self, self_ref: &ActorRef<T>) -> usize {
        self_ref.mbox.message_queue.len()
    }

    fn num_total_message(&self, self_ref: &ActorRef<T>) -> usize {
        self.num_internal_message(self_ref)
            + self_ref.mbox.message_queue.len()
            + self.cell.unstashed.len()
    }

    fn pop_internal_message(&self, self_ref: &ActorRef<T>) -> Option<InternalMessage> {
        self_ref.mbox.internal_queue.pop()
    }

    pub async fn actor_loop(&mut self, self_ref: ActorRef<T>) {
        if self.actor.is_none() {
            self.actor = Some(self.prop.create());

            self.on_enter(&self_ref);
        }

        // let mut actor = cell.actor;
        // let mut ch = cell.ch;

        // let mut stash = cell.stash;
        if self.num_total_message(&self_ref) == 0 {
            return;
        }

        let mut count = 0;
        loop {
            while let Some(msg) = self.pop_internal_message(&self_ref) {
                self.on_internal_message(&self_ref, msg);
            }

            if self.cell.unstashed.len() > 0 {
                while let Some(msg) = self.cell.unstashed.pop() {
                    count += 1;

                    if count >= 100 {
                        count = 0;
                        tokio::task::yield_now().await;
                    }

                    self.process_message(&self_ref, Message::User(msg));
                }
            }

            if self.num_user_message(&self_ref) > 0 {
                if let Some(msg) = self_ref.mbox.message_queue.pop() {
                    count += 1;
                    let stop_flag = self.process_message(&self_ref, msg);
                    if stop_flag {
                        break;
                    }
                } else {
                    break;
                }
            } else if self.num_total_message(&self_ref) == 0 {
                break;
            }

            if count >= 100 {
                count = 0;
                tokio::task::yield_now().await;
            }
        }
    }
}
