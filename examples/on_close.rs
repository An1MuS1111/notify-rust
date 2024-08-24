#![allow(unused_imports, dead_code)]
use std::{io, thread};

use notify_rust::Notification;

fn wait_for_keypress() {
    println!("halted until you hit the \"ANY\" key");
    io::stdin().read_line(&mut String::new()).unwrap();
}

fn print() {
    println!("notification was closed, don't know why");
}
fn print2() {
    println!("this is an extra callback");
}

#[cfg(any(target_os = "windows", target_os = "macos"))]
fn main() {
    println!("this is a xdg only feature")
}

#[cfg(all(unix, not(target_os = "macos")))]
fn main() {
    thread::spawn(|| {
        Notification::new()
            .summary("Time is running out")
            .body("This will go away.")
            .icon("clock")
            .show()
            .map(|handler| {
                handler.on_close_async(print);
                handler.on_close_async(print2);
            })
    });
    wait_for_keypress();
}
