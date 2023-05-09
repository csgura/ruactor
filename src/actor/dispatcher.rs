use std::borrow::Cow;
use std::collections::HashMap;

use std::mem::replace;
use std::panic;
use std::panic::AssertUnwindSafe;
use std::time::Instant;

use super::context::ActorContext;
use super::Actor;
use super::ActorRef;
use super::AutoMessage;
use super::ParentRef;
use super::Scheduled;

use super::context::SuspendReason;
use super::mailbox::CrossbeamSegQueue;
use super::InternalMessage;
use super::Message;
use crate::system::PropDyn;
use crate::ReplyTo;

#[allow(dead_code)]
pub struct TimerMessage {
    pub(crate) gen: u32,
    pub(crate) scheduled: Scheduled,
}

pub struct Timer {
    pub(crate) list: HashMap<Cow<'static, str>, TimerMessage>,
}

impl Default for Timer {
    fn default() -> Self {
        Self {
            list: Default::default(),
        }
    }
}

pub struct Dispatcher<T: 'static + Send> {
    pub(crate) parent: Option<Box<dyn ParentRef>>,
    pub(crate) actor: Option<Box<dyn Actor<Message = T>>>,
    pub(crate) prop: Box<dyn PropDyn<T>>,
    pub(crate) last_message_timestamp: Instant,
    pub(crate) context: Option<ActorContext<T>>,
    pub(crate) message_queue: CrossbeamSegQueue<T>,
    pub(crate) watcher: Vec<ReplyTo<()>>,
}

