use std::{
    marker::PhantomData,
    sync::{
        atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering},
        Arc,
    },
    time::Instant,
};

use crossbeam::queue::SegQueue;
use tokio::sync::Mutex;

use crate::{Actor, Prop};

use super::{
    context::ActorCell, ActorRef, Dispatcher, InternalMessage, Message, ParentRef, PropWrap,
};

pub struct Mailbox<T: 'static + Send> {
    pub(crate) internal_queue: SegQueue<InternalMessage>,
    pub(crate) message_queue: SegQueue<Message<T>>,
    pub(crate) running: Arc<AtomicBool>,
    pub(crate) terminated: Arc<AtomicBool>,
    pub(crate) dispatcher: Arc<Mutex<Dispatcher<T>>>,
    pub(crate) handle: tokio::runtime::Handle,
}

impl<T: 'static + Send> Clone for Mailbox<T> {
    fn clone(&self) -> Self {
        Self {
            internal_queue: SegQueue::new(),
            message_queue: SegQueue::new(),
            running: self.running.clone(),
            terminated: self.terminated.clone(),
            dispatcher: self.dispatcher.clone(),
            handle: self.handle.clone(),
        }
    }
}

impl<T: 'static + Send> Mailbox<T> {
    pub(crate) fn new<P, A>(p: P, parent: Option<Box<dyn ParentRef>>) -> Mailbox<T>
    where
        P: Prop<A>,
        A: Actor<Message = T>,
    {
        let pdyn = PropWrap {
            prop: p,
            phantom: PhantomData,
        };

        let dispatcher = Dispatcher {
            actor: None,
            prop: Box::new(pdyn),
            last_message_timestamp: Instant::now(),
            cell: ActorCell::new(parent),
        };

        let mbox = Mailbox {
            internal_queue: SegQueue::new(),
            dispatcher: Arc::new(Mutex::new(dispatcher)),
            message_queue: SegQueue::new(),
            running: Arc::new(false.into()),
            terminated: Arc::new(false.into()),
            handle: tokio::runtime::Handle::current(),
        };
        mbox
    }

    //#[async_recursion::async_recursion]
    pub(crate) async fn receive(&self, self_ref: ActorRef<T>) {
        let mut owned = false;

        if let Ok(mut dispatcher) = self.dispatcher.try_lock() {
            //println!("start receive loop");

            owned = true;
            self.running.store(true, Ordering::SeqCst);
            dispatcher.actor_loop(self_ref.clone()).await;
            self.running.store(false, Ordering::SeqCst);
            //println!("end receive loop");
        } else {
            //println!("lock failed");
        }

        if owned {
            let num_msg = self.num_total_message();
            if num_msg > 0 {
                //println!("num msg = {}", num_msg);
                //self.receive().await;
                self.schedule(self_ref.clone());
            }
        }
    }

    pub(crate) fn num_total_message(&self) -> usize {
        self.internal_queue.len() + self.message_queue.len()
    }

    pub(crate) fn num_user_message(&self) -> usize {
        self.message_queue.len()
    }

    pub(crate) fn schedule(&self, self_ref: ActorRef<T>) {
        if !self.running.load(Ordering::SeqCst) {
            //println!("schedule");
            let cl: Mailbox<T> = self.clone();

            self.handle.spawn(async move {
                cl.receive(self_ref.clone()).await
                //cl.status.store(true, std::sync::atomic::Ordering::SeqCst);
                //actor_loop( &mut cell ).await;
            });
        } else {
            //println!("not schedule");
        }
    }

    pub(crate) fn is_terminated(&self) -> bool {
        self.terminated.load(Ordering::SeqCst)
    }

    pub(crate) fn close(&self) {
        self.terminated.store(true, Ordering::SeqCst);
    }

    pub(crate) fn send(&self, self_ref: ActorRef<T>, msg: Message<T>) {
        if !self.is_terminated() {
            self.message_queue.push(msg);
            self.schedule(self_ref);
        }
    }

    pub(crate) fn send_internal(&self, self_ref: ActorRef<T>, msg: InternalMessage) {
        self.internal_queue.push(msg);
        self.schedule(self_ref);
    }
}
