use std::{
    borrow::Cow,
    collections::{HashMap, VecDeque},
    sync::Arc,
    task::Context,
    time::{Duration, Instant},
};

use futures::{Future, FutureExt};

use crate::{Actor, Props};

use super::{
    dispatcher::TimerMessage, ActorRef, ChildContainer, InternalMessage, Mailbox, Message,
    ParentRef, Scheduled, Timer,
};

pub struct ActorContext<T: 'static + Send> {
    pub(crate) self_ref: ActorRef<T>,
    pub(crate) actor: Option<Box<dyn Actor<Message = T>>>,
    pub(crate) handle: tokio::runtime::Handle,
    pub(crate) stash: VecDeque<T>,
    pub(crate) unstashed: VecDeque<T>,
    pub(crate) timer: Timer,
    pub(crate) childrens: HashMap<String, ChildContainer>,
    pub(crate) receive_timeout: Option<Duration>,
    pub(crate) scheduled_receive_timeout: Option<Scheduled>,
    pub(crate) timer_gen: u32,
    pub(crate) next_name_offset: usize,
    pub(crate) suspend_reason: Option<SuspendReason>,
}

pub(crate) enum SuspendReason {
    ChildrenTermination,
}

// impl<T: 'static + Send> Default for ActorCell<T> {
//     fn default() -> Self {
//         Self {
//             stash: Default::default(),
//             unstashed: Default::default(),
//             timer: Default::default(),
//             childrens: Default::default(),
//             receive_timeout: Default::default(),
//             scheduled_receive_timeout: None,
//             timer_gen: Default::default(),
//             next_name_offset: 0,
//             suspend_reason: None,
//         }
//     }
// }

const BASE64CHARS: &str = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789+~";

fn base64(l: usize, s: String) -> String {
    let mut s = s;
    let ch = BASE64CHARS.chars().nth(l & 63).unwrap_or('~');
    s.push(ch);
    let next = l >> 6;
    if next == 0 {
        s
    } else {
        base64(next, s)
    }
}

