# Protocol Design Group 9
This repository contains the implementation of the TBD protocol from group 3 and 9.

# Usage
The usage of the program can either be done via cargo or by building and using the final executable.

## Building the program
The program is build with:

    cargo build --release 

## Running the client
The client is run with either cargo:

    cargo run --release -- <server addr> [options] <file(s)>

or directly from the command line:

    ./pd <server addr> [options] <file(s)>

## Running the server
The server can also be run with cargo:

    cargo run --release -- -s <server_addr> [options]

or directly from the command line:

    ./pd -s <server addr> [options]