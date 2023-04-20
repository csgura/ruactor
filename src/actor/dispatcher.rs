use std::borrow::Cow;
use std::collections::HashMap;

use std::mem::replace;
use std::panic::AssertUnwindSafe;
use std::time::Instant;

use futures::FutureExt;
use tokio::task::JoinSet;

use super::context::ActorCell;
use super::context::ActorContext;
use super::Actor;
use super::ActorRef;
use super::AutoMessage;
use super::ChildContainer;
use super::ParentRef;

use super::mailbox::TokioChannelQueue;
use super::InternalMessage;
use super::Message;
use crate::system::PropDyn;

pub struct TimerMessage {
    pub(crate) gen: u32,
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
    pub(crate) actor: Option<Box<dyn Actor<Message = T>>>,
    pub(crate) prop: Box<dyn PropDyn<T>>,
    pub(crate) last_message_timestamp: Instant,
    pub(crate) cell: Option<ActorCell<T>>,
    pub(crate) message_queue: TokioChannelQueue<T>,
}

struct PanicError {}

fn pop_internal_message<T: 'static + Send>(self_ref: &ActorRef<T>) -> Option<InternalMessage> {
    self_ref.mbox.internal_queue.pop()
}

impl<T: 'static + Send> Dispatcher<T> {
    fn create_context(&mut self, self_ref: &ActorRef<T>) -> ActorContext<T> {
        //println!("create context");

        let cell = self.cell.take();
        ActorContext {
            self_ref: self_ref.clone(),
            actor: None,
            cell: cell.unwrap(),
            handle: self_ref.mbox.handle.clone(),
        }
    }

    fn drop_context(&mut self, self_ref: &ActorRef<T>, context: ActorContext<T>) {
        //println!("drop context");
        self.cell = Some(context.cell);

        if context.actor.is_some() {
            let old_actor = replace(&mut self.actor, context.actor);

            self.on_exit(old_actor, self_ref);

            self.on_enter(self_ref);
        }
    }

    fn on_exit(&mut self, old_actor: Option<Box<dyn Actor<Message = T>>>, self_ref: &ActorRef<T>) {
        let mut context = self.create_context(self_ref);
        let _guard = context.handle.enter();

        if let Some(mut actor) = old_actor {
            actor.on_exit(&mut context);
        }
        self.drop_context(self_ref, context);
    }

    fn on_enter(&mut self, self_ref: &ActorRef<T>) {
        let mut context = self.create_context(self_ref);
        let _guard = context.handle.enter();

        if let Some(actor) = &mut self.actor {
            actor.on_enter(&mut context);
        }
        self.drop_context(self_ref, context);
    }

    async fn on_internal_message(&mut self, self_ref: &ActorRef<T>, message: InternalMessage) {
        match message {
            InternalMessage::Terminate(reply_to) => {
                //println!("stop actor {}", self_ref);
                self_ref.mbox.close();

                let da = None;
                let old_actor = replace(&mut self.actor, da);

                self.on_exit(old_actor, self_ref);

                if let Some(parent) = self.take_parent() {
                    parent.send_internal_message(InternalMessage::ChildTerminate(
                        self_ref.path.clone(),
                    ))
                }

                {
                    let childs = self.take_childrens();

                    let mut join_set = JoinSet::new();

                    let mut childs = childs.into_iter().collect::<Vec<_>>();
                    while let Some((_, ch)) = childs.pop() {
                        join_set.spawn(async move { ch.stop_ref.wait_stop().await });
                    }

                    while let Some(_) = join_set.join_next().await {}
                }

                if let Some(sender) = reply_to {
                    let _ = sender.send(());
                }
                //self.cell.childrens.clear();

                //println!("{} stop complete", self_ref);
            }
            InternalMessage::ChildTerminate(msg) => {
                //println!("child terminated : {}", msg);
                let key = msg.key();

                if let Some(cell) = &mut self.cell {
                    cell.childrens.remove(&key);
                }
                //self.cell.childrens.remove(&key);
                // println!("after children size =  {}", context.childrens.len());
            }
            InternalMessage::Created => {}
        }
    }

    async fn on_message_in_context(
        &mut self,
        self_ref: &ActorRef<T>,
        context: &mut ActorContext<T>,
        message: Message<T>,
    ) {
        let _guard = context.handle.enter();

        if let Some(actor) = &mut self.actor {
            match message {
                Message::User(msg) => {
                    self.last_message_timestamp = Instant::now();

                    actor.on_message_async(context, msg).await;
                }
                Message::Timer(key, gen, msg) => {
                    self.last_message_timestamp = Instant::now();

                    let signal = match context.cell.timer.list.get(&key) {
                        Some(info) if info.gen == gen => true,
                        _ => false,
                    };

                    if signal {
                        actor.on_message_async(context, msg).await;
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
                } // Message::Terminate(_) => {
                  //     // covered by actor loop
                  // }
            }
        }
    }
    async fn on_message(
        &mut self,
        self_ref: &ActorRef<T>,
        message: Message<T>,
    ) -> Result<(), PanicError> {
        let mut context = self.create_context(self_ref);
        let res = AssertUnwindSafe(self.on_message_in_context(self_ref, &mut context, message))
            .catch_unwind()
            .await;
        self.drop_context(self_ref, context);

        match res {
            Ok(_) => Ok(()),
            Err(_) => Err(PanicError {}),
        }
    }

    #[allow(dead_code)]
    fn parent_ref(&self) -> Option<&Box<dyn ParentRef>> {
        self.cell.as_ref().and_then(|x| x.parent.as_ref())
    }

    fn take_parent(&mut self) -> Option<Box<dyn ParentRef>> {
        self.cell.as_mut().and_then(|x| x.parent.take())
    }

    fn take_childrens(&mut self) -> HashMap<String, ChildContainer> {
        if let Some(cell) = &mut self.cell {
            let child = replace(&mut cell.childrens, Default::default());
            child
        } else {
            Default::default()
        }
    }

    async fn process_message(&mut self, self_ref: &ActorRef<T>, msg: Message<T>) {
        let res = self.on_message(self_ref, msg).await;
        if let Err(_) = res {
            //println!("panic occurred {:?}", err);
            let old_actor = self.actor.take();
            self.on_exit(old_actor, self_ref);

            self.actor = Some(self.prop.create());
            self.on_enter(&self_ref);
        }
    }

    fn num_internal_message(&self, self_ref: &ActorRef<T>) -> usize {
        self_ref.mbox.internal_queue.len()
    }

    fn num_total_message(&self, self_ref: &ActorRef<T>) -> usize {
        self.num_internal_message(self_ref)
            + self_ref.mbox.message_queue.len()
            + self.cell.as_ref().map(|x| x.unstashed.len()).unwrap_or(0)
    }

    async fn process_internal_message_all(&mut self, self_ref: &ActorRef<T>) {
        while let Some(msg) = pop_internal_message(self_ref) {
            self.on_internal_message(self_ref, msg).await;
        }
    }

    async fn next_message(&mut self, self_ref: &ActorRef<T>) -> Option<Message<T>> {
        self.process_internal_message_all(self_ref).await;

        if self_ref.mbox.is_terminated() {
            return None;
        }

        if let Some(cell) = &mut self.cell {
            if let Some(msg) = cell.unstashed.pop() {
                return Some(Message::User(msg));
            }
        }

        self.message_queue.pop().await
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
        while let Some(msg) = self.next_message(&self_ref).await {
            self.process_message(&self_ref, msg).await;

            count += 1;

            if count >= 100 {
                count = 0;
                if self_ref.mbox.dedicated_runtime.is_none() {
                    tokio::task::yield_now().await;
                }
            }
        }
    }
}
