#![feature(const_fn)]

extern crate easter;
extern crate esprit;


use std::env;
use std::fs::File;
use std::io::prelude::*;

// mod esprima;
mod atoms;
mod serialize;
mod varnum;

fn main() {
    for source_path in env::args().skip(1) {
        println!("Reading {}", source_path);
        let mut source_text = String::new();
        let mut file = File::open(source_path).expect("Could not open file.");
        file.read_to_string(&mut source_text).expect("Could not read file.");

        println!("Parsing...");
        let script = esprit::script(&source_text).expect("Could not parse file.");

        println!("Compiling...");
        serialize::compile(&script);
    }
}