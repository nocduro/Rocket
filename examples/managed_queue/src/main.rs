#![feature(plugin, decl_macro)]
#![plugin(rocket_codegen)]

extern crate crossbeam;
#[macro_use] extern crate rocket;

#[cfg(test)] mod tests;

use crossbeam::queue::MsQueue;
use rocket::State;

#[derive(FromForm, Debug)]
struct Event {
    description: String
}

struct LogChannel(MsQueue<Event>);

#[put("/push?<event>")]
fn push(event: Event, queue: State<LogChannel>) {
    queue.0.push(event);
}

#[get("/pop")]
fn pop(queue: State<LogChannel>) -> String {
    let queue = &queue.0;
    queue.pop().description
}

fn rocket() -> rocket::Rocket {
    rocket::ignite()
        .mount("/", routes![push, pop])
        .manage(LogChannel(MsQueue::new()))
}

fn main() {
    rocket().launch();
}
