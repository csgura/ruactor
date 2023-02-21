#[derive(Debug)]
struct A {}

impl Drop for A {
    fn drop(&mut self) {
        println!("drop A");
    }
}

#[derive(Debug)]
struct B {}
impl Drop for B {
    fn drop(&mut self) {
        println!("drop B");
    }
}

fn with_b() {
    let b = B {};

    println!("b = {:?}", b);
    panic!("panic!!");
}

fn with_a() -> String {
    let a = A {};
    println!("a = {:?}", a);

    with_b();
    "hello".into()
}

/*
thread 'main' panicked at 'panic!!', examples/unwind.rs:21:5
stack backtrace:
   0: rust_begin_unwind
             at /rustc/d5a82bbd26e1ad8b7401f6a718a9c57c96905483/library/std/src/panicking.rs:575:5
   1: core::panicking::panic_fmt
             at /rustc/d5a82bbd26e1ad8b7401f6a718a9c57c96905483/library/core/src/panicking.rs:64:14
   2: unwind::withB
             at ./examples/unwind.rs:21:5
   3: unwind::withA
             at ./examples/unwind.rs:26:5
   4: unwind::main::{{closure}}
             at ./examples/unwind.rs:51:43
   5: std::panicking::try::do_call
             at /rustc/d5a82bbd26e1ad8b7401f6a718a9c57c96905483/library/std/src/panicking.rs:483:40
   6: ___rust_try
   7: std::panicking::try
             at /rustc/d5a82bbd26e1ad8b7401f6a718a9c57c96905483/library/std/src/panicking.rs:447:19
   8: std::panic::catch_unwind
             at /rustc/d5a82bbd26e1ad8b7401f6a718a9c57c96905483/library/std/src/panic.rs:137:14
   9: unwind::main
             at ./examples/unwind.rs:51:15
  10: core::ops::function::FnOnce::call_once
             at /rustc/d5a82bbd26e1ad8b7401f6a718a9c57c96905483/library/core/src/ops/function.rs:507:5
note: Some details are omitted, run with `RUST_BACKTRACE=full` for a verbose backtrace.
 */

fn set_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let location = info.location().unwrap();
        // thread 'main' panicked at 'panic!!', examples/unwind.rs:19:5
        println!("panic location = {}", location);

        let msg = match info.payload().downcast_ref::<&'static str>() {
            Some(s) => *s,
            None => match info.payload().downcast_ref::<String>() {
                Some(s) => &s[..],
                None => "Box<dyn Any>",
            },
        };

        let thread = std::thread::current();
        let name = thread.name().unwrap_or("<unnamed>");

        let _ = println!("thread '{name}' panicked at '{msg}', {location}");
        let bk = std::backtrace::Backtrace::force_capture().to_string();
        let bk = bk.split("\n");
        let str = bk
            .skip_while(|x| !x.contains("__rust_end_short_backtrace"))
            .skip(2)
            .take_while(|x| !x.contains("__rust_begin_short_backtrace"))
            .collect::<Vec<&str>>()
            .join("\n");
        println!("{}", str);
    }));
}
fn main() {
    set_panic_hook();
    let res = std::panic::catch_unwind(|| with_a());
    if let Err(err) = res {
        println!("res = {:?}", err);
    }
}
