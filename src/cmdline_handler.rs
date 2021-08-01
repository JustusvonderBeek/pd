/* The commandline options */
#[derive(Clone)]
pub struct Options {
    pub local_port: u32,
    pub remote_port: u32,
    pub server: bool,
    pub hostname: String,
    pub p: u32,
    pub q: u32,
    pub filename: Vec<String>,
    pub logfile: String,
}

fn default_options() -> Options {
    let set = Options {
        local_port: 5001,
        remote_port: 5001,
        server: false,
        hostname: String::from("127.0.0.1"),
        p: 0,
        q: 0,
        filename: vec![],
        logfile: String::from("tbd.log"),
    };
    set
}

fn print_help() {
    println!("RFT - Robust File Transfer:");
    println!("Usage (Server):");
    println!("rft [-s] [-t <port>] [-p <p>] [-q <q>]");
    println!("Usage (Client):");
    println!("rft <host> [-t <port>] [-p <p>] [-q <q>] <file> ...");
    println!("Options:");
    println!("-s: servermode: accept incoming requests from any host. Operates in client mode if “–s” is not specified.");
    println!("-t: specify the port number to use (default: 5001");
    println!("-p, -q: specify the loss probabilities for the Markov chain model. If only one is specified, p=q is assumed; if neither is specified no loss is assumed.");
    println!("<file> the name(s) of the file(s) to fetch.");
}

pub fn parse_cmdline(args : Vec<String>) -> Option<Options> {
    if args.len() < 2 {
        error!("The given arguments does not contain any file!");
    } else {
        let mut settings = default_options();
        let mut i = 1; // Skip the path
        while i < args.len() {
            let str = args.get(i).unwrap();
            match str.as_str() {
                "-h" => {
                    print_help();
                    std::process::exit(0);
                },
                "-s" => {
                    settings.server = true; // Enabling server mode
                },
                "-t" => {
                    // Expecting a port to operate on
                    let port = args.get(i + 1).expect("Expected a port but got nothing!");
                    settings.local_port = port.parse::<u32>().unwrap();
                    i += 1;
                },
                "-p" => {
                    let p = args.get(i + 1).expect("Expected a probability p but got nothing!");
                    settings.p = p.parse::<u32>().unwrap();
                    i += 1;
                },
                "-q" => {
                    let q = args.get(i + 1).expect("Expected a probability q but got nothing!");
                    settings.q = q.parse::<u32>().unwrap();
                    i += 1;
                },
                _ => {
                    // TODO: Allow for more than one file
                    if settings.server {
                        error!("Cannot transfer a file in server mode!");
                        return None;
                    }
                    info!("File: {}", str);
                    settings.filename.push(str.to_string());
                },
            }
            i += 1;
        }
        return Some(settings)
    }
    None
}