impl<T: 'static + Send> ActorContext<T> {
    pub fn spawner(&mut self) -> tokio::runtime::Handle {
        self.handle.clone()
    }

    pub fn self_ref(&mut self) -> ActorRef<T> {
        self.self_ref.clone()
    }

    pub fn transit<A: Actor<Message = T> + 'static>(&mut self, new_actor: A) {
        self.actor = Some(Box::new(new_actor));
    }

    pub fn spawn(&mut self, f: impl Future<Output = ()> + Send + 'static) {
        let mut f = Box::pin(f);

        let waker = futures::task::noop_waker();
        let mut ctx = Context::from_waker(&waker);

        let res = f.poll_unpin(&mut ctx);

        match res {
            std::task::Poll::Ready(_) => {}
            std::task::Poll::Pending => {
                self.handle.spawn(f);
            }
        }
    }

    fn next_timer_gen(&mut self) -> u32 {
        let ret = self.timer_gen;

        self.timer_gen = match self.timer_gen {
            u32::MAX => 0,
            _ => self.timer_gen + 1,
        };

        ret
    }
    pub fn start_single_timer<S>(&mut self, name: S, d: Duration, t: T)
    where
        S: Into<Cow<'static, str>>,
    {
        let name = name.into();

        let gen = self.next_timer_gen();

        let self_ref: ActorRef<T> = self.self_ref.clone();

        let nc = name.clone();
        let scheduled = self.self_ref.mbox.scheduler.after_func(d, move || {
            self_ref.send(Message::Timer(nc, gen, t));
        });

        self.timer
            .list
            .insert(name, TimerMessage { gen, scheduled });

        // self.handle.spawn(async move {
        //     let s = tokio::time::sleep(d);
        //     s.await;

        //     self_ref.send(Message::Timer(name, gen, t));
        // });
    }

    pub fn cancel_timer<S>(&mut self, name: S)
    where
        S: Into<Cow<'static, str>>,
    {
        let name = name.into();
        self.timer.list.remove(&name);
    }

    pub fn set_receive_timeout(&mut self, d: Duration) {
        self.receive_timeout = Some(d);
        self.schedule_receive_timeout(d);
    }

    pub fn cancel_receive_timeout(&mut self) {
        self.receive_timeout = None;
        self.scheduled_receive_timeout = None;
    }

    pub(crate) fn schedule_receive_timeout(&mut self, d: Duration) {
        // let self_ref = self.self_ref.clone();

        // let tmout = Instant::now() + d;

        // self.handle.spawn(async move {
        //     let s = tokio::time::sleep(d);
        //     s.await;

        //     self_ref.send(Message::ReceiveTimeout(tmout));
        // });

        let self_ref = self.self_ref.clone();
        let tmout = Instant::now() + d;

        self.scheduled_receive_timeout =
            Some(self.self_ref.mbox.scheduler.after_func(d, move || {
                self_ref.send(Message::ReceiveTimeout(tmout));
            }));
    }

    pub fn stash(&mut self, message: T) {
        self.stash.push_back(message);
    }

    pub fn unstash_all(&mut self) {
        while let Some(msg) = self.stash.pop_front() {
            self.unstashed.push_back(msg);
        }
    }

    pub fn get_child<M: 'static + Send>(&self, name: &str) -> Option<ActorRef<M>> {
        let ret = self
            .childrens
            .get(name)
            .and_then(|any| any.actor_ref.downcast_ref::<ActorRef<M>>().cloned());

        ret
    }

    fn random_name(&mut self) -> String {
        let num = self.next_name_offset;
        self.next_name_offset = self.next_name_offset.checked_add(1).unwrap_or(0);
        base64(num, "$".into())
    }

    pub fn create_child<A: Actor, P: Props<A>>(&mut self, prop: P) -> ActorRef<A::Message> {
        let name = self.random_name();
        self.get_or_create_child(name, prop)
    }

    pub fn get_or_create_child<A: Actor, P: Props<A>>(
        &mut self,
        name: String,
        prop: P,
    ) -> ActorRef<A::Message> {
        let ret = self.get_child(&name);
        match ret {
            Some(actor_ref) => actor_ref,
            None => {
                //println!("actor {} crate child {}", self.self_ref, name);
                let cpath = self.self_ref.path.as_ref().clone() / name.as_str();

                let mbox = Mailbox::new(
                    cpath.clone(),
                    prop,
                    Some(Box::new(self.self_ref.clone())),
                    self.self_ref.mbox.pool.clone(),
                    self.handle.clone(),
                    self.self_ref.mbox.scheduler.clone(),
                );

                let actor_ref = ActorRef::new(cpath, Arc::new(mbox));

                self.childrens.insert(
                    name,
                    ChildContainer {
                        actor_ref: Box::new(actor_ref.clone()),
                        stop_ref: Box::new(actor_ref.clone()),
                    },
                );
                actor_ref.send_internal_message(InternalMessage::Created);

                actor_ref
            }
        }
    }

    pub fn num_children(&mut self) -> usize {
        self.childrens.len()
    }

    pub fn stop_self(&mut self) {
        self.self_ref
            .send_internal_message(InternalMessage::Terminate);
    }

    pub fn num_user_message(&mut self) -> usize {
        self.self_ref.mbox.num_user_message()
    }
}

#[cfg(test)]
mod tests {
    use super::base64;

    #[test]
    fn base64_test() {
        let l = 0;
        let res = base64(l, "$".into());
        assert_eq!(res, "$a");

        let l = 3;
        let res = base64(l, "$".into());
        assert_eq!(res, "$d");

        let l = 63;
        let res = base64(l, "$".into());
        assert_eq!(res, "$~");

        let l = 64;
        let res = base64(l, "$".into());
        assert_eq!(res, "$ab");

        let l = 69;
        let res = base64(l, "$".into());
        assert_eq!(res, "$fb");
    }
}