impl<T: 'static + Send> Dispatcher<T> {
    fn create_context(&mut self, self_ref: &ActorRef<T>) {
        if self.context.is_none() {
            self.context = Some(ActorContext {
                self_ref: self_ref.clone(),
                actor: None,
                cell: Default::default(),
                handle: self_ref.mbox.handle.clone(),
            })
        }
    }

    fn check_become(&mut self, self_ref: &ActorRef<T>) {
        let context = self.context.as_mut().expect("must");
        if context.actor.is_some() {
            let old_actor = replace(&mut self.actor, context.actor.take());

            self.on_exit(old_actor, self_ref);

            self.on_enter(self_ref);
        }
    }

    fn on_exit(&mut self, old_actor: Option<Box<dyn Actor<Message = T>>>, self_ref: &ActorRef<T>) {
        if let Some(mut actor) = old_actor {
            actor.on_exit(self.context.as_mut().expect("must"));
            self.check_become(self_ref);
        }
    }

    fn on_enter(&mut self, self_ref: &ActorRef<T>) {
        if let Some(actor) = &mut self.actor {
            actor.on_enter(self.context.as_mut().expect("must"));
            self.check_become(self_ref);
        }
    }

    fn terminate(&mut self, self_ref: &ActorRef<T>) {
        let da = None;
        let old_actor = replace(&mut self.actor, da);

        self.on_exit(old_actor, self_ref);

        if let Some(parent) = self.take_parent() {
            parent.send_internal_message(InternalMessage::ChildTerminate(self_ref.path.clone()))
        }

        let watcher = replace(&mut self.watcher, Default::default());
        watcher.into_iter().for_each(|x| {
            let _ = x.send(());
        });

        self.context = None;
    }
    fn on_internal_message(&mut self, self_ref: &ActorRef<T>, message: InternalMessage) {
        match message {
            InternalMessage::Watch(reply_to) => {
                if self_ref.mbox.is_terminated() {
                    let _ = reply_to.send(());
                } else {
                    self.watcher.push(reply_to);
                }
            }
            InternalMessage::Terminate => {
                self_ref.mbox.close();
                let context: &mut ActorContext<T> = self.context.as_mut().expect("must");

                if context.cell.childrens.len() > 0 {
                    context.cell.suspend_reason = Some(SuspendReason::ChildrenTermination);

                    context.cell.childrens.iter().for_each(|c| {
                        c.1.stop_ref.stop();
                    });
                } else {
                    self.terminate(self_ref);
                }
            }
            InternalMessage::ChildTerminate(msg) => {
                let key = msg.key();

                let context: &mut ActorContext<T> = self.context.as_mut().expect("must");

                context.cell.childrens.remove(&key);

                if context.cell.suspend_reason.is_some() && context.cell.childrens.len() == 0 {
                    context.cell.suspend_reason = None;
                    self.terminate(self_ref);
                }
            }
            InternalMessage::Created => {}
        }
    }

    fn on_message(&mut self, self_ref: &ActorRef<T>, message: Message<T>) {
        if let Some(actor) = &mut self.actor {
            let context = self.context.as_mut().expect("must");
            match message {
                Message::User(msg) => {
                    if context.cell.receive_timeout.is_some() {
                        self.last_message_timestamp = Instant::now();
                    }

                    actor.on_message(context, msg);
                }
                Message::Timer(key, gen, msg) => {
                    if context.cell.receive_timeout.is_some() {
                        self.last_message_timestamp = Instant::now();
                    }

                    let signal = match context.cell.timer.list.get(&key) {
                        Some(info) if info.gen == gen => true,
                        _ => false,
                    };

                    if signal {
                        actor.on_message(context, msg);
                    }
                }
                Message::AutoMessage(AutoMessage::PoisonPill) => context.stop_self(),
                Message::ReceiveTimeout(exp) => {
                    let num_msg = self_ref.mbox.num_user_message();

                    if let Some(tmout) = context.cell.receive_timeout {
                        if num_msg == 0 {
                            if exp > self.last_message_timestamp {
                                if exp - self.last_message_timestamp >= tmout {
                                    actor.on_system_message(
                                        context,
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
            }
        }
    }

    fn process_message(&mut self, self_ref: &ActorRef<T>, message: Message<T>) {
        // let res = AssertUnwindSafe(self.on_message(self_ref, message))
        //     .catch_unwind()
        //     .await;

        let res = panic::catch_unwind(AssertUnwindSafe(|| self.on_message(self_ref, message)));

        match res {
            Ok(_) => {
                self.check_become(self_ref);
            }
            Err(_) => {
                let old_actor = self.actor.take();
                self.on_exit(old_actor, self_ref);

                self.actor = Some(self.prop.create());
                self.on_enter(&self_ref);
            }
        }
    }

    fn take_parent(&mut self) -> Option<Box<dyn ParentRef>> {
        self.parent.take()
    }

    // fn take_childrens(&mut self, context: &mut ActorContext<T>) -> HashMap<String, ChildContainer> {
    //     let child = replace(&mut context.cell.childrens, Default::default());
    //     child
    // }

    #[allow(dead_code)]
    fn num_internal_message(&self, self_ref: &ActorRef<T>) -> usize {
        self_ref.mbox.internal_queue.len()
    }

    #[allow(dead_code)]
    fn num_total_message(&self, self_ref: &ActorRef<T>, context: &mut ActorContext<T>) -> usize {
        self.num_internal_message(self_ref)
            + self_ref.mbox.message_queue.len()
            + context.cell.unstashed.len()
    }

    fn pop_internal_message(&mut self, self_ref: &ActorRef<T>) -> Option<InternalMessage> {
        if self.context.is_none() {
            return None;
        }
        self_ref.mbox.internal_queue.pop()
    }

    fn process_internal_message_all(&mut self, self_ref: &ActorRef<T>) {
        while let Some(msg) = self.pop_internal_message(self_ref) {
            self.on_internal_message(self_ref, msg);
        }
    }

    fn next_message(&mut self, self_ref: &ActorRef<T>) -> Option<Message<T>> {
        if self.context.is_none() {
            return None;
        }

        self.process_internal_message_all(self_ref);

        if self_ref.mbox.is_terminated() {
            return None;
        }

        let context: &mut ActorContext<T> = self.context.as_mut().expect("must");

        if let Some(msg) = context.cell.unstashed.pop_front() {
            return Some(Message::User(msg));
        }

        self.message_queue.pop()
    }

    pub fn actor_loop(&mut self, self_ref: ActorRef<T>) {
        let _guard = self_ref.mbox.handle.enter();

        self.create_context(&self_ref);

        if self.actor.is_none() {
            self.actor = Some(self.prop.create());

            self.on_enter(&self_ref);
        }

        let throughput = self_ref.mbox.option.throughput();
        let dedicated = self_ref.mbox.dedicated_runtime.is_some();

        let mut count = 0;
        while let Some(msg) = self.next_message(&self_ref) {
            self.process_message(&self_ref, msg);
            if !dedicated {
                count += 1;

                if count >= throughput {
                    break;
                }
            }
        }
    }
}
