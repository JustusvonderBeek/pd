extern crate pnet;

use dns_lookup::lookup_host;
use pnet::datalink::{self, NetworkInterface, interfaces};

/* The commandline options */
#[derive(Clone)]
pub struct Options {
    pub client_port: u32,
    pub server_port: u32,
    pub server: bool,
    pub hostname: String,
    pub local_hostname : String,
    pub p: f64,
    pub q: f64,
    pub filename: Vec<String>,
    pub logfile: String,
}

fn default_options() -> Options {
    let set = Options {
        client_port: 6001,
        server_port: 5001,
        server: false,
        hostname: String::new(),
        local_hostname : String::new(),
        p: 0.0,
        q: 0.0,
        filename: vec![],
        logfile: String::from("tbd.log"),
    };
    set
}

fn print_help() {
    println!("RFT - Robust File Transfer:");
    println!("Usage (Server):");
    println!("rft [-s [<host>]] [-t <port>] [-p <p>] [-q <q>] [-l <logfile>]");
    println!("Usage (Client):");
    println!("rft <host> [-t <port>] [-p <p>] [-q <q>] [-l <logfile>] <file> ...");
    println!("Options:");
    println!("-s: servermode: accept incoming requests from any host. Operates in client mode if “–s” is not specified. Expected as first argument! Default address is 127.0.0.1.");
    println!("<host>: The address for the server to bind to (optional) or for the client to connect to. Default: 127.0.0.1");
    println!("-t: specify the port number of the server. Default server: 5001, Default client: >=6001");
    println!("-p, -q: specify the loss probabilities for the Markov chain model. If only one is specified, p=q is assumed; if neither is specified no loss is assumed.");
    println!("-l: Specify the path to the logfile. Default: tbd.log");
    println!("<file> the name(s) of the file(s) to fetch.");
}

pub fn parse_cmdline(args : Vec<String>) -> Option<Options> {
    if args.len() < 2 {
        error!("The given arguments are too short!");
    } else {
        let mut settings = default_options();
        
        // Expecting either the hostname or server option as first argument
        println!("Args: {:?}", args);
        // Extracting the host or server information
        let mut i = 1; // Skip the path
        match args[i].as_str() {
            "-s" => {
                settings.server = true;
                i += 1;
                if args.len() > 2 && !args[i].starts_with('-') {
                    settings.hostname.push_str(&args[i]);
                    i += 1;
                }
            },
            _ => {
                settings.hostname.push_str(&args[i]);
                i += 1;
            },
        }

        resolve_hostname(&mut settings);
        let mut pq = 0;

        while i < args.len() {
            let _str = args.get(i).unwrap();
            match _str.as_str() {
                "-h" => {
                    print_help();
                    std::process::exit(0);
                },
                "-t" => {
                    // Expecting a port to operate on
                    let port = args.get(i + 1).expect("Expected a port but got nothing!");
                    settings.server_port = port.parse::<u32>().unwrap();
                    i += 1;
                },
                "-p" => {
                    let p = args.get(i + 1).expect("Expected a probability p but got nothing!");
                    settings.p = p.parse::<f64>().unwrap();
                    i += 1;
                    pq += 1;
                },
                "-q" => {
                    let q = args.get(i + 1).expect("Expected a probability q but got nothing!");
                    settings.q = q.parse::<f64>().unwrap();
                    i += 1;
                    pq += 2;
                },
                "-l" => {
                    let logfile = args.get(i + 1).expect("Expected a logfile but got nothing!");
                    settings.logfile = String::from(logfile);
                    i += 1;
                }
                _ => {
                    if settings.server {
                        error!("Cannot transfer a file in server mode!");
                        return None;
                    }
                    println!("File: {}", _str);
                    settings.filename.push(_str.to_string());
                },
            }
            i += 1;
        }
        
        match pq {
            1 => settings.q = settings.p,
            2 => settings.p = settings.q,
            _ => {},
        }

        return Some(settings)
    }
    None
}

fn resolve_hostname(settings : &mut Options) {
    for c in settings.hostname.chars() {
        if c.is_alphabetic() && c != '.' && c != ':' {
            let ips = match lookup_host(&settings.hostname) {
                Err(e) => {
                    println!("Failed to lookup {}: {}", settings.hostname, e);
                    std::process::exit(1);
                }
                Ok(i) => i,
            };
            println!("Resolved hostname {} to {}", settings.hostname, ips[0]);
            settings.hostname = String::from(ips[0].to_string());
            break;
        }
    }
    if !settings.server {
        let interfaces = datalink::interfaces();
        let interface = interfaces.iter().find(|i| i.is_up() && !i.is_loopback() && !i.ips.is_empty());
        let interface = match interface {
            Some(i) => i,
            None => {
                settings.local_hostname.push_str("127.0.0.1");
                return;
            },
        };
        let ip = interface.ips[0].ip();
        settings.local_hostname.push_str(&ip.to_string());
        // println!("Found IP: {}", settings.local_hostname);
    }
}