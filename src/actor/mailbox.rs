use std::{
    marker::PhantomData,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

use crossbeam::queue::SegQueue;
use rayon::ThreadPool;
use tokio::sync::Mutex;

use crate::{Actor, ActorPath, Props, PropsOption};

use super::{
    context::ActorCell, ActorRef, Dispatcher, InternalMessage, Message, ParentRef, PropWrap,
};

pub(crate) struct TokioChannelQueue<T: 'static + Send> {
    dedicated: bool,
    sender: tokio::sync::mpsc::UnboundedSender<Message<T>>,
    receiver: tokio::sync::mpsc::UnboundedReceiver<Message<T>>,
    num_msg: Arc<AtomicUsize>,
}

pub(crate) struct TokioChannelSender<T: 'static + Send> {
    sender: tokio::sync::mpsc::UnboundedSender<Message<T>>,
    num_msg: Arc<AtomicUsize>,
}

impl<T: 'static + Send> TokioChannelSender<T> {
    pub fn len(&self) -> usize {
        self.num_msg.load(Ordering::SeqCst)
    }

    pub fn push(&self, msg: Message<T>) {
        let _ = self.sender.send(msg);
        self.num_msg.fetch_add(1, Ordering::SeqCst);
    }
}
impl<T: 'static + Send> TokioChannelQueue<T> {
    pub fn new(dedicated: bool) -> Self {
        let ch = tokio::sync::mpsc::unbounded_channel();

        TokioChannelQueue {
            dedicated,
            sender: ch.0,
            receiver: ch.1,
            num_msg: Default::default(),
        }
    }

    pub fn sender(&self) -> TokioChannelSender<T> {
        return TokioChannelSender {
            sender: self.sender.clone(),
            num_msg: self.num_msg.clone(),
        };
    }
    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.num_msg.load(Ordering::SeqCst)
    }

    #[allow(dead_code)]
    pub fn push(&self, msg: Message<T>) {
        let _ = self.sender.send(msg);
        self.num_msg.fetch_add(1, Ordering::SeqCst);
    }

    pub async fn pop(&mut self) -> Option<Message<T>> {
        let ret = if self.dedicated {
            self.receiver.try_recv().ok()
        } else {
            tokio::time::timeout(Duration::from_secs(1), self.receiver.recv())
                .await
                .unwrap_or(None)
        };
        if ret.is_some() {
            self.num_msg.fetch_sub(1, Ordering::SeqCst);
        }
        ret
    }
}
pub struct Mailbox<T: 'static + Send> {
    pub(crate) option: PropsOption,
    pub(crate) internal_queue: SegQueue<InternalMessage>,
    //pub(crate) message_queue: SegQueue<Message<T>>,
    pub(crate) message_queue: TokioChannelSender<T>,

    pub(crate) running: Arc<AtomicBool>,
    pub(crate) terminated: Arc<AtomicBool>,
    pub(crate) dispatcher: Arc<Mutex<Dispatcher<T>>>,
    pub(crate) handle: tokio::runtime::Handle,
    pub(crate) pool: Arc<ThreadPool>,
    pub(crate) dedicated_runtime: Option<tokio::runtime::Runtime>,
    pub(crate) child_runtime: Option<tokio::runtime::Runtime>,
}

impl<T: 'static + Send> Drop for Mailbox<T> {
    fn drop(&mut self) {
        let runtime = self.dedicated_runtime.take();
        if let Some(runtime) = runtime {
            self.pool.install(move || drop(runtime))
        }

        let runtime = self.child_runtime.take();
        if let Some(runtime) = runtime {
            self.pool.install(move || drop(runtime))
        }
    }
}

// impl<T: 'static + Send> Clone for Mailbox<T> {
//     fn clone(&self) -> Self {
//         Self {
//             internal_queue: SegQueue::new(),
//             message_queue: SegQueue::new(),
//             running: self.running.clone(),
//             terminated: self.terminated.clone(),
//             dispatcher: self.dispatcher.clone(),
//             handle: self.handle.clone(),
//         }
//     }
// }

pub(crate) async fn receive<T: 'static + Send>(self_ref: ActorRef<T>) {
    let mbox = self_ref.mbox.as_ref();

    if let Ok(mut dispatcher) = mbox.dispatcher.try_lock() {
        //mbox.running.store(true, Ordering::SeqCst);
        dispatcher.actor_loop(self_ref.clone()).await;
        drop(dispatcher);

        mbox.running.store(false, Ordering::SeqCst);

        let num_msg = mbox.num_user_message();
        if num_msg > 0 && !mbox.is_terminated() {
            mbox.schedule(self_ref.clone());
        } else {
            let num_sys_msg = mbox.num_internal_message();
            if num_sys_msg > 0 {
                mbox.schedule(self_ref.clone());
            }
        }
    }
}

impl<T: 'static + Send> Mailbox<T> {
    pub(crate) fn new<P, A>(
        path: ActorPath,
        p: P,
        parent: Option<Box<dyn ParentRef>>,
        pool: Arc<ThreadPool>,
        handle: tokio::runtime::Handle,
    ) -> Mailbox<T>
    where
        P: Props<A>,
        A: Actor<Message = T>,
    {
        let option = p.option();
        let dedicated_thread = option.dedicated_thread();
        let pdyn = PropWrap {
            prop: p,
            phantom: PhantomData,
        };

        let message_queue = TokioChannelQueue::new(dedicated_thread.is_some());
        let message_sender = message_queue.sender();

        let dispatcher = Dispatcher {
            actor: None,
            prop: Box::new(pdyn),
            last_message_timestamp: Instant::now(),
            cell: Some(ActorCell::new(parent)),
            message_queue,
            watcher: Default::default(),
        };

        let dedicated_runtime = dedicated_thread.and_then(|_| {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .ok()
        });

        let (child_runtime, actor_handle) = if let Some(nworker) = dedicated_thread {
            let nworker = std::cmp::max(2, nworker);

            let runtime = tokio::runtime::Builder::new_multi_thread()
                .thread_name(path.to_string())
                .worker_threads(nworker)
                .enable_all()
                .build()
                .ok();
            let handle = runtime
                .as_ref()
                .map(|x| x.handle().clone())
                .unwrap_or(handle);
            (runtime, handle)
        } else {
            (None, handle)
        };

        let mbox = Mailbox {
            option,
            internal_queue: SegQueue::new(),
            dispatcher: Arc::new(Mutex::new(dispatcher)),
            //message_queue: SegQueue::new(),
            message_queue: message_sender,
            running: Arc::new(false.into()),
            terminated: Arc::new(false.into()),
            handle: actor_handle,
            dedicated_runtime: dedicated_runtime,
            child_runtime,
            pool: pool.clone(),
        };
        mbox
    }

    //#[async_recursion::async_recursion]

    pub(crate) fn num_internal_message(&self) -> usize {
        self.internal_queue.len()
    }

    #[allow(dead_code)]
    pub(crate) fn num_total_message(&self) -> usize {
        self.internal_queue.len() + self.message_queue.len()
    }

    pub(crate) fn num_user_message(&self) -> usize {
        self.message_queue.len()
    }

    pub(crate) fn schedule(&self, self_ref: ActorRef<T>) {
        if let Ok(_) =
            self.running
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        {
            if let Some(runtime) = &self.dedicated_runtime {
                let handle = runtime.handle().clone();

                self.pool.spawn(move || {
                    let _guard = handle.enter();
                    handle.block_on(async move {
                        receive(self_ref).await;
                    });
                })
            } else {
                self.handle.spawn(async move { receive(self_ref).await });
            }
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
