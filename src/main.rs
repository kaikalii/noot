#![allow(unstable_name_collisions)]
#![allow(dead_code)]

mod ast;
mod compile;
mod parse;
mod resolve;

fn main() {
    use std::process::Command;

    use compile::*;

    color_backtrace::install();

    let input = std::fs::read_to_string("test.noot").unwrap();
    match parse::parse(&input) {
        Ok(items) => {
            println!("{}", items);

            println!();

            let mut target = CTarget::new("main", true);
            target.compile_items(items, false);
            if target.res.errors.is_empty() {
                println!("No resolution errors");
                target.write().unwrap();
                Command::new("gcc")
                    .arg("build/main.c")
                    .arg("-o")
                    .arg("test")
                    .spawn()
                    .unwrap()
                    .wait()
                    .unwrap();
            } else {
                for error in &target.res.errors {
                    println!("{}", error);
                }
            }
        }
        Err(e) => println!("{}", e),
    }
}
